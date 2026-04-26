/// Persona repository. All functions take `user_id: Uuid` and filter by it.
use crate::error::AppError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Persona {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub relation: Option<String>,
    pub description: Option<String>,
    pub avatar_path: Option<String>,
    pub birth_year: Option<i32>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PersonaWithCounts {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub relation: Option<String>,
    pub description: Option<String>,
    pub avatar_path: Option<String>,
    pub birth_year: Option<i32>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub doc_count: i64,
    pub era_count: i64,
}

pub struct DeletedPersonaInfo {
    pub document_ids: Vec<Uuid>,
    pub avatar_path: Option<String>,
}

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

/// Create a persona. Checks quota and atomically increments counter in a transaction.
pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    relation: Option<&str>,
    description: Option<&str>,
    birth_year: Option<i32>,
) -> Result<Persona, AppError> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    let (current, quota): (i64, i64) = sqlx::query_as(
        "SELECT current_persona_count, quota_persona_count FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    if current >= quota {
        tx.rollback().await.ok();
        return Err(AppError::QuotaExceeded);
    }

    let persona: Persona = sqlx::query_as(
        r#"INSERT INTO personas (user_id, name, relation, description, birth_year)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, user_id, name, relation, description, avatar_path, birth_year, created_at, updated_at"#,
    )
    .bind(user_id)
    .bind(name)
    .bind(relation)
    .bind(description)
    .bind(birth_year)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| crate::error::pg_unique_to_conflict(e, "name_taken"))?;

    sqlx::query("UPDATE users SET current_persona_count = current_persona_count + 1 WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;
    Ok(persona)
}

pub async fn find_by_id(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<Persona>, AppError> {
    sqlx::query_as(
        "SELECT id, user_id, name, relation, description, avatar_path, birth_year, created_at, updated_at
         FROM personas WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// List a user's personas with cursor pagination. Returns (items, next_cursor).
pub async fn list(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
    cursor: Option<(OffsetDateTime, Uuid)>,
) -> Result<(Vec<PersonaWithCounts>, Option<String>), AppError> {
    let limit = limit.clamp(1, 200);

    let rows: Vec<PersonaWithCounts> = if let Some((cursor_ts, cursor_id)) = cursor {
        sqlx::query_as(
            r#"SELECT p.id, p.user_id, p.name, p.relation, p.description, p.avatar_path,
                      p.birth_year, p.created_at, p.updated_at,
                      (SELECT COUNT(*) FROM documents d WHERE d.persona_id = p.id) AS doc_count,
                      (SELECT COUNT(*) FROM eras e WHERE e.persona_id = p.id) AS era_count
               FROM personas p
               WHERE p.user_id = $1
                 AND (p.created_at, p.id) < ($2, $3)
               ORDER BY p.created_at DESC, p.id DESC
               LIMIT $4"#,
        )
        .bind(user_id)
        .bind(cursor_ts)
        .bind(cursor_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as(
            r#"SELECT p.id, p.user_id, p.name, p.relation, p.description, p.avatar_path,
                      p.birth_year, p.created_at, p.updated_at,
                      (SELECT COUNT(*) FROM documents d WHERE d.persona_id = p.id) AS doc_count,
                      (SELECT COUNT(*) FROM eras e WHERE e.persona_id = p.id) AS era_count
               FROM personas p
               WHERE p.user_id = $1
               ORDER BY p.created_at DESC, p.id DESC
               LIMIT $2"#,
        )
        .bind(user_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    }
    .map_err(AppError::Database)?;

    let has_more = rows.len() as i64 > limit;
    let items: Vec<PersonaWithCounts> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        items.last().map(|p| encode_cursor(p.created_at, p.id))
    } else {
        None
    };

    Ok((items, next_cursor))
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    name: Option<&str>,
    relation: Option<Option<&str>>,
    description: Option<Option<&str>>,
    birth_year: Option<Option<i32>>,
) -> Result<Option<Persona>, AppError> {
    let mut builder =
        sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE personas SET updated_at = now()");

    if let Some(n) = name {
        builder.push(", name = ");
        builder.push_bind(n);
    }
    if let Some(rel) = relation {
        builder.push(", relation = ");
        builder.push_bind(rel);
    }
    if let Some(desc) = description {
        builder.push(", description = ");
        builder.push_bind(desc);
    }
    if let Some(by) = birth_year {
        builder.push(", birth_year = ");
        builder.push_bind(by);
    }

    builder.push(" WHERE id = ");
    builder.push_bind(id);
    builder.push(" AND user_id = ");
    builder.push_bind(user_id);
    builder.push(
        " RETURNING id, user_id, name, relation, description, avatar_path, birth_year, created_at, updated_at",
    );

    builder
        .build_query_as::<Persona>()
        .fetch_optional(pool)
        .await
        .map_err(|e| crate::error::pg_unique_to_conflict(e, "name_taken"))
}

/// Delete a persona (with FOR UPDATE guard). Returns info for async filesystem cleanup, or None if not found.
pub async fn delete(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<DeletedPersonaInfo>, AppError> {
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT avatar_path FROM personas WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    let (avatar_path,) = match row {
        Some(r) => r,
        None => {
            tx.rollback().await.ok();
            return Ok(None);
        }
    };

    let document_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE persona_id = $1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(AppError::Database)?;

    sqlx::query("DELETE FROM personas WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    sqlx::query(
        "UPDATE users SET current_persona_count = GREATEST(0, current_persona_count - 1) WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    Ok(Some(DeletedPersonaInfo {
        document_ids,
        avatar_path,
    }))
}

pub async fn set_avatar_path(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    avatar_path: &str,
) -> Result<bool, AppError> {
    let rows = sqlx::query(
        "UPDATE personas SET avatar_path = $1, updated_at = now() WHERE id = $2 AND user_id = $3",
    )
    .bind(avatar_path)
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows.rows_affected() > 0)
}

pub async fn clear_avatar_path(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<bool, AppError> {
    let rows = sqlx::query(
        "UPDATE personas SET avatar_path = NULL, updated_at = now() WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(rows.rows_affected() > 0)
}
