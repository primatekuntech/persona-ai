use crate::{
    auth::middleware::UserCtx,
    error::AppError,
    repositories::{personas as persona_repo, style_profiles},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;
use uuid::Uuid;

fn idempotency_key_from_headers(headers: &HeaderMap) -> Result<String, AppError> {
    let val = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Validation("Idempotency-Key header is required.".into()))?;

    Uuid::parse_str(val)
        .map_err(|_| AppError::Validation("Idempotency-Key must be a valid UUID.".into()))?;

    Ok(val.to_string())
}

async fn has_queued_job(
    pool: &sqlx::PgPool,
    persona_id: Uuid,
    era_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let count: i64 = if era_id.is_none() {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs
             WHERE kind = 'recompute_profile'
               AND status IN ('queued', 'running')
               AND payload->>'persona_id' = $1
               AND payload->>'era_id' IS NULL",
        )
        .bind(persona_id.to_string())
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs
             WHERE kind = 'recompute_profile'
               AND status IN ('queued', 'running')
               AND payload->>'persona_id' = $1
               AND payload->>'era_id' = $2",
        )
        .bind(persona_id.to_string())
        .bind(era_id.map(|e| e.to_string()))
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?
    };
    Ok(count > 0)
}

async fn has_any_documents(
    pool: &sqlx::PgPool,
    persona_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE persona_id = $1 AND user_id = $2")
            .bind(persona_id)
            .bind(user_id)
            .fetch_one(pool)
            .await
            .map_err(AppError::Database)?;
    Ok(count > 0)
}

pub async fn get_persona_profile(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // Verify persona ownership
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let has_job = has_queued_job(&state.db, persona_id, None).await?;
    let profile = style_profiles::find(&state.db, persona_id, None, ctx.user_id).await?;

    match (has_job, profile) {
        (true, None) => Ok((StatusCode::OK, Json(json!({"status": "pending"}))).into_response()),
        (true, Some(p)) => {
            let mut body = p.profile.clone();
            body["status"] = json!("pending");
            Ok((StatusCode::OK, Json(body)).into_response())
        }
        (false, Some(p)) => Ok((StatusCode::OK, Json(p.profile)).into_response()),
        (false, None) => {
            let has_docs = has_any_documents(&state.db, persona_id, ctx.user_id).await?;
            if has_docs {
                Ok((StatusCode::OK, Json(json!({"status": "pending"}))).into_response())
            } else {
                Err(AppError::NotFound)
            }
        }
    }
}

pub async fn get_era_profile(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, era_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let has_job = has_queued_job(&state.db, persona_id, Some(era_id)).await?;
    let profile = style_profiles::find(&state.db, persona_id, Some(era_id), ctx.user_id).await?;

    match (has_job, profile) {
        (true, None) => Ok((StatusCode::OK, Json(json!({"status": "pending"}))).into_response()),
        (true, Some(p)) => {
            let mut body = p.profile.clone();
            body["status"] = json!("pending");
            Ok((StatusCode::OK, Json(body)).into_response())
        }
        (false, Some(p)) => Ok((StatusCode::OK, Json(p.profile)).into_response()),
        (false, None) => {
            let has_docs = has_any_documents(&state.db, persona_id, ctx.user_id).await?;
            if has_docs {
                Ok((StatusCode::OK, Json(json!({"status": "pending"}))).into_response())
            } else {
                Err(AppError::NotFound)
            }
        }
    }
}

pub async fn recompute_profile(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    idempotency_key_from_headers(&headers)?;

    // Verify persona ownership
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    sqlx::query(
        "INSERT INTO jobs (kind, user_id, persona_id, payload)
         VALUES ('recompute_profile', $1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(ctx.user_id)
    .bind(persona_id)
    .bind(json!({ "persona_id": persona_id.to_string(), "era_id": serde_json::Value::Null }))
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((StatusCode::ACCEPTED, Json(json!({"status": "queued"}))).into_response())
}
