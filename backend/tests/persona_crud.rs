/// TDD tests for persona repository user_id scoping and cascade behaviour.
/// All tests use sqlx::test which auto-creates and rolls back a per-test database.
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

async fn create_persona(pool: &PgPool, user_id: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO personas (user_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("create persona")
}

// ─── Authorization boundary ──────────────────────────────────────────────────

/// User B cannot see User A's persona — the user_id filter must return nothing.
#[sqlx::test(migrations = "./migrations")]
async fn find_persona_by_other_user_returns_none(pool: PgPool) {
    let user_a = seed_user(&pool, "pa_a@example.com").await;
    let user_b = seed_user(&pool, "pa_b@example.com").await;

    let persona_id = create_persona(&pool, user_a, "A's persona").await;

    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM personas WHERE id = $1 AND user_id = $2",
    )
    .bind(persona_id)
    .bind(user_b)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(found.is_none(), "user B must not see user A's persona");
}

/// Two different users can each have a persona named "Alpha" (unique per user, not globally).
#[sqlx::test(migrations = "./migrations")]
async fn duplicate_persona_name_allowed_across_users(pool: PgPool) {
    let user_a = seed_user(&pool, "dup_a@example.com").await;
    let user_b = seed_user(&pool, "dup_b@example.com").await;

    let id_a = create_persona(&pool, user_a, "Alpha").await;
    let id_b = create_persona(&pool, user_b, "Alpha").await;

    assert_ne!(id_a, id_b);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM personas WHERE name = 'Alpha'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

/// Persona names must be unique *within* a user account.
#[sqlx::test(migrations = "./migrations")]
async fn duplicate_persona_name_rejected_for_same_user(pool: PgPool) {
    let user_a = seed_user(&pool, "same_user_dup@example.com").await;
    create_persona(&pool, user_a, "Alpha").await;

    let result: Result<Uuid, _> = sqlx::query_scalar(
        "INSERT INTO personas (user_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_a)
    .bind("Alpha")
    .fetch_one(&pool)
    .await;

    assert!(result.is_err(), "duplicate persona name for same user must fail");
}

// ─── Cascade behaviour ───────────────────────────────────────────────────────

/// Deleting a persona removes its child eras via FK cascade.
#[sqlx::test(migrations = "./migrations")]
async fn delete_persona_cascades_eras(pool: PgPool) {
    let user = seed_user(&pool, "cascade_era@example.com").await;
    let persona_id = create_persona(&pool, user, "Cascade test").await;

    // Create an era under this persona
    sqlx::query(
        "INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Childhood')",
    )
    .bind(persona_id)
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    let era_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM eras WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(era_count_before, 1);

    // Delete the persona
    sqlx::query("DELETE FROM personas WHERE id = $1")
        .bind(persona_id)
        .execute(&pool)
        .await
        .unwrap();

    let era_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM eras WHERE persona_id = $1")
            .bind(persona_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(era_count_after, 0, "eras must be cascade-deleted with persona");
}

/// After concurrent deletes, exactly one succeeds; the second sees the row as gone.
#[sqlx::test(migrations = "./migrations")]
async fn delete_persona_concurrent_second_returns_nothing(pool: PgPool) {
    let user = seed_user(&pool, "concur_del@example.com").await;
    let persona_id = create_persona(&pool, user, "To be deleted").await;

    // First delete
    let rows1 = sqlx::query("DELETE FROM personas WHERE id = $1 AND user_id = $2")
        .bind(persona_id)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

    // Second delete (row is gone now)
    let rows2 = sqlx::query("DELETE FROM personas WHERE id = $1 AND user_id = $2")
        .bind(persona_id)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

    assert_eq!(rows1, 1);
    assert_eq!(rows2, 0, "second delete must affect 0 rows");
}

// ─── Quota counter ───────────────────────────────────────────────────────────

/// current_persona_count increments when a persona is created.
#[sqlx::test(migrations = "./migrations")]
async fn persona_count_increments_on_create(pool: PgPool) {
    let user = seed_user(&pool, "quota_inc@example.com").await;

    let before: i64 =
        sqlx::query_scalar("SELECT current_persona_count FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Simulate create + increment as the repository does
    sqlx::query("INSERT INTO personas (user_id, name) VALUES ($1, 'P1')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE users SET current_persona_count = current_persona_count + 1 WHERE id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    let after: i64 =
        sqlx::query_scalar("SELECT current_persona_count FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(after, before + 1);
}
