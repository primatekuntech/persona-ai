/// Background job worker pool. Polls the `jobs` table using `SELECT ... FOR UPDATE SKIP LOCKED`.
use crate::{repositories::documents as doc_repo, services::ingest, state::AppState};
use sqlx::FromRow;
use std::time::Duration;
use uuid::Uuid;

const MAX_CONCURRENT_INGEST_PER_USER: i64 = 3;
const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const POLL_INTERVAL_SECS: u64 = 1;
const REAPER_INTERVAL_SECS: u64 = 60;
const MAX_ATTEMPTS: i32 = 3;

#[derive(Debug, FromRow)]
struct JobRow {
    id: Uuid,
    kind: String,
    user_id: Option<Uuid>,
    persona_id: Option<Uuid>,
    payload: serde_json::Value,
    attempts: i32,
}

/// Spawn `num_workers` async tasks polling `jobs`, plus one reaper task.
pub fn start_workers(state: AppState, num_workers: usize) {
    let num_workers = num_workers.max(1);

    for worker_idx in 0..num_workers {
        let state_clone = state.clone();
        tokio::spawn(async move {
            run_worker(state_clone, worker_idx).await;
        });
    }

    // Reaper task
    let state_clone = state.clone();
    tokio::spawn(async move {
        run_reaper(state_clone).await;
    });
}

async fn run_worker(state: AppState, worker_idx: usize) {
    let worker_id = format!("{}:{}:{}", hostname(), std::process::id(), worker_idx);

    loop {
        match pick_job(&state, &worker_id).await {
            Ok(Some(job)) => {
                execute_job(&state, job, &worker_id).await;
            }
            Ok(None) => {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
            Err(e) => {
                tracing::error!(error = %e, "worker: job pick error");
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }
    }
}

async fn pick_job(state: &AppState, worker_id: &str) -> Result<Option<JobRow>, sqlx::Error> {
    let mut tx = state.db.begin().await?;

    let job: Option<JobRow> = sqlx::query_as(
        "SELECT id, kind, user_id, persona_id, payload, attempts FROM jobs \
         WHERE status = 'queued' AND scheduled_at <= now() \
         ORDER BY scheduled_at ASC \
         FOR UPDATE SKIP LOCKED \
         LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?;

    let job = match job {
        Some(j) => j,
        None => {
            tx.rollback().await.ok();
            return Ok(None);
        }
    };

    // Per-user concurrency cap for ingest_document jobs
    if job.kind == "ingest_document" {
        if let Some(uid) = job.user_id {
            let running: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM jobs WHERE kind='ingest_document' AND status='running' AND user_id=$1",
            )
            .bind(uid)
            .fetch_one(&mut *tx)
            .await?;

            if running >= MAX_CONCURRENT_INGEST_PER_USER {
                // Reschedule in 5 seconds and release
                sqlx::query(
                    "UPDATE jobs SET scheduled_at = now() + interval '5 seconds' WHERE id = $1",
                )
                .bind(job.id)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                return Ok(None);
            }
        }
    }

    // Mark running
    sqlx::query(
        "UPDATE jobs SET status='running', started_at=now(), worker_id=$1, heartbeat_at=now() WHERE id=$2",
    )
    .bind(worker_id)
    .bind(job.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(job))
}

async fn execute_job(state: &AppState, job: JobRow, _worker_id: &str) {
    let job_id = job.id;

    // Spawn heartbeat task
    let db_clone = state.db.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        interval.tick().await; // first tick is immediate, skip it
        loop {
            interval.tick().await;
            if let Err(e) =
                sqlx::query("UPDATE jobs SET heartbeat_at=now() WHERE id=$1 AND status='running'")
                    .bind(job_id)
                    .execute(&db_clone)
                    .await
            {
                tracing::warn!(job_id=%job_id, error=%e, "heartbeat update failed");
            }
        }
    });

    let result = dispatch(state, &job).await;
    heartbeat_handle.abort();

    match result {
        Ok(()) => {
            if let Err(e) = sqlx::query(
                "UPDATE jobs SET status='done', finished_at=now(), worker_id=NULL WHERE id=$1",
            )
            .bind(job_id)
            .execute(&state.db)
            .await
            {
                tracing::error!(job_id=%job_id, error=%e, "failed to mark job done");
            }
            tracing::info!(job_id=%job_id, kind=%job.kind, "job completed");
        }
        Err(err_msg) => {
            let new_attempts = job.attempts + 1;
            if new_attempts >= MAX_ATTEMPTS {
                // Mark failed permanently
                let _ = sqlx::query(
                    "UPDATE jobs SET status='failed', finished_at=now(), last_error=$1, \
                     attempts=$2, worker_id=NULL WHERE id=$3",
                )
                .bind(&err_msg)
                .bind(new_attempts)
                .bind(job_id)
                .execute(&state.db)
                .await;

                // Mark the document as failed
                if let Some(doc_id) = extract_document_id(&job.payload) {
                    let _ =
                        doc_repo::update_status(&state.db, doc_id, "failed", None, Some(&err_msg))
                            .await;

                    if let (Some(uid), Some(pid)) = (job.user_id, job.persona_id) {
                        let event = crate::services::broadcast::IngestEvent {
                            user_id: uid,
                            persona_id: pid,
                            document_id: doc_id,
                            status: "failed".into(),
                            progress_pct: None,
                            error: Some(err_msg.clone()),
                        };
                        let _ = state.ingest_tx.send(event);
                    }
                }
                tracing::error!(job_id=%job_id, kind=%job.kind, error=%err_msg, "job permanently failed");
            } else {
                // Exponential backoff: 2^attempts seconds
                let backoff = 2_i64.pow(new_attempts as u32);
                let _ = sqlx::query(
                    "UPDATE jobs SET status='queued', scheduled_at=now()+($1 * interval '1 second'), \
                     attempts=$2, last_error=$3, worker_id=NULL WHERE id=$4",
                )
                .bind(backoff)
                .bind(new_attempts)
                .bind(&err_msg)
                .bind(job_id)
                .execute(&state.db)
                .await;
                tracing::warn!(
                    job_id=%job_id, kind=%job.kind, attempt=new_attempts,
                    backoff_sec=backoff, error=%err_msg, "job failed, will retry"
                );
            }
        }
    }
}

