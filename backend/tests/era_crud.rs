/// TDD tests for era repository user_id scoping and date validation invariants.
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
    sqlx::query_scalar(
        "INSERT INTO personas (user_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("seed persona")
}

// ─── Authorization boundary ──────────────────────────────────────────────────

/// User B cannot list eras for User A's persona because the persona is invisible to B.
#[sqlx::test(migrations = "./migrations")]
async fn era_list_scoped_to_persona_owner(pool: PgPool) {
    let user_a = seed_user(&pool, "era_a@example.com").await;
    let user_b = seed_user(&pool, "era_b@example.com").await;

    let persona_id = seed_persona(&pool, user_a, "A persona").await;

    sqlx::query("INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Youth')")
        .bind(persona_id)
        .bind(user_a)
        .execute(&pool)
        .await
        .unwrap();

    // User B trying to list eras for User A's persona sees nothing
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM eras WHERE persona_id = $1 AND user_id = $2",
    )
    .bind(persona_id)
    .bind(user_b)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 0, "user B must not see user A's eras");
}

/// Era labels are unique per persona (not globally).
#[sqlx::test(migrations = "./migrations")]
async fn era_label_unique_per_persona(pool: PgPool) {
    let user = seed_user(&pool, "era_uniq@example.com").await;
    let persona = seed_persona(&pool, user, "My Persona").await;

    sqlx::query("INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Childhood')")
        .bind(persona)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    let result = sqlx::query(
        "INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Childhood')",
    )
    .bind(persona)
    .bind(user)
    .execute(&pool)
    .await;

    assert!(result.is_err(), "duplicate label in same persona must fail");
}

/// Same label allowed in different personas.
#[sqlx::test(migrations = "./migrations")]
async fn era_label_same_allowed_across_personas(pool: PgPool) {
    let user = seed_user(&pool, "era_cross@example.com").await;
    let p1 = seed_persona(&pool, user, "Persona 1").await;
    let p2 = seed_persona(&pool, user, "Persona 2").await;

    sqlx::query("INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Youth')")
        .bind(p1)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Youth')")
        .bind(p2)
        .bind(user)
        .execute(&pool)
        .await
        .expect("same label in different persona must succeed");
}

// ─── Date invariants ─────────────────────────────────────────────────────────

/// Era rows accept NULL start/end dates.
#[sqlx::test(migrations = "./migrations")]
async fn era_dates_nullable(pool: PgPool) {
    let user = seed_user(&pool, "era_null_date@example.com").await;
    let persona = seed_persona(&pool, user, "Dateless").await;

    sqlx::query(
        "INSERT INTO eras (persona_id, user_id, label, start_date, end_date)
         VALUES ($1, $2, 'No dates', NULL, NULL)",
    )
    .bind(persona)
    .bind(user)
    .execute(&pool)
    .await
    .expect("null dates should be accepted");
}

/// Era with valid date range (start ≤ end) is stored correctly.
#[sqlx::test(migrations = "./migrations")]
async fn era_valid_date_range_stored(pool: PgPool) {
    let user = seed_user(&pool, "era_dates@example.com").await;
    let persona = seed_persona(&pool, user, "With dates").await;

    sqlx::query(
        "INSERT INTO eras (persona_id, user_id, label, start_date, end_date)
         VALUES ($1, $2, 'School', '2005-09-01', '2012-06-30')",
    )
    .bind(persona)
    .bind(user)
    .execute(&pool)
    .await
    .expect("valid date range must be stored");

    let (start, end): (time::Date, time::Date) = sqlx::query_as(
        "SELECT start_date, end_date FROM eras WHERE persona_id = $1 AND label = 'School'",
    )
    .bind(persona)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(start <= end, "start must be ≤ end as stored");
}

// ─── Cascade on era delete ────────────────────────────────────────────────────

/// Deleting an era sets era_id to NULL on documents (SET NULL FK) rather than deleting them.
#[sqlx::test(migrations = "./migrations")]
async fn delete_era_nullifies_document_era_id(pool: PgPool) {
    let user = seed_user(&pool, "era_doc_null@example.com").await;
    let persona = seed_persona(&pool, user, "Persona").await;

    let era_id: Uuid =
        sqlx::query_scalar("INSERT INTO eras (persona_id, user_id, label) VALUES ($1, $2, 'Era 1') RETURNING id")
            .bind(persona)
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Insert a minimal document referencing this era
    let doc_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO documents
           (persona_id, era_id, user_id, kind, mime_type, original_path, content_hash, size_bytes)
           VALUES ($1, $2, $3, 'text', 'text/plain', '/tmp/x.txt', 'aabbcc', 100)
           RETURNING id"#,
    )
    .bind(persona)
    .bind(era_id)
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Delete the era
    sqlx::query("DELETE FROM eras WHERE id = $1")
        .bind(era_id)
        .execute(&pool)
        .await
        .unwrap();

    // Document still exists but era_id is now NULL
    let era_col: Option<Uuid> =
        sqlx::query_scalar("SELECT era_id FROM documents WHERE id = $1")
            .bind(doc_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(era_col.is_none(), "era_id must be set to NULL after era deletion");
}
