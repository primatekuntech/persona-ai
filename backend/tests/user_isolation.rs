/// Integration tests for cross-user data isolation.
/// Every domain table row carries user_id; these tests verify that queries
/// that scope by user_id cannot access another user's data.
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role, display_name)
           VALUES ($1, 'hashed', 'user', 'Test')
           RETURNING id"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

// ─── Personas isolation ──────────────────────────────────────────────────────

/// User A's persona is not visible when querying by user B's id.
#[sqlx::test(migrations = "./migrations")]
async fn persona_scoped_to_owner(pool: PgPool) {
    let user_a = seed_user(&pool, "a@example.com").await;
    let user_b = seed_user(&pool, "b@example.com").await;

    // Insert a persona for user_a
    let persona_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO personas (user_id, name, description)
           VALUES ($1, 'Alice Persona', 'desc')
           RETURNING id"#,
    )
    .bind(user_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Query from user_b's perspective — should return nothing
    let visible: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM personas WHERE id = $1 AND user_id = $2")
            .bind(persona_id)
            .bind(user_b)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(
        visible.is_none(),
        "user_b must not see user_a's persona (got {visible:?})"
    );
}

// ─── Documents isolation ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn document_scoped_to_owner(pool: PgPool) {
    let user_a = seed_user(&pool, "doc_a@example.com").await;
    let user_b = seed_user(&pool, "doc_b@example.com").await;

    // Need a persona first (FK)
    let persona_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO personas (user_id, name, description)
           VALUES ($1, 'P', 'd')
           RETURNING id"#,
    )
    .bind(user_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    let doc_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO documents (user_id, persona_id, title, source_type, status)
           VALUES ($1, $2, 'Secret Doc', 'text', 'ready')
           RETURNING id"#,
    )
    .bind(user_a)
    .bind(persona_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let visible: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE id = $1 AND user_id = $2")
            .bind(doc_id)
            .bind(user_b)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(visible.is_none(), "user_b must not see user_a's document");
}

// ─── Chat sessions isolation ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn chat_session_scoped_to_owner(pool: PgPool) {
    let user_a = seed_user(&pool, "chat_a@example.com").await;
    let user_b = seed_user(&pool, "chat_b@example.com").await;

    let persona_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO personas (user_id, name, description)
           VALUES ($1, 'P', 'd')
           RETURNING id"#,
    )
    .bind(user_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    let chat_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO chat_sessions (user_id, persona_id, title)
           VALUES ($1, $2, 'Private chat')
           RETURNING id"#,
    )
    .bind(user_a)
    .bind(persona_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let visible: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM chat_sessions WHERE id = $1 AND user_id = $2")
            .bind(chat_id)
            .bind(user_b)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(
        visible.is_none(),
        "user_b must not see user_a's chat session"
    );
}

// ─── Find-by-id returns own row ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn find_user_by_id_returns_correct_row(pool: PgPool) {
    let user_a = seed_user(&pool, "findme_a@example.com").await;
    let user_b = seed_user(&pool, "findme_b@example.com").await;

    let found: Option<(String,)> = sqlx::query_as("SELECT email FROM users WHERE id = $1")
        .bind(user_a)
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert_eq!(found.map(|r| r.0).as_deref(), Some("findme_a@example.com"));

    // user_b lookup returns user_b's row, not user_a's
    let found_b: Option<(String,)> = sqlx::query_as("SELECT email FROM users WHERE id = $1")
        .bind(user_b)
        .fetch_optional(&pool)
        .await
        .unwrap();

    assert_eq!(
        found_b.map(|r| r.0).as_deref(),
        Some("findme_b@example.com")
    );
}

// ─── Password reset tokens scoped to user ────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn password_reset_scoped_to_user(pool: PgPool) {
    let user_a = seed_user(&pool, "reset_a@example.com").await;
    let user_b = seed_user(&pool, "reset_b@example.com").await;

    let expires_at = time::OffsetDateTime::now_utc() + time::Duration::minutes(30);
    let token_hash_a = "aaaa0000000000000000000000000000000000000000000000000000000000aa";

    sqlx::query(
        "INSERT INTO password_resets (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(token_hash_a)
    .bind(user_a)
    .bind(expires_at)
    .execute(&pool)
    .await
    .unwrap();

    // user_b trying to use user_a's reset token finds nothing when checking user_id
    let found: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM password_resets
           WHERE token_hash = $1 AND user_id = $2 AND used_at IS NULL AND expires_at > now()"#,
    )
    .bind(token_hash_a)
    .bind(user_b)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        found.is_none(),
        "user_b must not be able to use user_a's reset token"
    );

    // user_a using their own token finds it
    let found_own: Option<(String,)> = sqlx::query_as(
        r#"SELECT token_hash FROM password_resets
           WHERE token_hash = $1 AND user_id = $2 AND used_at IS NULL AND expires_at > now()"#,
    )
    .bind(token_hash_a)
    .bind(user_a)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        found_own.is_some(),
        "user_a should find their own reset token"
    );
}
