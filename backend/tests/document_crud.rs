/// Integration tests for the document repository.
/// Run with: cargo test --test document_crud
/// Requires a live database; uses sqlx::test to spin up a fresh DB per test.
use sqlx::PgPool;
use uuid::Uuid;

// ─── Seed helpers ─────────────────────────────────────────────────────────────

async fn seed_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO users (email, password_hash, role)
           VALUES ($1, '$argon2id$v=19$m=19456,t=2,p=1$fake', 'user')
           RETURNING id"#,
    )
    .bind(format!("test-{}@example.com", Uuid::now_v7()))
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

async fn set_quota(pool: &PgPool, user_id: Uuid, storage: i64, docs: i32) {
    sqlx::query("UPDATE users SET quota_storage_bytes = $1, quota_doc_count = $2 WHERE id = $3")
        .bind(storage)
        .bind(docs)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("set quota");
}

async fn insert_doc(
    pool: &PgPool,
    persona_id: Uuid,
    user_id: Uuid,
    hash: &str,
    size: i64,
    title: Option<&str>,
) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO documents (persona_id, user_id, kind, mime_type, original_path,
                                  content_hash, size_bytes, title, status)
           VALUES ($1, $2, 'text', 'text/plain', 'uploads/test/foo.txt', $3, $4, $5, 'pending')
           RETURNING id"#,
    )
    .bind(persona_id)
    .bind(user_id)
    .bind(hash)
    .bind(size)
    .bind(title)
    .fetch_one(pool)
    .await
    .expect("insert document")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn upload_creates_document(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;
    set_quota(&pool, user_id, 1_000_000_000, 1000).await;

    let doc_id = insert_doc(
        &pool,
        persona_id,
        user_id,
        "deadbeef",
        1024,
        Some("Test Document"),
    )
    .await;

    // Verify the row exists with expected fields
    let row: (String, String, i64, String) = sqlx::query_as(
        "SELECT kind, status, size_bytes, content_hash FROM documents WHERE id = $1",
    )
    .bind(doc_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.0, "text");
    assert_eq!(row.1, "pending");
    assert_eq!(row.2, 1024);
    assert_eq!(row.3, "deadbeef");
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicate_content_hash_returns_existing_id(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;
    set_quota(&pool, user_id, 1_000_000_000, 1000).await;

    let doc_id = insert_doc(&pool, persona_id, user_id, "aabbccdd", 512, None).await;

    // find_by_content_hash should return the existing document's ID
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE persona_id = $1 AND content_hash = $2")
            .bind(persona_id)
            .bind("aabbccdd")
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert_eq!(found, Some(doc_id));

    // Different hash → not found
    let missing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE persona_id = $1 AND content_hash = $2")
            .bind(persona_id)
            .bind("notexist")
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(missing.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn document_scoped_to_owner(pool: PgPool) {
    let user_a = seed_user(&pool).await;
    let user_b = seed_user(&pool).await;
    let persona_a = seed_persona(&pool, user_a).await;
    set_quota(&pool, user_a, 1_000_000_000, 1000).await;

    let doc_id = insert_doc(&pool, persona_a, user_a, "hash_a", 100, None).await;

    // user_b cannot see user_a's document (user_id filter)
    let result: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM documents WHERE id = $1 AND persona_id = $2 AND user_id = $3",
    )
    .bind(doc_id)
    .bind(persona_a)
    .bind(user_b)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(result.is_none(), "user_b must not see user_a's document");

    // user_a can see it
    let result: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM documents WHERE id = $1 AND persona_id = $2 AND user_id = $3",
    )
    .bind(doc_id)
    .bind(persona_a)
    .bind(user_a)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(result.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_returns_size_bytes(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;
    set_quota(&pool, user_id, 1_000_000_000, 1000).await;

    let doc_id = insert_doc(&pool, persona_id, user_id, "delhash", 4096, None).await;

    let size: Option<i64> = sqlx::query_scalar(
        "DELETE FROM documents WHERE id = $1 AND user_id = $2 RETURNING size_bytes",
    )
    .bind(doc_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert_eq!(size, Some(4096));

    // Second delete: row is gone
    let size2: Option<i64> = sqlx::query_scalar(
        "DELETE FROM documents WHERE id = $1 AND user_id = $2 RETURNING size_bytes",
    )
    .bind(doc_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(size2.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn quota_increment_prevents_exceed(pool: PgPool) {
    let user_id = seed_user(&pool).await;

    // 1000 byte quota, 950 used
    sqlx::query(
        "UPDATE users SET quota_storage_bytes = 1000, quota_doc_count = 10, \
         current_storage_bytes = 950 WHERE id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    // Adding 60 bytes would exceed 1000: expect 0 rows updated
    let rows = sqlx::query(
        r#"UPDATE users
           SET current_storage_bytes = current_storage_bytes + 60,
               current_doc_count = current_doc_count + 1
           WHERE id = $1
             AND current_storage_bytes + 60 <= quota_storage_bytes
             AND current_doc_count + 1 <= quota_doc_count"#,
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(rows, 0, "quota should block 60-byte add");

    // Adding 40 bytes fits (950+40=990<=1000)
    let rows = sqlx::query(
        r#"UPDATE users
           SET current_storage_bytes = current_storage_bytes + 40,
               current_doc_count = current_doc_count + 1
           WHERE id = $1
             AND current_storage_bytes + 40 <= quota_storage_bytes
             AND current_doc_count + 1 <= quota_doc_count"#,
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(rows, 1, "quota should allow 40-byte add");
}

#[sqlx::test(migrations = "./migrations")]
async fn list_cursor_pagination(pool: PgPool) {
    let user_id = seed_user(&pool).await;
    let persona_id = seed_persona(&pool, user_id).await;
    set_quota(&pool, user_id, 1_000_000_000, 1000).await;

    // Create 5 documents with distinct timestamps
    let mut ids = Vec::new();
    for i in 0..5u32 {
        let id = insert_doc(
            &pool,
            persona_id,
            user_id,
            &format!("hash{i}"),
            100,
            Some(&format!("Doc {i}")),
        )
        .await;
        ids.push(id);
        // Ensure distinct created_at (millisecond resolution)
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    // Page 1: fetch 2 rows
    let page1: Vec<(Uuid, time::OffsetDateTime)> = sqlx::query_as(
        "SELECT id, created_at FROM documents WHERE persona_id = $1 AND user_id = $2 \
         ORDER BY created_at DESC, id DESC LIMIT 3",
    )
    .bind(persona_id)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    // has_more: returned > 2
    let has_more = page1.len() > 2;
    let visible1: Vec<_> = page1.iter().take(2).cloned().collect();
    assert_eq!(visible1.len(), 2);

    if has_more {
        let cursor_ts = visible1.last().unwrap().1;
        let cursor_id = visible1.last().unwrap().0;

        // Page 2: keyset after the cursor
        let page2: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM documents WHERE persona_id = $1 AND user_id = $2 \
             AND (created_at, id) < ($3, $4) \
             ORDER BY created_at DESC, id DESC LIMIT 3",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(cursor_ts)
        .bind(cursor_id)
        .fetch_all(&pool)
        .await
        .unwrap();

        // No overlap
        let p1_ids: std::collections::HashSet<Uuid> = visible1.iter().map(|(id, _)| *id).collect();
        for (id,) in page2.iter().take(2) {
            assert!(!p1_ids.contains(id), "pages must not overlap");
        }
    }
}
