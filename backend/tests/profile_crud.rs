/// Integration tests for style_profiles table — user_id scoping invariants.
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, role) VALUES ($1, 'x', 'user') RETURNING id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

async fn seed_persona(pool: &PgPool, user_id: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO personas (user_id, name) VALUES ($1, $2) RETURNING id")
        .bind(user_id)
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("seed persona")
}

async fn seed_era(pool: &PgPool, persona_id: Uuid, user_id: Uuid, label: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(persona_id)
    .bind(user_id)
    .bind(label)
    .fetch_one(pool)
    .await
    .expect("seed era")
}

async fn upsert_profile(
    pool: &PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
    user_id: Uuid,
    corpus_tokens: i32,
    profile: serde_json::Value,
) -> Uuid {
    if era_id.is_none() {
        sqlx::query_scalar(
            "INSERT INTO style_profiles (persona_id, era_id, user_id, corpus_tokens, profile)
             VALUES ($1, NULL, $2, $3, $4)
             ON CONFLICT (persona_id) WHERE era_id IS NULL
             DO UPDATE SET corpus_tokens = EXCLUDED.corpus_tokens, profile = EXCLUDED.profile, computed_at = now()
             RETURNING id",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(corpus_tokens)
        .bind(profile)
        .fetch_one(pool)
        .await
        .expect("upsert profile")
    } else {
        sqlx::query_scalar(
            "INSERT INTO style_profiles (persona_id, era_id, user_id, corpus_tokens, profile)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (persona_id, era_id) WHERE era_id IS NOT NULL
             DO UPDATE SET corpus_tokens = EXCLUDED.corpus_tokens, profile = EXCLUDED.profile, computed_at = now()
             RETURNING id",
        )
        .bind(persona_id)
        .bind(era_id)
        .bind(user_id)
        .bind(corpus_tokens)
        .bind(profile)
        .fetch_one(pool)
        .await
        .expect("upsert era profile")
    }
}

fn sample_profile(status: &str) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "status": status,
        "corpus": {
            "document_count": 5,
            "chunk_count": 50,
            "word_count": 10000,
            "date_range": null
        }
    })
}

/// Upsert creates a new profile row.
#[sqlx::test(migrations = "./migrations")]
async fn upsert_creates_profile(pool: PgPool) {
    let user_id = seed_user(&pool, "profile_create@example.com").await;
    let persona_id = seed_persona(&pool, user_id, "Test Persona").await;

    let id = upsert_profile(&pool, persona_id, None, user_id, 3000, sample_profile("ok")).await;
    assert!(!id.is_nil());

    let row: (i32, serde_json::Value) =
        sqlx::query_as("SELECT corpus_tokens, profile FROM style_profiles WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("fetch profile");

    assert_eq!(row.0, 3000);
    assert_eq!(row.1["status"], "ok");
}

/// Upsert a second time updates the existing row.
#[sqlx::test(migrations = "./migrations")]
async fn upsert_updates_existing(pool: PgPool) {
    let user_id = seed_user(&pool, "profile_update@example.com").await;
    let persona_id = seed_persona(&pool, user_id, "Update Persona").await;

    upsert_profile(
        &pool,
        persona_id,
        None,
        user_id,
        1000,
        sample_profile("insufficient_corpus"),
    )
    .await;

    upsert_profile(&pool, persona_id, None, user_id, 5000, sample_profile("ok")).await;

    // Only one row should exist
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM style_profiles WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);

    let tokens: i32 =
        sqlx::query_scalar("SELECT corpus_tokens FROM style_profiles WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tokens, 5000);
}

/// User B cannot see User A's profile.
#[sqlx::test(migrations = "./migrations")]
async fn profile_scoped_to_persona(pool: PgPool) {
    let user_a = seed_user(&pool, "profile_a@example.com").await;
    let user_b = seed_user(&pool, "profile_b@example.com").await;
    let persona_id = seed_persona(&pool, user_a, "A's Persona").await;

    upsert_profile(&pool, persona_id, None, user_a, 3000, sample_profile("ok")).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM style_profiles WHERE persona_id = $1 AND user_id = $2",
    )
    .bind(persona_id)
    .bind(user_b)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 0, "user B must not see user A's profile");
}

/// Era-specific profile coexists with whole-persona profile.
#[sqlx::test(migrations = "./migrations")]
async fn era_profile_separate_from_persona_wide(pool: PgPool) {
    let user_id = seed_user(&pool, "profile_era@example.com").await;
    let persona_id = seed_persona(&pool, user_id, "Era Persona").await;
    let era_id = seed_era(&pool, persona_id, user_id, "Youth").await;

    upsert_profile(&pool, persona_id, None, user_id, 4000, sample_profile("ok")).await;
    upsert_profile(
        &pool,
        persona_id,
        Some(era_id),
        user_id,
        2000,
        sample_profile("ok"),
    )
    .await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM style_profiles WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2, "persona-wide and era profiles should coexist");

    let wide_era_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT era_id FROM style_profiles WHERE persona_id = $1 AND era_id IS NULL",
    )
    .bind(persona_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(wide_era_id.is_none());

    let era_profile_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT era_id FROM style_profiles WHERE persona_id = $1 AND era_id = $2",
    )
    .bind(persona_id)
    .bind(era_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(era_profile_id, Some(era_id));
}

/// Profile with status=insufficient_corpus stores correctly.
#[sqlx::test(migrations = "./migrations")]
async fn insufficient_corpus_stored_correctly(pool: PgPool) {
    let user_id = seed_user(&pool, "profile_small@example.com").await;
    let persona_id = seed_persona(&pool, user_id, "Small Corpus Persona").await;

    let profile_val = serde_json::json!({
        "version": 1,
        "status": "insufficient_corpus",
        "corpus": {
            "document_count": 1,
            "chunk_count": 3,
            "word_count": 500,
            "date_range": null
        },
        "message": "Upload at least ~2000 words (roughly 4 pages) to generate a reliable style profile."
    });

    upsert_profile(&pool, persona_id, None, user_id, 800, profile_val.clone()).await;

    let stored_profile: serde_json::Value =
        sqlx::query_scalar("SELECT profile FROM style_profiles WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(stored_profile["status"], "insufficient_corpus");
}
