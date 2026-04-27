/// Repository for `provider_configs` table.
/// Every function filters on `user_id` — no exceptions.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

/// Row representation of `provider_configs`.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProviderConfig {
    pub id: Uuid,
    pub user_id: Uuid,
    pub service: String,
    pub provider: String,
    pub priority: i32,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
}

/// Fields that can be partially updated via PATCH.
#[derive(Debug, Default)]
pub struct UpdateFields {
    pub priority: Option<i32>,
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

// ─── Queries ──────────────────────────────────────────────────────────────────

/// List all provider configs for a user, ordered by service + priority.
pub async fn list_for_user(db: &PgPool, user_id: Uuid) -> Result<Vec<ProviderConfig>, AppError> {
    sqlx::query_as(
        "SELECT id, user_id, service, provider, priority, config, enabled, created_at \
         FROM provider_configs \
         WHERE user_id = $1 \
         ORDER BY service, priority ASC, created_at ASC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
    .map_err(AppError::Database)
}

/// List enabled configs for a specific service, ordered by priority (lowest first).
pub async fn list_enabled_for_service(
    db: &PgPool,
    user_id: Uuid,
    service: &str,
) -> Result<Vec<ProviderConfig>, AppError> {
    sqlx::query_as(
        "SELECT id, user_id, service, provider, priority, config, enabled, created_at \
         FROM provider_configs \
         WHERE user_id = $1 AND service = $2 AND enabled = true \
         ORDER BY priority ASC, created_at ASC",
    )
    .bind(user_id)
    .bind(service)
    .fetch_all(db)
    .await
    .map_err(AppError::Database)
}

/// Create a new provider config. Returns the created row.
pub async fn create(
    db: &PgPool,
    user_id: Uuid,
    service: &str,
    provider: &str,
    priority: i32,
    config: serde_json::Value,
) -> Result<ProviderConfig, AppError> {
    sqlx::query_as(
        "INSERT INTO provider_configs (user_id, service, provider, priority, config) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, user_id, service, provider, priority, config, enabled, created_at",
    )
    .bind(user_id)
    .bind(service)
    .bind(provider)
    .bind(priority)
    .bind(config)
    .fetch_one(db)
    .await
    .map_err(|e| crate::error::pg_unique_to_conflict(e, "provider_exists"))
}

/// Partial update. Returns the updated row, or `AppError::NotFound` if
/// the row doesn't exist for this user (404 not 403).
pub async fn update(
    db: &PgPool,
    id: Uuid,
    user_id: Uuid,
    fields: UpdateFields,
) -> Result<ProviderConfig, AppError> {
    // Build the SET clause dynamically — only touch provided fields.
    // We use a single query with CASE expressions to avoid string concatenation.
    let row: Option<ProviderConfig> = sqlx::query_as(
        "UPDATE provider_configs \
         SET priority   = CASE WHEN $3::boolean THEN $4 ELSE priority END, \
             config     = CASE WHEN $5::boolean THEN $6 ELSE config END, \
             enabled    = CASE WHEN $7::boolean THEN $8 ELSE enabled END \
         WHERE id = $1 AND user_id = $2 \
         RETURNING id, user_id, service, provider, priority, config, enabled, created_at",
    )
    .bind(id)
    .bind(user_id)
    // priority
    .bind(fields.priority.is_some())
    .bind(fields.priority.unwrap_or(0))
    // config
    .bind(fields.config.is_some())
    .bind(fields.config.unwrap_or(serde_json::Value::Null))
    // enabled
    .bind(fields.enabled.is_some())
    .bind(fields.enabled.unwrap_or(false))
    .fetch_optional(db)
    .await
    .map_err(AppError::Database)?;

    row.ok_or(AppError::NotFound)
}

/// Delete a provider config. Returns `AppError::Conflict` if the provider
/// is a local provider (starts with `local_`), which cannot be deleted.
pub async fn delete(db: &PgPool, id: Uuid, user_id: Uuid) -> Result<(), AppError> {
    // First fetch to check the provider name and ownership (404 not 403)
    let row: Option<ProviderConfig> = sqlx::query_as(
        "SELECT id, user_id, service, provider, priority, config, enabled, created_at \
         FROM provider_configs WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(AppError::Database)?;

    let row = row.ok_or(AppError::NotFound)?;

    if row.provider.starts_with("local_") {
        return Err(AppError::Conflict {
            code: "cannot_delete",
        });
    }

    sqlx::query("DELETE FROM provider_configs WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(db)
        .await
        .map_err(AppError::Database)?;

    Ok(())
}

/// Find a single provider config by id for a user.
pub async fn find(
    db: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<ProviderConfig>, AppError> {
    sqlx::query_as(
        "SELECT id, user_id, service, provider, priority, config, enabled, created_at \
         FROM provider_configs WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .map_err(AppError::Database)
}

/// Insert the three default local provider rows for a newly created user.
/// Called from the user creation path. Silently ignores conflicts (idempotent).
pub async fn insert_local_defaults(db: &PgPool, user_id: Uuid) -> Result<(), AppError> {
    let defaults = [
        ("transcription", "local_whisper"),
        ("llm", "local_llama"),
        ("embeddings", "local_bge"),
    ];

    for (service, provider) in &defaults {
        sqlx::query(
            "INSERT INTO provider_configs (user_id, service, provider, priority, config) \
             VALUES ($1, $2, $3, 0, '{}') \
             ON CONFLICT (user_id, service, provider) DO NOTHING",
        )
        .bind(user_id)
        .bind(service)
        .bind(provider)
        .execute(db)
        .await
        .map_err(AppError::Database)?;
    }

    Ok(())
}

// ─── Deserializable request types (used by routes) ───────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProviderRequest {
    pub service: String,
    pub provider: String,
    pub priority: Option<i32>,
    /// Raw config; `api_key` field is encrypted server-side.
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchProviderRequest {
    pub priority: Option<i32>,
    /// Raw config patch; `api_key` field is encrypted server-side.
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}
