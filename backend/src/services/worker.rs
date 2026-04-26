/// Background job worker pool. Polls the `jobs` table using `SELECT ... FOR UPDATE SKIP LOCKED`.
use crate::{repositories::documents as doc_repo, services::ingest, state::AppState};
use sqlx::FromRow;
use std::{io::Write, time::Duration};
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
        "user_export" => {
            let user_id = job
                .user_id
                .ok_or_else(|| "user_export: missing user_id".to_string())?;
            let job_id = job.id;
            run_user_export(state, user_id, job_id)
                .await
                .map_err(|e| e.to_string())
        }
        "user_delete" => {
            let user_id = job
                .user_id
                .ok_or_else(|| "user_delete: missing user_id".to_string())?;
            run_user_delete(state, user_id)
                .await
                .map_err(|e| e.to_string())
        }
        other => {
            tracing::warn!(kind=%other, "unknown job kind — skipping");
            Ok(())
        }
    }
}

// ─── user_export ──────────────────────────────────────────────────────────────

async fn run_user_export(
    state: &AppState,
    user_id: Uuid,
    job_id: Uuid,
) -> Result<(), anyhow::Error> {
    use zip::{write::FileOptions, ZipWriter};

    // Update progress: started
    update_job_payload(state, job_id, serde_json::json!({ "progress_pct": 0 })).await;

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buf);
    let options = FileOptions::<()>::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. account.json
    let account_json: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT row_to_json(u) FROM \
         (SELECT id, email, role, status, display_name, created_at, last_login_at \
          FROM users WHERE id=$1) u",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    zip.start_file("account.json", options)?;
    zip.write_all(
        serde_json::to_vec_pretty(&account_json.unwrap_or(serde_json::Value::Null))?.as_slice(),
    )?;

    // 2. Personas
    let persona_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM personas WHERE user_id=$1 ORDER BY created_at")
            .bind(user_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

    let total = persona_ids.len().max(1) as i64;

    for (i, persona_id) in persona_ids.iter().enumerate() {
        // persona.json
        let persona_json: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT row_to_json(p) FROM \
             (SELECT id, name, relation, description, birth_year, created_at, updated_at \
              FROM personas WHERE id=$1) p",
        )
        .bind(persona_id)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);

        let eras_json: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT row_to_json(e) FROM \
             (SELECT id, label, start_date, end_date, description, created_at \
              FROM eras WHERE persona_id=$1 ORDER BY start_date NULLS LAST) e",
        )
        .bind(persona_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let style_json: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT row_to_json(sp) FROM \
             (SELECT id, era_id, tone, vocabulary, themes, updated_at \
              FROM style_profiles WHERE persona_id=$1) sp",
        )
        .bind(persona_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let persona_entry = serde_json::json!({
            "persona": persona_json,
            "eras": eras_json,
            "style_profiles": style_json,
        });

        zip.start_file(format!("personas/{persona_id}/persona.json"), options)?;
        zip.write_all(serde_json::to_vec_pretty(&persona_entry)?.as_slice())?;

        // documents.json manifest
        let docs_json: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT row_to_json(d) FROM \
             (SELECT id, kind, mime_type, title, source, word_count, duration_sec, \
                     status, created_at, ingested_at \
              FROM documents WHERE persona_id=$1 ORDER BY created_at) d",
        )
        .bind(persona_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        zip.start_file(format!("personas/{persona_id}/documents.json"), options)?;
        zip.write_all(serde_json::to_vec_pretty(&docs_json)?.as_slice())?;

        // Per-document files — read from disk inside spawn_blocking
        let doc_rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, original_path, mime_type FROM documents WHERE persona_id=$1",
        )
        .bind(persona_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        for (doc_id, original_path, _mime) in &doc_rows {
            let path_owned = original_path.clone();
            let doc_id_owned = *doc_id;
            let read_result = tokio::task::spawn_blocking(move || {
                let p = std::path::Path::new(&path_owned);
                if p.exists() {
                    std::fs::read(p).ok().map(|b| {
                        let ext = p
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("bin")
                            .to_owned();
                        (b, ext)
                    })
                } else {
                    None
                }
            })
            .await
            .unwrap_or(None);

            match read_result {
                Some((bytes, ext)) => {
                    zip.start_file(
                        format!("personas/{persona_id}/documents/{doc_id_owned}.{ext}"),
                        options,
                    )?;
                    zip.write_all(&bytes)?;
                }
                None => {
                    tracing::warn!(doc_id=%doc_id, path=%original_path, "user_export: document file not found or unreadable, skipping");
                }
            }
        }

        // chats
        let sessions: Vec<(Uuid,)> =
            sqlx::query_as("SELECT id FROM chat_sessions WHERE persona_id=$1 ORDER BY created_at")
                .bind(persona_id)
                .fetch_all(&state.db)
                .await
                .unwrap_or_default();

        for (session_id,) in sessions {
            let messages_json: Vec<serde_json::Value> = sqlx::query_scalar(
                "SELECT row_to_json(m) FROM \
                 (SELECT id, role, content, created_at FROM messages \
                  WHERE session_id=$1 ORDER BY created_at) m",
            )
            .bind(session_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            let session_json_val: Option<serde_json::Value> = sqlx::query_scalar(
                "SELECT row_to_json(s) FROM \
                 (SELECT id, title, created_at FROM chat_sessions WHERE id=$1) s",
            )
            .bind(session_id)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);

            let chat_entry = serde_json::json!({
                "session": session_json_val,
                "messages": messages_json,
            });

            zip.start_file(
                format!("personas/{persona_id}/chats/{session_id}.json"),
                options,
            )?;
            zip.write_all(serde_json::to_vec_pretty(&chat_entry)?.as_slice())?;
        }

        // Update progress
        let pct = ((i as i64 + 1) * 90 / total).min(90);
        update_job_payload(state, job_id, serde_json::json!({ "progress_pct": pct })).await;
    }

    // 3. audit_log.json
    let audit_json: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT row_to_json(a) FROM \
         (SELECT id, action, resource_type, resource_id, ip, metadata, created_at \
          FROM audit_log WHERE user_id=$1 ORDER BY created_at) a",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    zip.start_file("audit_log.json", options)?;
    zip.write_all(serde_json::to_vec_pretty(&audit_json)?.as_slice())?;

    let cursor = zip.finish()?;
    let zip_bytes = cursor.into_inner();

    // Write to disk: /data/exports/<user_id>/<job_id>.zip
    let exports_dir = state
        .config
        .data_dir
        .join("exports")
        .join(user_id.to_string());
    tokio::fs::create_dir_all(&exports_dir).await?;
    let export_path = exports_dir.join(format!("{job_id}.zip"));
    tokio::fs::write(&export_path, &zip_bytes).await?;

    update_job_payload(
        state,
        job_id,
        serde_json::json!({
            "progress_pct": 100,
            "export_path": export_path.to_string_lossy()
        }),
    )
    .await;

    tracing::info!(user_id=%user_id, job_id=%job_id, path=%export_path.display(), "user_export: complete");
    Ok(())
}

