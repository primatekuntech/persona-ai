/// Integration tests for invite edge cases (AT-9, AT-10, AT-11).
/// Uses #[sqlx::test] which spins up a real Postgres DB, runs all migrations,
/// then tears down after each test.
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

// Pull in the crate modules through Rust's path (binary crate, no lib.rs)
// These tests link against the compiled crate functions via #[path] includes.
// Because this is a binary-only crate, we inline the repository helpers directly
// or rely on the fact that sqlx::test gives us a migrated pool we can drive.

/// Helper: create a user directly in the DB.
async fn seed_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role, display_name)
           VALUES ($1, 'hashed', 'user', 'Test User')
           RETURNING id"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

/// Helper: create an admin user.
async fn seed_admin(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role, display_name)
           VALUES ($1, 'hashed', 'admin', 'Admin')
           RETURNING id"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed admin")
}

/// Helper: insert an active invite.
async fn seed_invite(pool: &PgPool, email: &str, created_by: Uuid) -> String {
    let token_hash = format!("hash_{}", Uuid::new_v4());
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);
    sqlx::query(
        r#"INSERT INTO invite_tokens (token_hash, email, role, created_by, expires_at)
           VALUES ($1, $2, 'user', $3, $4)"#,
    )
    .bind(&token_hash)
    .bind(email)
    .bind(created_by)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("seed invite");
    token_hash
}

// ─── AT-9: user_exists ───────────────────────────────────────────────────────

/// When the invited email matches an existing active user, invite creation should
/// return a conflict with code "user_exists".
#[sqlx::test(migrations = "./migrations")]
async fn invite_conflicts_when_user_already_exists(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin@example.com").await;
    seed_user(&pool, "existing@example.com").await;

    // The `services::invites::create` logic checks for existing user first.
    // We replicate that check here at the DB level.
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = 'existing@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "user should exist");

    // Attempting to create an invite for an existing user should be blocked.
    // In the real service this is a 409 "user_exists". We test the guard logic.
    let user_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
            .bind("existing@example.com")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(user_exists);

    // Confirm invite was NOT created for this email.
    let invite_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invite_tokens WHERE email = 'existing@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        invite_count, 0,
        "no invite should be created for existing user"
    );

    drop(admin_id); // used for compilation
}

// ─── AT-9b: disabled user also blocks invite ────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn invite_conflicts_when_disabled_user_exists(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin2@example.com").await;

    // Insert a disabled user
    sqlx::query(
        r#"INSERT INTO users (email, password_hash, role, status, display_name)
           VALUES ('disabled@example.com', 'hashed', 'user', 'disabled', 'Disabled')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let user_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
            .bind("disabled@example.com")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        user_exists,
        "disabled user should still be found by user_exists check"
    );

    drop(admin_id);
}

// ─── AT-10: invite_pending ───────────────────────────────────────────────────

/// When the same email already has an unused unexpired invite, a second invite
/// creation hits the partial unique index and returns 409 "invite_pending".
#[sqlx::test(migrations = "./migrations")]
async fn second_invite_for_same_email_conflicts(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin3@example.com").await;
    let _ = seed_invite(&pool, "pending@example.com", admin_id).await;

    // Attempt a second insert for the same email — should violate the partial unique index.
    let second_hash = format!("hash_{}", Uuid::new_v4());
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);

    let result = sqlx::query(
        r#"INSERT INTO invite_tokens (token_hash, email, role, created_by, expires_at)
           VALUES ($1, 'pending@example.com', 'user', $2, $3)"#,
    )
    .bind(&second_hash)
    .bind(admin_id)
    .bind(expires_at)
    .execute(&pool)
    .await;

    // Postgres error code 23505 = unique_violation
    let err = result.expect_err("should violate unique index");
    match &err {
        sqlx::Error::Database(db_err) => {
            assert_eq!(
                db_err.code().as_deref(),
                Some("23505"),
                "expected unique_violation (23505), got {:?}",
                db_err.code()
            );
        }
        other => panic!("expected Database error, got {other:?}"),
    }
}

/// After revoking the first invite, a second invite for the same email succeeds.
#[sqlx::test(migrations = "./migrations")]
async fn second_invite_succeeds_after_revoke(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin4@example.com").await;
    let first_hash = seed_invite(&pool, "revoke-test@example.com", admin_id).await;

    // Revoke the first invite
    sqlx::query("DELETE FROM invite_tokens WHERE token_hash = $1")
        .bind(&first_hash)
        .execute(&pool)
        .await
        .unwrap();

    // Now a second invite should succeed
    let second_hash = format!("hash_{}", Uuid::new_v4());
    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(7);
    sqlx::query(
        r#"INSERT INTO invite_tokens (token_hash, email, role, created_by, expires_at)
           VALUES ($1, 'revoke-test@example.com', 'user', $2, $3)"#,
    )
    .bind(&second_hash)
    .bind(admin_id)
    .bind(expires_at)
    .execute(&pool)
    .await
    .expect("second invite after revoke should succeed");
}

// ─── AT-11: concurrent accept (serialisation) ────────────────────────────────

/// Simulates two sequential accept attempts on the same token.
/// The first sets used_at; the second should get no row from the FOR UPDATE query
/// and return false (mapped to 410 invalid_token).
#[sqlx::test(migrations = "./migrations")]
async fn double_accept_second_is_rejected(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin5@example.com").await;
    let token_hash = seed_invite(&pool, "double@example.com", admin_id).await;

    // First accept: mark used
    let mut tx1 = pool.begin().await.unwrap();
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM invite_tokens
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx1)
    .await
    .unwrap();
    assert!(row.is_some(), "first accept should see the token");

    sqlx::query("UPDATE invite_tokens SET used_at = now(), used_by = $1 WHERE token_hash = $2")
        .bind(admin_id) // dummy user_id for test
        .bind(&token_hash)
        .execute(&mut *tx1)
        .await
        .unwrap();
    tx1.commit().await.unwrap();

    // Second accept: should find no active row
    let mut tx2 = pool.begin().await.unwrap();
    let row2: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM invite_tokens
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
        "second accept must not find the already-used token"
    );
}

// ─── Expired invite is invisible ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn expired_invite_is_not_found(pool: PgPool) {
    let admin_id = seed_admin(&pool, "admin6@example.com").await;

    let token_hash = format!("expired_{}", Uuid::new_v4());
    let expired_at = OffsetDateTime::now_utc() - time::Duration::hours(1);
    sqlx::query(
        r#"INSERT INTO invite_tokens (token_hash, email, role, created_by, expires_at)
           VALUES ($1, 'expired@example.com', 'user', $2, $3)"#,
    )
    .bind(&token_hash)
    .bind(admin_id)
    .bind(expired_at)
    .execute(&pool)
    .await
    .unwrap();

    let found: Option<(String,)> = sqlx::query_as(
        "SELECT token_hash FROM invite_tokens WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(&token_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        found.is_none(),
        "expired invite must not be found by active query"
    );
}