async fn dispatch(state: &AppState, job: &JobRow) -> Result<(), String> {
    match job.kind.as_str() {
        "ingest_document" => {
            let doc_id = extract_document_id(&job.payload)
                .ok_or_else(|| "ingest_document job missing document_id in payload".to_string())?;

            ingest::run_ingest(&state.db, &state.config, doc_id, &state.ingest_tx)
                .await
                .map_err(|e| e.to_string())
        }
        "recompute_profile" => {
            let persona_id = job.payload["persona_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| "recompute_profile missing persona_id".to_string())?;
            let era_id = job.payload["era_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok());

            crate::services::analysis_runner::run_recompute_profile(&state.db, persona_id, era_id)
                .await
                .map_err(|e| e.to_string())
        }
        other => {
            tracing::warn!(kind=%other, "unknown job kind — skipping");
            Ok(())
        }
    }
}

fn extract_document_id(payload: &serde_json::Value) -> Option<Uuid> {
    payload["document_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
}

async fn run_reaper(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(REAPER_INTERVAL_SECS));
    loop {
        interval.tick().await;
        match sqlx::query(
            "UPDATE jobs \
             SET status='queued', \
                 scheduled_at=now()+interval '30 seconds', \
                 worker_id=NULL, \
                 heartbeat_at=NULL, \
                 attempts=attempts+1, \
                 last_error='reaped: heartbeat expired' \
             WHERE status='running' \
               AND heartbeat_at < now() - interval '2 minutes'",
        )
        .execute(&state.db)
        .await
        {
            Ok(r) if r.rows_affected() > 0 => {
                tracing::warn!(reaped=%r.rows_affected(), "reaper: re-queued stuck jobs");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(error=%e, "reaper: update failed");
            }
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into())
}
