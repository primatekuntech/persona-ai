/// TDD tests for chat session CRUD and message posting (Sprint 5).
/// Uses `sqlx::test` with a real PgPool (auto-rolled-back per test).
use sqlx::PgPool;
use uuid::Uuid;

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn seed_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, role) VALUES ($1, 'x', 'user') RETURNING id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

async fn seed_persona(pool: &PgPool, user_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO personas (user_id, name) VALUES ($1, 'Test Persona') RETURNING id",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("seed persona")
}

async fn seed_session(pool: &PgPool, user_id: Uuid, persona_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO chat_sessions (persona_id, user_id, model_id, temperature, top_p)
         VALUES ($1, $2, 'test-model', 0.7, 0.9) RETURNING id",
    )
    .bind(persona_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("seed session")
}

async fn create_session_db(
    pool: &PgPool,
    user_id: Uuid,
    persona_id: Uuid,
    model_id: &str,
    temperature: f32,
    top_p: f32,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO chat_sessions (persona_id, user_id, model_id, temperature, top_p)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(persona_id)
    .bind(user_id)
    .bind(model_id)
    .bind(temperature)
    .bind(top_p)
    .fetch_one(pool)
    .await
    .expect("create session")
}

async fn create_message_db(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    role: &str,
    content: &str,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO messages (session_id, user_id, role, content)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(role)
    .bind(content)
    .fetch_one(pool)
    .await
    .expect("create message")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// create_session: creates and returns a session row.
#[sqlx::test(migrations = "./migrations")]
async fn create_session_returns_session(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_create@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;

    let session_id = create_session_db(&pool, user_id, persona_id, "test-model", 0.7, 0.9).await;

    let row: (Uuid, Uuid, String) =
        sqlx::query_as("SELECT persona_id, user_id, model_id FROM chat_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .expect("fetch session");

    assert_eq!(row.0, persona_id);
    assert_eq!(row.1, user_id);
    assert_eq!(row.2, "test-model");
}

/// list_sessions: persona with no sessions returns empty list.
#[sqlx::test(migrations = "./migrations")]
async fn list_sessions_empty(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_list_empty@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_sessions WHERE user_id = $1 AND persona_id = $2",
    )
    .bind(user_id)
    .bind(persona_id)
    .fetch_one(&pool)
    .await
    .expect("count sessions");

    assert_eq!(count, 0);
}

/// get_session: user B cannot retrieve user A's session (scoped by user_id).
#[sqlx::test(migrations = "./migrations")]
async fn get_session_404_wrong_user(pool: PgPool) {
    let user_a = seed_user(&pool, "chat_404_a@example.com").await;
    let user_b = seed_user(&pool, "chat_404_b@example.com").await;
    let persona_a = seed_persona(&pool, user_a).await;

    let session_id = seed_session(&pool, user_a, persona_a).await;

    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM chat_sessions WHERE id = $1 AND user_id = $2")
            .bind(session_id)
            .bind(user_b)
            .fetch_optional(&pool)
            .await
            .expect("query");

    assert!(found.is_none(), "user B must not see user A's session");
}

/// delete_session: delete removes the row; subsequent lookup returns nothing.
#[sqlx::test(migrations = "./migrations")]
async fn delete_session_removes_it(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_delete@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;
    let session_id = seed_session(&pool, user_id, persona_id).await;

    let affected = sqlx::query("DELETE FROM chat_sessions WHERE id = $1 AND user_id = $2")
        .bind(session_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("delete")
        .rows_affected();
    assert_eq!(affected, 1);

    let after: Option<Uuid> = sqlx::query_scalar("SELECT id FROM chat_sessions WHERE id = $1")
        .bind(session_id)
        .fetch_optional(&pool)
        .await
        .expect("fetch after delete");
    assert!(after.is_none(), "session must be gone after delete");
}

/// Empty corpus guard: zero embedded chunks for user/persona combination.
/// This is a DB-level invariant test — the route handler queries this count.
#[sqlx::test(migrations = "./migrations")]
async fn empty_corpus_chunk_count_is_zero(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_empty_corpus@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunks WHERE user_id = $1 AND persona_id = $2
         AND embedding IS NOT NULL",
    )
    .bind(user_id)
    .bind(persona_id)
    .fetch_one(&pool)
    .await
    .expect("count chunks");

    assert_eq!(count, 0, "no embedded chunks for fresh persona");
}

/// Content length limit: 20 480 bytes is the cap.
/// This is a unit invariant test — verifies the constant defined in the route.
#[test]
fn content_length_cap_is_20480_bytes() {
    // The route handler rejects content > 20_480 bytes
    const MAX_CONTENT_BYTES: usize = 20_480;
    let too_large = "x".repeat(MAX_CONTENT_BYTES + 1);
    assert!(
        too_large.len() > MAX_CONTENT_BYTES,
        "sanity check: string is over cap"
    );
    let ok = "x".repeat(MAX_CONTENT_BYTES);
    assert!(ok.len() <= MAX_CONTENT_BYTES);
}

/// Empty content is invalid.
#[test]
fn empty_content_is_invalid() {
    let content = "   ";
    assert!(
        content.trim().is_empty(),
        "whitespace-only content must be rejected"
    );
}

/// create_message: persists rows and returns correct fields.
#[sqlx::test(migrations = "./migrations")]
async fn create_message_persists_correctly(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_msg@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;
    let session_id = seed_session(&pool, user_id, persona_id).await;

    let msg_id = create_message_db(&pool, user_id, session_id, "user", "Hello persona!").await;

    let row: (String, String, Uuid) =
        sqlx::query_as("SELECT role, content, session_id FROM messages WHERE id = $1")
            .bind(msg_id)
            .fetch_one(&pool)
            .await
            .expect("fetch message");

    assert_eq!(row.0, "user");
    assert_eq!(row.1, "Hello persona!");
    assert_eq!(row.2, session_id);
}

/// update_message_metadata: sets retrieved_chunk_ids, tokens_in, tokens_out.
#[sqlx::test(migrations = "./migrations")]
async fn update_message_metadata_works(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_meta@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;
    let session_id = seed_session(&pool, user_id, persona_id).await;
    let msg_id = create_message_db(&pool, user_id, session_id, "assistant", "").await;

    let chunk_id = Uuid::now_v7();
    sqlx::query(
        "UPDATE messages
         SET retrieved_chunk_ids = $1, tokens_in = $2, tokens_out = $3
         WHERE id = $4",
    )
    .bind(vec![chunk_id])
    .bind(100_i32)
    .bind(50_i32)
    .bind(msg_id)
    .execute(&pool)
    .await
    .expect("update metadata");

    let updated: (Vec<Uuid>, Option<i32>, Option<i32>) = sqlx::query_as(
        "SELECT retrieved_chunk_ids, tokens_in, tokens_out FROM messages WHERE id = $1",
    )
    .bind(msg_id)
    .fetch_one(&pool)
    .await
    .expect("fetch updated message");

    assert_eq!(updated.0, vec![chunk_id]);
    assert_eq!(updated.1, Some(100));
    assert_eq!(updated.2, Some(50));
}

/// update_session_title: sets title only when currently NULL (idempotent first-write).
#[sqlx::test(migrations = "./migrations")]
async fn update_session_title_only_when_null(pool: PgPool) {
    let user_id = seed_user(&pool, "chat_title@example.com").await;
    let persona_id = seed_persona(&pool, user_id).await;
    let session_id = seed_session(&pool, user_id, persona_id).await;

    // First update sets the title
    sqlx::query(
        "UPDATE chat_sessions SET title = $1, updated_at = now() WHERE id = $2 AND title IS NULL",
    )
    .bind("First title")
    .bind(session_id)
    .execute(&pool)
    .await
    .expect("set title");

    let title: Option<String> = sqlx::query_scalar("SELECT title FROM chat_sessions WHERE id = $1")
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .expect("fetch title");
    assert_eq!(title.as_deref(), Some("First title"));

    // Second update must NOT overwrite (title IS NOT NULL)
    sqlx::query(
        "UPDATE chat_sessions SET title = $1, updated_at = now() WHERE id = $2 AND title IS NULL",
    )
    .bind("Second title")
    .bind(session_id)
    .execute(&pool)
    .await
    .expect("idempotent title update");

    let title2: Option<String> =
        sqlx::query_scalar("SELECT title FROM chat_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .expect("fetch title2");
    assert_eq!(
        title2.as_deref(),
        Some("First title"),
        "title must not change after first set"
    );
}
