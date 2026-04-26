/// Idempotency key service. Implements the 24-hour deduplication window per
/// docs/06-api-conventions.md#idempotency.
use crate::error::AppError;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

pub struct IdempotencyHit {
    pub status: i32,
    pub body: serde_json::Value,
}

fn hash_body(json_bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(json_bytes))
}

pub fn body_hash<T: serde::Serialize>(body: &T) -> String {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    hash_body(&bytes)
}

/// Look up an idempotency key. Returns:
/// - `Ok(Some(hit))` if the key exists with the same body hash → replay the cached response.
/// - `Ok(None)` if the key is new → proceed normally.
/// - `Err(Conflict)` if the key exists but with a different body hash.
pub async fn check(
    pool: &PgPool,
    key: &str,
    user_id: Uuid,
    route: &str,
    body_hash: &str,
) -> Result<Option<IdempotencyHit>, AppError> {
    let row: Option<(i32, serde_json::Value, String)> = sqlx::query_as(
        r#"SELECT response_status, response_body, request_hash
           FROM idempotency_keys
           WHERE user_id = $1 AND route = $2 AND key = $3
             AND created_at > now() - interval '24 hours'"#,
    )
    .bind(user_id)
    .bind(route)
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    match row {
        Some((status, body, stored_hash)) => {
            if stored_hash != body_hash {
                return Err(AppError::Conflict {
                    code: "idempotency_key_conflict",
                });
            }
            Ok(Some(IdempotencyHit { status, body }))
        }
        None => Ok(None),
    }
}

/// Store a completed response so future replays return the same result.
pub async fn store(
    pool: &PgPool,
    key: &str,
    user_id: Uuid,
    route: &str,
    body_hash: &str,
    status: i32,
    body: serde_json::Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO idempotency_keys (key, user_id, route, request_hash, response_status, response_body)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (user_id, route, key) DO NOTHING"#,
    )
    .bind(key)
    .bind(user_id)
    .bind(route)
    .bind(body_hash)
    .bind(status)
    .bind(body)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}
