/// Document repository. All functions take `user_id: Uuid` and filter by it.
use crate::error::AppError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Document {
    pub id: Uuid,
    pub persona_id: Uuid,
    pub era_id: Option<Uuid>,
    pub user_id: Uuid,
    pub kind: String,
    pub mime_type: String,
    pub original_path: String,
    pub transcript_path: Option<String>,
    pub content_hash: String,
    pub size_bytes: i64,
    pub title: Option<String>,
    pub source: Option<String>,
    pub word_count: Option<i32>,
    pub duration_sec: Option<i32>,
    pub progress_pct: Option<i16>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: OffsetDateTime,
    pub ingested_at: Option<OffsetDateTime>,
}

const DOCUMENT_COLUMNS: &str =
    "id, persona_id, era_id, user_id, kind, mime_type, original_path, transcript_path, \
     content_hash, size_bytes, title, source, word_count, duration_sec, progress_pct, \
     status, error, created_at, ingested_at";

pub fn encode_cursor(ts: OffsetDateTime, id: Uuid) -> String {
    let ts_str = ts
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    URL_SAFE_NO_PAD.encode(format!("{ts_str}|{id}"))
}

pub fn decode_cursor(cursor: &str) -> Result<(OffsetDateTime, Uuid), AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| AppError::Validation("Invalid cursor.".into()))?;
    let s = String::from_utf8(bytes).map_err(|_| AppError::Validation("Invalid cursor.".into()))?;
    let (ts_str, id_str) = s
        .split_once('|')
        .ok_or_else(|| AppError::Validation("Invalid cursor.".into()))?;
    let ts = OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
        .map_err(|_| AppError::Validation("Invalid cursor.".into()))?;
    let id = Uuid::parse_str(id_str).map_err(|_| AppError::Validation("Invalid cursor.".into()))?;
    Ok((ts, id))
}

/// Insert a new document row with status='pending'.
/// Verifies persona ownership (404 if not found / not owned by user).
#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
    persona_id: Uuid,
    user_id: Uuid,
    era_id: Option<Uuid>,
    kind: &str,
    mime_type: &str,
    original_path: &str,
    content_hash: &str,
    size_bytes: i64,
    title: Option<&str>,
    source: Option<&str>,
) -> Result<Document, AppError> {
    // Verify persona ownership
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM personas WHERE id = $1 AND user_id = $2)")
            .bind(persona_id)
            .bind(user_id)
            .fetch_one(pool)
            .await
            .map_err(AppError::Database)?;

    if !exists {
        return Err(AppError::NotFound);
    }

    let doc: Document = sqlx::query_as(&format!(
        r#"INSERT INTO documents (persona_id, era_id, user_id, kind, mime_type, original_path,
                                  content_hash, size_bytes, title, source, status)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending')
           RETURNING {DOCUMENT_COLUMNS}"#
    ))
    .bind(persona_id)
    .bind(era_id)
    .bind(user_id)
    .bind(kind)
    .bind(mime_type)
    .bind(original_path)
    .bind(content_hash)
    .bind(size_bytes)
    .bind(title)
    .bind(source)
    .fetch_one(pool)
    .await
    .map_err(|e| crate::error::pg_unique_to_conflict(e, "duplicate"))?;

    Ok(doc)
}

