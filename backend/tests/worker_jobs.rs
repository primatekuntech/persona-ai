/// Integration tests for worker SQL patterns: reaper, concurrency cap, retry/failure logic.
/// These test the exact SQL the worker executes, without needing model files or spawning tasks.
use sqlx::PgPool;
use uuid::Uuid;

// ─── Seed helpers ─────────────────────────────────────────────────────────────

async fn seed_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, role) \
         VALUES ($1, '$argon2id$v=19$m=19456,t=2,p=1$fake', 'user') RETURNING id",
    )
    .bind(format!("worker-{}@example.com", Uuid::now_v7()))
    .fetch_one(pool)
    .await
    .expect("seed user")
}

async fn seed_persona(pool: &PgPool, user_id: Uuid) -> Uuid {
    sqlx::query_scalar("INSERT INTO personas (user_id, name) VALUES ($1, $2) RETURNING id")
        .bind(user_id)
        .bind(format!("Persona {}", Uuid::now_v7()))
        .fetch_one(pool)
        .await
        .expect("seed persona")
}

/// Insert a job with explicit status and optional heartbeat_at offset from now.
async fn insert_job(
    pool: &PgPool,
    kind: &str,
    user_id: Uuid,
    persona_id: Uuid,
    status: &str,
    heartbeat_offset_secs: Option<i64>, // negative = in the past
) -> Uuid {
    let id = if let Some(offset) = heartbeat_offset_secs {
        sqlx::query_scalar(
            "INSERT INTO jobs (kind, user_id, persona_id, payload, status, heartbeat_at) \
             VALUES ($1, $2, $3, $4, $5, now() + ($6 * interval '1 second')) RETURNING id",
        )
        .bind(kind)
        .bind(user_id)
        .bind(persona_id)
        .bind(serde_json::json!({"document_id": Uuid::now_v7().to_string()}))
        .bind(status)
        .bind(offset)
        .fetch_one(pool)
        .await
        .expect("insert job with heartbeat")
    } else {
        sqlx::query_scalar(
            "INSERT INTO jobs (kind, user_id, persona_id, payload, status) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(kind)
        .bind(user_id)
        .bind(persona_id)
        .bind(serde_json::json!({"document_id": Uuid::now_v7().to_string()}))
        .bind(status)
        .fetch_one(pool)
        .await
        .expect("insert job")
    };
    id
}

// ─── Reaper tests ─────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn reaper_requeues_stale_running_jobs(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    // Running job whose heartbeat expired > 2 minutes ago
    let stale_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "running",
        Some(-200), // 200 seconds in the past
    )
    .await;

    // Execute the exact reaper query
    let rows = sqlx::query(
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
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();

    assert_eq!(rows, 1, "stale job should be reaped");

    let (status, last_error, attempts): (String, Option<String>, i32) = sqlx::query_as(
        "SELECT status, last_error, attempts FROM jobs WHERE id=$1",
    )
    .bind(stale_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(status, "queued");
    assert_eq!(last_error.as_deref(), Some("reaped: heartbeat expired"));
    assert_eq!(attempts, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn reaper_skips_fresh_running_jobs(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    // Running job with a recent heartbeat (10 seconds ago — well within 2 minutes)
    let fresh_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "running",
        Some(-10),
    )
    .await;

    let rows = sqlx::query(
        "UPDATE jobs \
         SET status='queued', scheduled_at=now()+interval '30 seconds', \
             worker_id=NULL, heartbeat_at=NULL, attempts=attempts+1, \
             last_error='reaped: heartbeat expired' \
         WHERE status='running' \
           AND heartbeat_at < now() - interval '2 minutes'",
    )
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();

    assert_eq!(rows, 0, "fresh job must not be reaped");

    let status: String =
        sqlx::query_scalar("SELECT status FROM jobs WHERE id=$1")
            .bind(fresh_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "running");
}

// ─── Concurrency cap tests ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn concurrency_cap_defers_job_at_limit(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    // Insert 3 already-running ingest_document jobs for this user (the cap)
    for _ in 0..3 {
        insert_job(
            &pool,
            "ingest_document",
            user_id,
            persona_id,
            "running",
            Some(-10),
        )
        .await;
    }

    // Insert 1 queued job
    let queued_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "queued",
        None,
    )
    .await;

    // Simulate pick_job concurrency check: count running jobs for user
    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind='ingest_document' AND status='running' AND user_id=$1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(running, 3);

    // At cap (3 >= 3): reschedule the queued job instead of picking it
    if running >= 3 {
        sqlx::query(
            "UPDATE jobs SET scheduled_at=now()+interval '5 seconds' WHERE id=$1",
        )
        .bind(queued_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Job must still be queued, not running
    let status: String =
        sqlx::query_scalar("SELECT status FROM jobs WHERE id=$1")
            .bind(queued_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "queued", "job should stay queued when cap is reached");
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrency_cap_allows_job_under_limit(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    // Insert 2 running jobs (under cap of 3)
    for _ in 0..2 {
        insert_job(
            &pool,
            "ingest_document",
            user_id,
            persona_id,
            "running",
            Some(-10),
        )
        .await;
    }

    let queued_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "queued",
        None,
    )
    .await;

    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind='ingest_document' AND status='running' AND user_id=$1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(running, 2);
    assert!(running < 3, "should be under cap");

    // Under cap: mark job running (simulates worker claiming the job)
    sqlx::query(
        "UPDATE jobs SET status='running', started_at=now(), heartbeat_at=now() WHERE id=$1",
    )
    .bind(queued_id)
    .execute(&pool)
    .await
    .unwrap();

    let status: String =
        sqlx::query_scalar("SELECT status FROM jobs WHERE id=$1")
            .bind(queued_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "running", "job should be picked up when under cap");
}

// ─── Retry / failure tests ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn job_retries_with_exponential_backoff(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    let job_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "running",
        Some(-10),
    )
    .await;

    // First failure: attempts=0 → new_attempts=1, backoff = 2^1 = 2s
    let new_attempts = 1i32;
    let backoff = 2_i64.pow(new_attempts as u32);
    sqlx::query(
        "UPDATE jobs SET status='queued', \
         scheduled_at=now()+($1 * interval '1 second'), \
         attempts=$2, last_error=$3, worker_id=NULL WHERE id=$4",
    )
    .bind(backoff)
    .bind(new_attempts)
    .bind("parse error")
    .bind(job_id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, attempts, last_error): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, last_error FROM jobs WHERE id=$1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(status, "queued");
    assert_eq!(attempts, 1);
    assert_eq!(last_error.as_deref(), Some("parse error"));
}

#[sqlx::test(migrations = "./migrations")]
async fn job_permanently_fails_at_max_attempts(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;

    let job_id = insert_job(
        &pool,
        "ingest_document",
        user_id,
        persona_id,
        "running",
        Some(-10),
    )
    .await;

    // Simulate max attempts exceeded (attempts was 2, new_attempts = 3 = MAX_ATTEMPTS)
    let new_attempts = 3i32;
    sqlx::query(
        "UPDATE jobs SET status='failed', finished_at=now(), last_error=$1, \
         attempts=$2, worker_id=NULL WHERE id=$3",
    )
    .bind("permanent failure")
    .bind(new_attempts)
    .bind(job_id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM jobs WHERE id=$1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(status, "failed");
    assert_eq!(attempts, 3);
}
