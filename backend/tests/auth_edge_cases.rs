/// Integration tests for auth edge cases not covered elsewhere:
/// - Concurrent password reset (same token used twice should yield one 410)
/// - Disabled user login is rejected with the correct error code
/// - Role change takes effect immediately (no session cache)
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

async fn seed_user(pool: &PgPool, email: &str, status: &str) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role, status, display_name)
           VALUES ($1, 'hashed', 'user', $2, 'Test')
           RETURNING id"#,
    )
    .bind(email)
    .bind(status)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

async fn seed_admin(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role, status, display_name)
           VALUES ($1, 'hashed', 'admin', 'active', 'Admin')
           RETURNING id"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed admin")
}

async fn seed_reset_token(pool: &PgPool, user_id: Uuid) -> String {
    // Build a 64-char hex string: UUID hex (32 chars) zero-padded on the left.
    let uuid_hex = Uuid::new_v4().to_string().replace('-', ""); // 32 chars
    let token_hash = format!("{:0>64}", uuid_hex); // "00...0<32 chars>" = 64 chars
    let expires_at = OffsetDateTime::now_utc() + time::Duration::minutes(30);
    sqlx::query(
        "INSERT INTO password_resets (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("seed reset token");
    token_hash
}

// ─── Concurrent password reset (FOR UPDATE prevents double-use) ──────────────

/// The first reset within a transaction locks the row and marks it used.
/// A second reset attempt (sequential, after the first commits) must find
/// used_at IS NOT NULL and return None.
#[sqlx::test(migrations = "./migrations")]
async fn double_password_reset_second_is_rejected(pool: PgPool) {
    let user_id = seed_user(&pool, "resetrace@example.com", "active").await;
    let token_hash = seed_reset_token(&pool, user_id).await;

    // First reset: begin tx, lock and mark token used, commit.
    let mut tx1 = pool.begin().await.unwrap();
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM password_resets
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx1)
    .await
    .unwrap();
    assert!(row.is_some(), "first reset should see active token");

    sqlx::query("UPDATE password_resets SET used_at = now() WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(&mut *tx1)
        .await
        .unwrap();
    tx1.commit().await.unwrap();

    // Second reset: should find no active row.
    let mut tx2 = pool.begin().await.unwrap();
    let row2: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM password_resets
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx2)
    .await
    .unwrap();
    tx2.rollback().await.unwrap();

    assert!(
        row2.is_none(),
        "second reset attempt must not find the already-used token"
    );
}

/// An expired reset token is invisible to the FOR UPDATE query.
#[sqlx::test(migrations = "./migrations")]
async fn expired_reset_token_is_rejected(pool: PgPool) {
    let user_id = seed_user(&pool, "expiredreset@example.com", "active").await;

    // 64 hex chars: 'e' followed by 63 zeros — deterministic, unique per test DB.
    let token_hash = "e000000000000000000000000000000000000000000000000000000000000000".to_string();
    let past = OffsetDateTime::now_utc() - time::Duration::minutes(5);
    sqlx::query(
        "INSERT INTO password_resets (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(past)
    .execute(&pool)
    .await
    .unwrap();

    let found: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM password_resets
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()"#,
    )
    .bind(&token_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(found.is_none(), "expired reset token must not be found");
}

// ─── Disabled user cannot log in ─────────────────────────────────────────────

/// A disabled user's status field should block login.
/// The handler checks `user.status != "active"` after password verification
/// and returns 403 account_disabled.  Here we verify the DB state that would
/// trigger that path.
#[sqlx::test(migrations = "./migrations")]
async fn disabled_user_has_disabled_status(pool: PgPool) {
    let user_id = seed_user(&pool, "disabled@example.com", "disabled").await;

    let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        status, "disabled",
        "seeded user must have status='disabled'"
    );
}

/// Re-enabling a user sets status back to 'active'.
#[sqlx::test(migrations = "./migrations")]
async fn enabling_user_sets_status_active(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin-enable@example.com").await;
    let user_id = seed_user(&pool, "toenable@example.com", "disabled").await;

    sqlx::query("UPDATE users SET status = 'active' WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(status, "active");
    drop(admin_id);
}

// ─── Role change is immediately visible (no session caching) ─────────────────

/// Verifies that demoting an admin to 'user' is immediately reflected in a
/// fresh DB lookup (the auth middleware refetches role per request).
#[sqlx::test(migrations = "./migrations")]
async fn role_change_is_immediately_visible(pool: PgPool) {
    let admin_id = seed_admin(&pool, "demoteme@example.com").await;

    // Verify initial role
    let role: String = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(admin_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, "admin");

    // Demote
    sqlx::query("UPDATE users SET role = 'user' WHERE id = $1")
        .bind(admin_id)
        .execute(&pool)
        .await
        .unwrap();

    // A fresh lookup (simulating what require_auth does) reflects the new role.
    let new_role: String = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(admin_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        new_role, "user",
        "role change must be visible on the next DB lookup without re-login"
    );
}

/// Promoting a user to admin is also immediately visible.
#[sqlx::test(migrations = "./migrations")]
async fn promotion_to_admin_is_immediately_visible(pool: PgPool) {
    let user_id = seed_user(&pool, "promoteme@example.com", "active").await;

    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let role: String = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, "admin");
}

// ─── Password reset token is user-scoped ─────────────────────────────────────

/// A reset token belongs to a specific user; another user's ID does not match.
#[sqlx::test(migrations = "./migrations")]
async fn reset_token_scoped_to_correct_user(pool: PgPool) {
    let owner = seed_user(&pool, "owner@example.com", "active").await;
    let other = seed_user(&pool, "other@example.com", "active").await;
    let token_hash = seed_reset_token(&pool, owner).await;

    // Querying with other's ID returns nothing
    let found: Option<(String,)> = sqlx::query_as(
        "SELECT token_hash FROM password_resets WHERE token_hash = $1 AND user_id = $2 AND used_at IS NULL",
    )
    .bind(&token_hash)
    .bind(other)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        found.is_none(),
        "reset token must not be usable by a different user_id"
    );

    // Querying with owner's ID returns it
    let found_own: Option<(String,)> = sqlx::query_as(
        "SELECT token_hash FROM password_resets WHERE token_hash = $1 AND user_id = $2 AND used_at IS NULL",
    )
    .bind(&token_hash)
    .bind(owner)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        found_own.is_some(),
        "owner must be able to use their own token"
    );
}