pub async fn find_by_id(
    pool: &PgPool,
    id: Uuid,
    persona_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Document>, AppError> {
    sqlx::query_as(&format!(
        "SELECT {DOCUMENT_COLUMNS} FROM documents WHERE id = $1 AND persona_id = $2 AND user_id = $3"
    ))
    .bind(id)
    .bind(persona_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// Find document by content hash within a persona. Returns just the ID (for duplicate check).
pub async fn find_by_content_hash(
    pool: &PgPool,
    persona_id: Uuid,
    content_hash: &str,
) -> Result<Option<Uuid>, AppError> {
    sqlx::query_scalar("SELECT id FROM documents WHERE persona_id = $1 AND content_hash = $2")
        .bind(persona_id)
        .bind(content_hash)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)
}

/// List documents for a persona with cursor pagination and optional filters.
#[allow(clippy::too_many_arguments)]
pub async fn list(
    pool: &PgPool,
    persona_id: Uuid,
    user_id: Uuid,
    limit: i64,
    cursor: Option<(OffsetDateTime, Uuid)>,
    era_id: Option<Uuid>,
    kind: Option<&str>,
    status: Option<&[String]>,
) -> Result<Vec<Document>, AppError> {
    let limit = limit.clamp(1, 200);

    // Build query with optional filters using QueryBuilder
    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(format!(
        "SELECT {DOCUMENT_COLUMNS} FROM documents WHERE persona_id = "
    ));
    builder.push_bind(persona_id);
    builder.push(" AND user_id = ");
    builder.push_bind(user_id);

    if let Some((cursor_ts, cursor_id)) = cursor {
        builder.push(" AND (created_at, id) < (");
        builder.push_bind(cursor_ts);
        builder.push(", ");
        builder.push_bind(cursor_id);
        builder.push(")");
    }

    if let Some(eid) = era_id {
        builder.push(" AND era_id = ");
        builder.push_bind(eid);
    }

    if let Some(k) = kind {
        builder.push(" AND kind = ");
        builder.push_bind(k);
    }

    if let Some(statuses) = status {
        if !statuses.is_empty() {
            builder.push(" AND status = ANY(");
            builder.push_bind(statuses);
            builder.push(")");
        }
    }

    builder.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    builder.push_bind(limit + 1);

    let rows: Vec<Document> = builder
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

    Ok(rows)
}

/// Update document status and optional progress/error fields.
pub async fn update_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    progress_pct: Option<i16>,
    error: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE documents SET status = $1, progress_pct = $2, error = $3 WHERE id = $4")
        .bind(status)
        .bind(progress_pct)
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// Mark document as done with ingested_at and optional word_count / transcript_path.
pub async fn update_ingested(
    pool: &PgPool,
    id: Uuid,
    word_count: Option<i32>,
    transcript_path: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE documents SET status = 'done', ingested_at = now(), progress_pct = NULL, \
         word_count = $1, transcript_path = $2 WHERE id = $3",
    )
    .bind(word_count)
    .bind(transcript_path)
    .bind(id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

/// Delete a document scoped to user. Returns the size_bytes for quota decrement, or None if not found.
pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Option<i64>, AppError> {
    let result: Option<(i64,)> =
        sqlx::query_as("DELETE FROM documents WHERE id = $1 AND user_id = $2 RETURNING size_bytes")
            .bind(id)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(AppError::Database)?;

    Ok(result.map(|(sz,)| sz))
}

/// Delete a document and atomically decrement the user's quota in one transaction.
/// Returns the size_bytes of the deleted document, or None if not found.
pub async fn delete_with_quota(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<i64>, AppError> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    let result: Option<(i64,)> =
        sqlx::query_as("DELETE FROM documents WHERE id = $1 AND user_id = $2 RETURNING size_bytes")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(AppError::Database)?;

    if let Some((size,)) = result {
        sqlx::query(
            "UPDATE users SET \
             current_storage_bytes = GREATEST(0, current_storage_bytes - $1), \
             current_doc_count = GREATEST(0, current_doc_count - 1) \
             WHERE id = $2",
        )
        .bind(size)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;
        Ok(Some(size))
    } else {
        tx.rollback().await.ok();
        Ok(None)
    }
}

/// Atomic quota check + increment. Returns true if quota was available and incremented.
/// The UPDATE touches users.current_storage_bytes and current_doc_count in one statement.
pub async fn atomic_quota_check_and_increment(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    size_bytes: i64,
) -> Result<bool, AppError> {
    let result: Option<i64> = sqlx::query_scalar(
        r#"UPDATE users
           SET current_storage_bytes = current_storage_bytes + $1,
               current_doc_count     = current_doc_count + 1
           WHERE id = $2
             AND current_storage_bytes + $1 <= quota_storage_bytes
             AND current_doc_count + 1       <= quota_doc_count
           RETURNING current_storage_bytes"#,
    )
    .bind(size_bytes)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    Ok(result.is_some())
}

/// Decrement storage and document count after a document is deleted.
pub async fn decrement_quota(
    pool: &PgPool,
    user_id: Uuid,
    size_bytes: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE users SET \
         current_storage_bytes = GREATEST(0, current_storage_bytes - $1), \
         current_doc_count = GREATEST(0, current_doc_count - 1) \
         WHERE id = $2",
    )
    .bind(size_bytes)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}
