use crate::{
    error::AppError,
    repositories::style_profiles,
    services::analysis::{build_profile, Chunk, ProfileCtx},
};
use sqlx::PgPool;
use uuid::Uuid;

struct ChunkRow {
    id: Uuid,
    text: String,
    token_count: i32,
    embedding: Option<pgvector::Vector>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for ChunkRow {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(ChunkRow {
            id: row.try_get("id")?,
            text: row.try_get("text")?,
            token_count: row.try_get("token_count")?,
            embedding: row.try_get("embedding")?,
        })
    }
}

pub async fn run_recompute_profile(
    pool: &PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
) -> Result<(), AppError> {
    // Fetch user_id for this persona
    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM personas WHERE id = $1")
        .bind(persona_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::NotFound)?;

    // If this is a persona-wide job, also compute per-era profiles.
    if era_id.is_none() {
        let era_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT era_id FROM chunks
             WHERE persona_id = $1 AND user_id = $2 AND era_id IS NOT NULL",
        )
        .bind(persona_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

        for eid in era_ids {
            compute_and_store(pool, persona_id, Some(eid), user_id).await?;
        }
    }

    compute_and_store(pool, persona_id, era_id, user_id).await?;

    Ok(())
}

async fn compute_and_store(
    pool: &PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
    user_id: Uuid,
) -> Result<(), AppError> {
    // Fetch chunks
    let chunk_rows: Vec<ChunkRow> = if let Some(eid) = era_id {
        sqlx::query_as(
            "SELECT id, text, token_count, embedding
             FROM chunks
             WHERE persona_id = $1 AND user_id = $2 AND era_id = $3
             ORDER BY chunk_index",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(eid)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_as(
            "SELECT id, text, token_count, embedding
             FROM chunks
             WHERE persona_id = $1 AND user_id = $2
             ORDER BY chunk_index",
        )
        .bind(persona_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    };

    let corpus_tokens: i32 = chunk_rows.iter().map(|c| c.token_count).sum();

    // Document count for the scope
    let document_count: i64 = if let Some(eid) = era_id {
        sqlx::query_scalar(
            "SELECT COUNT(DISTINCT document_id) FROM chunks
             WHERE persona_id = $1 AND user_id = $2 AND era_id = $3",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(eid)
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM documents
             WHERE persona_id = $1 AND user_id = $2",
        )
        .bind(persona_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    };

    // Date range from documents
    let date_range: Option<(String, String)> = if let Some(eid) = era_id {
        sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT MIN(created_at)::text, MAX(created_at)::text FROM documents
             WHERE persona_id = $1 AND user_id = $2 AND era_id = $3",
        )
        .bind(persona_id)
        .bind(user_id)
        .bind(eid)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .and_then(|(a, b)| Some((a?, b?)))
    } else {
        sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT MIN(created_at)::text, MAX(created_at)::text FROM documents
             WHERE persona_id = $1 AND user_id = $2",
        )
        .bind(persona_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .and_then(|(a, b)| Some((a?, b?)))
    };

    let ctx = ProfileCtx {
        document_count,
        date_range,
    };

    let chunks: Vec<Chunk> = chunk_rows
        .into_iter()
        .map(|r| Chunk {
            id: r.id,
            text: r.text,
            token_count: r.token_count,
            embedding: r.embedding.map(|v| v.to_vec()),
        })
        .collect();

    // CPU-bound: run in spawn_blocking
    let profile_json = tokio::task::spawn_blocking(move || build_profile(&chunks, &ctx))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("analysis spawn_blocking: {e}")))?;

    style_profiles::upsert(
        pool,
        persona_id,
        era_id,
        user_id,
        corpus_tokens,
        profile_json,
    )
    .await?;

    Ok(())
}
