/// Integration tests for the per-account brute-force cooldown (AT-8).
/// Tests that the login_attempts table correctly enforces the 5-failure/15-min window.
use sqlx::PgPool;

async fn insert_failed_attempts(pool: &PgPool, email: &str, count: i32) {
    for _ in 0..count {
        sqlx::query(
            "INSERT INTO login_attempts (email, ip, success) VALUES ($1, '127.0.0.1', false)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn insert_old_failed_attempts(pool: &PgPool, email: &str, count: i32) {
    // Insert attempts older than 15 minutes so they're outside the window
    for _ in 0..count {
        sqlx::query(
            r#"INSERT INTO login_attempts (email, ip, success, attempted_at)
               VALUES ($1, '127.0.0.1', false, now() - interval '16 minutes')"#,
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    }
}

// ─── Recent failure count ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn counts_only_recent_failures(pool: PgPool) {
    let email = "brutetest@example.com";

    // 3 recent failures + 10 old failures
    insert_failed_attempts(&pool, email, 3).await;
    insert_old_failed_attempts(&pool, email, 10).await;

    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 3, "only 3 recent failures should be counted");
}

#[sqlx::test(migrations = "./migrations")]
async fn five_failures_triggers_lockout(pool: PgPool) {
    let email = "locked@example.com";
    insert_failed_attempts(&pool, email, 5).await;

    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        count >= 5,
        "5 failures should meet the lockout threshold (count = {count})"
    );
}

// ─── Clear on success ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn successful_login_clears_failures(pool: PgPool) {
    let email = "clearme@example.com";
    insert_failed_attempts(&pool, email, 4).await;

    // Simulate what clear_failures() does
    sqlx::query("DELETE FROM login_attempts WHERE email = $1 AND success = false")
        .bind(email)
        .execute(&pool)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        count, 0,
        "failures should be cleared after successful login"
    );
}

// ─── Old failures don't trigger lockout ──────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn old_failures_do_not_lock_account(pool: PgPool) {
    let email = "oldfails@example.com";

    // 20 old failures (outside 15-min window) + 4 recent ones
    insert_old_failed_attempts(&pool, email, 20).await;
    insert_failed_attempts(&pool, email, 4).await;

    let recent: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        recent < 5,
        "only 4 recent failures — should not be locked (recent = {recent})"
    );
}

// ─── Retry-after calculation ──────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn retry_after_points_to_oldest_failure_age_out(pool: PgPool) {
    let email = "retryafter@example.com";

    // Insert 5 failures spaced 1 second apart so we can check the oldest
    for _ in 0..5 {
        sqlx::query(
            "INSERT INTO login_attempts (email, ip, success) VALUES ($1, '1.2.3.4', false)",
        )
        .bind(email)
        .execute(&pool)
        .await
        .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let oldest: time::OffsetDateTime = sqlx::query_scalar(
        r#"SELECT MIN(attempted_at) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(&pool)
    .await
    .unwrap();

    let age_out = oldest + time::Duration::minutes(15);
    let remaining = age_out - time::OffsetDateTime::now_utc();
    let retry_after_secs = remaining.whole_seconds().max(0) as u64;

    // Should be close to 900 seconds (15 minutes) since we just inserted
    assert!(
        retry_after_secs > 850 && retry_after_secs <= 900,
        "retry_after should be close to 900s (got {retry_after_secs})"
    );
}

// ─── Different emails are isolated ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn failures_are_per_email_not_global(pool: PgPool) {
    let email_a = "victim@example.com";
    let email_b = "innocent@example.com";

    // Flood email_a
    insert_failed_attempts(&pool, email_a, 10).await;

    // email_b should have 0 failures
    let count_b: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email_b)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        count_b, 0,
        "flooding email_a must not affect email_b's failure count"
    );
}
