use crate::error::AppError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatSession {
    pub id: Uuid,
    pub persona_id: Uuid,
    pub era_id: Option<Uuid>,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub model_id: String,
    pub temperature: f32,
    pub top_p: f32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub content: String,
    pub retrieved_chunk_ids: Vec<Uuid>,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub created_at: OffsetDateTime,
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

pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    persona_id: Uuid,
    era_id: Option<Uuid>,
    model_id: &str,
    temperature: f32,
    top_p: f32,
) -> Result<ChatSession, AppError> {
    sqlx::query_as::<_, ChatSession>(
        "INSERT INTO chat_sessions (persona_id, era_id, user_id, model_id, temperature, top_p)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, persona_id, era_id, user_id, title, model_id, temperature, top_p,
                   created_at, updated_at",
    )
    .bind(persona_id)
    .bind(era_id)
    .bind(user_id)
    .bind(model_id)
    .bind(temperature)
    .bind(top_p)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn list_sessions(
    pool: &PgPool,
    user_id: Uuid,
    persona_id: Uuid,
    cursor: Option<&str>,
    limit: i64,
) -> Result<(Vec<ChatSession>, Option<String>), AppError> {
    let rows = if let Some(c) = cursor {
        let (ts, id) = decode_cursor(c)?;
        sqlx::query_as::<_, ChatSession>(
            "SELECT id, persona_id, era_id, user_id, title, model_id, temperature, top_p,
                    created_at, updated_at
             FROM chat_sessions
             WHERE user_id = $1 AND persona_id = $2
               AND (created_at, id) < ($3, $4)
             ORDER BY created_at DESC, id DESC
             LIMIT $5",
        )
        .bind(user_id)
        .bind(persona_id)
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_as::<_, ChatSession>(
            "SELECT id, persona_id, era_id, user_id, title, model_id, temperature, top_p,
                    created_at, updated_at
             FROM chat_sessions
             WHERE user_id = $1 AND persona_id = $2
             ORDER BY created_at DESC, id DESC
             LIMIT $3",
        )
        .bind(user_id)
        .bind(persona_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    };

    let next_cursor = if rows.len() as i64 > limit {
        let last = &rows[limit as usize - 1];
        Some(encode_cursor(last.created_at, last.id))
    } else {
        None
    };
    let items = rows.into_iter().take(limit as usize).collect();
    Ok((items, next_cursor))
}

pub async fn get_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Option<ChatSession>, AppError> {
    sqlx::query_as::<_, ChatSession>(
        "SELECT id, persona_id, era_id, user_id, title, model_id, temperature, top_p,
                created_at, updated_at
         FROM chat_sessions
         WHERE id = $1 AND user_id = $2",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn delete_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, AppError> {
    let rows = sqlx::query("DELETE FROM chat_sessions WHERE id = $1 AND user_id = $2")
        .bind(session_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(rows.rows_affected() > 0)
}

pub async fn create_message(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    role: &str,
    content: &str,
) -> Result<Message, AppError> {
    sqlx::query_as::<_, Message>(
        "INSERT INTO messages (session_id, user_id, role, content)
         VALUES ($1, $2, $3, $4)
         RETURNING id, session_id, user_id, role, content,
                   retrieved_chunk_ids, tokens_in, tokens_out, created_at",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(role)
    .bind(content)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn list_messages(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    cursor: Option<&str>,
    limit: i64,
) -> Result<(Vec<Message>, Option<String>), AppError> {
    let rows = if let Some(c) = cursor {
        let (ts, id) = decode_cursor(c)?;
        sqlx::query_as::<_, Message>(
            "SELECT id, session_id, user_id, role, content,
                    retrieved_chunk_ids, tokens_in, tokens_out, created_at
             FROM messages
             WHERE session_id = $1 AND user_id = $2
               AND (created_at, id) < ($3, $4)
             ORDER BY created_at DESC, id DESC
             LIMIT $5",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_as::<_, Message>(
            "SELECT id, session_id, user_id, role, content,
                    retrieved_chunk_ids, tokens_in, tokens_out, created_at
             FROM messages
             WHERE session_id = $1 AND user_id = $2
             ORDER BY created_at DESC, id DESC
             LIMIT $3",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    };

    let next_cursor = if rows.len() as i64 > limit {
        let last = &rows[limit as usize - 1];
        Some(encode_cursor(last.created_at, last.id))
    } else {
        None
    };
    let items = rows.into_iter().take(limit as usize).collect();
    Ok((items, next_cursor))
}

pub async fn update_message_metadata(
    pool: &PgPool,
    msg_id: Uuid,
    retrieved_chunk_ids: &[Uuid],
    tokens_in: i32,
    tokens_out: i32,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE messages
         SET retrieved_chunk_ids = $1, tokens_in = $2, tokens_out = $3
         WHERE id = $4",
    )
    .bind(retrieved_chunk_ids)
    .bind(tokens_in)
    .bind(tokens_out)
    .bind(msg_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn update_session_title(
    pool: &PgPool,
    session_id: Uuid,
    title: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE chat_sessions SET title = $1, updated_at = now() WHERE id = $2 AND title IS NULL",
    )
    .bind(title)
    .bind(session_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}