async fn update_job_payload(state: &AppState, job_id: Uuid, payload: serde_json::Value) {
    let _ = sqlx::query("UPDATE jobs SET payload = payload || $1::jsonb WHERE id = $2")
        .bind(payload)
        .bind(job_id)
        .execute(&state.db)
        .await;
}

// ─── user_delete ──────────────────────────────────────────────────────────────

async fn run_user_delete(state: &AppState, user_id: Uuid) -> Result<(), anyhow::Error> {
    // Re-check last-admin guard inside the job to close the TOCTOU window (spec §7.23)
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM users WHERE id=$1 AND status='disabled'")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);

    if role.as_deref() == Some("admin") {
        let active_admins: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin' AND status='active'")
                .fetch_one(&state.db)
                .await
                .unwrap_or(1);
        if active_admins == 0 {
            return Err(anyhow::anyhow!(
                "user_delete: aborted — would leave no active admins"
            ));
        }
    }

    // Fetch all persona IDs for user
    let persona_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM personas WHERE user_id=$1")
        .bind(user_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    for persona_id in &persona_ids {
        // Delete upload files
        let uploads_dir = state
            .config
            .data_dir
            .join("uploads")
            .join(persona_id.to_string());
        if uploads_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&uploads_dir) {
                tracing::warn!(error=%e, path=%uploads_dir.display(), "user_delete: failed to remove uploads dir");
            }
        }

        // Delete avatar file (any extension)
        let avatars_dir = state.config.data_dir.join("avatars");
        if avatars_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&avatars_dir) {
                for entry in rd.flatten() {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy();
                    if fname_str.starts_with(&persona_id.to_string()) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }

    // Delete exports
    let exports_dir = state
        .config
        .data_dir
        .join("exports")
        .join(user_id.to_string());
    if exports_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&exports_dir) {
            tracing::warn!(error=%e, path=%exports_dir.display(), "user_delete: failed to remove exports dir");
        }
    }

    // Hard delete user row — cascade handles everything else
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    tracing::info!(user_id=%user_id, "user_delete: complete");
    Ok(())
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
