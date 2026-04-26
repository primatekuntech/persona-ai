use crate::error::AppError;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StyleProfile {
    pub id: Uuid,
    pub persona_id: Uuid,
    pub era_id: Option<Uuid>,
    pub user_id: Uuid,
    pub corpus_tokens: i32,
    pub profile: serde_json::Value,
    pub computed_at: OffsetDateTime,
}

pub async fn upsert(
    pool: &PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
    user_id: Uuid,
    corpus_tokens: i32,
    profile: serde_json::Value,
) -> Result<StyleProfile, AppError> {
    let row = if era_id.is_none() {
        sqlx::query_as::<_, StyleProfile>(
            "INSERT INTO style_profiles (persona_id, era_id, user_id, corpus_tokens, profile)
             VALUES ($1, NULL, $2, $3, $4)
             ON CONFLICT (persona_id) WHERE era_id IS NULL
             DO UPDATE SET corpus_tokens = EXCLUDED.corpus_tokens,
                           profile = EXCLUDED.profile,
                           computed_at = now()
             RETURNING id, persona_id, era_id, user_id, corpus_tokens, profile, computed_at",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(corpus_tokens)
        .bind(profile)
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_as::<_, StyleProfile>(
            "INSERT INTO style_profiles (persona_id, era_id, user_id, corpus_tokens, profile)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (persona_id, era_id) WHERE era_id IS NOT NULL
             DO UPDATE SET corpus_tokens = EXCLUDED.corpus_tokens,
                           profile = EXCLUDED.profile,
                           computed_at = now()
             RETURNING id, persona_id, era_id, user_id, corpus_tokens, profile, computed_at",
        )
        .bind(persona_id)
        .bind(era_id)
        .bind(user_id)
        .bind(corpus_tokens)
        .bind(profile)
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    };

    Ok(row)
}

pub async fn find(
    pool: &PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
    user_id: Uuid,
) -> Result<Option<StyleProfile>, AppError> {
    let row = if era_id.is_none() {
        sqlx::query_as::<_, StyleProfile>(
            "SELECT id, persona_id, era_id, user_id, corpus_tokens, profile, computed_at
             FROM style_profiles
             WHERE persona_id = $1 AND era_id IS NULL AND user_id = $2",
        )
        .bind(persona_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_as::<_, StyleProfile>(
            "SELECT id, persona_id, era_id, user_id, corpus_tokens, profile, computed_at
             FROM style_profiles
             WHERE persona_id = $1 AND era_id = $2 AND user_id = $3",
        )
        .bind(persona_id)
        .bind(era_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
    };

    Ok(row)
}

#[allow(dead_code)]
pub async fn delete_for_persona(pool: &PgPool, persona_id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM style_profiles WHERE persona_id = $1")
        .bind(persona_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}
