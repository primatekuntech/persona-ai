/// Admin route handlers: invites, users, jobs, errors, audit.
use crate::{
    auth::middleware::AdminCtx,
    error::AppError,
    repositories::{invites as invite_repo, users as user_repo},
    services::invites as invite_svc,
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

// ─── Invites ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInviteRequest {
    pub email: String,
    pub role: Option<String>,
}

#[derive(Serialize)]
pub struct CreateInviteResponse {
    pub invite_url: String,
    pub token_hash: String,
    pub expires_at: OffsetDateTime,
}

pub async fn create_invite(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Json(body): Json<CreateInviteRequest>,
) -> Result<impl IntoResponse, AppError> {
    let email = body.email.trim().to_lowercase();
    let role = body.role.as_deref().unwrap_or("user");

    if !["admin", "user"].contains(&role) {
        return Err(AppError::Validation("Invalid role.".into()));
    }

    let result = invite_svc::create(
        &state.db,
        &email,
        role,
        ctx.user_id,
        &state.config.app_base_url,
    )
    .await?;

    let expires_at = OffsetDateTime::now_utc() + time::Duration::days(invite_svc::INVITE_TTL_DAYS);

    crate::audit::log(
        &state.db,
        Some(ctx.user_id),
        "invite.created",
        Some("invite_token"),
        Some(&result.token_hash),
        None,
        Some(json!({ "email": email, "role": role })),
    )
    .await?;

    // Send the invite email (non-fatal if it fails)
    let inviter_name = "the admin"; // TODO: use ctx.display_name in sprint 2
    let _ = state
        .email
        .send_invite(&email, &result.invite_url, inviter_name)
        .await;

    Ok((
        StatusCode::CREATED,
        Json(CreateInviteResponse {
            invite_url: result.invite_url,
            token_hash: result.token_hash,
            expires_at,
        }),
    ))
}

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub cursor_ts: Option<OffsetDateTime>,
    pub cursor_id: Option<String>,
}

pub async fn list_invites(
    State(state): State<AppState>,
    _: AdminCtx,
    Query(q): Query<PaginationQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let items = invite_repo::list_invites(&state.db, limit, q.cursor_ts, q.cursor_id).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|i| base64_cursor(i.created_at, &i.token_hash))
    } else {
        None
    };

    Ok(Json(json!({
        "items": items,
        "next_cursor": next_cursor
    })))
}

pub async fn revoke_invite(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(token_hash): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = invite_repo::revoke_invite(&state.db, &token_hash).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }

    crate::audit::log(
        &state.db,
        Some(ctx.user_id),
        "invite.revoked",
        Some("invite_token"),
        Some(&token_hash),
        None,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Users ────────────────────────────────────────────────────────────────────

pub async fn list_users(
    State(state): State<AppState>,
    _: AdminCtx,
    Query(q): Query<PaginationQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    // Decode cursor
    let (cursor_ts, cursor_id) = if let Some(ref id) = q.cursor_id {
        (
            q.cursor_ts,
            Some(Uuid::parse_str(id).map_err(|_| AppError::Validation("invalid cursor".into()))?),
        )
    } else {
        (None, None)
    };

    let items = user_repo::list(&state.db, limit, cursor_ts, cursor_id).await?;
    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|u| base64_cursor(u.created_at, &u.id.to_string()))
    } else {
        None
    };

    Ok(Json(json!({
        "items": items,
        "next_cursor": next_cursor
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchUserRequest {
    pub status: Option<String>,
    pub role: Option<String>,
}

pub async fn patch_user(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(user_id): Path<Uuid>,
    Json(body): Json<PatchUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(ref s) = body.status {
        if !["active", "disabled"].contains(&s.as_str()) {
            return Err(AppError::Validation("Invalid status.".into()));
        }
    }
    if let Some(ref r) = body.role {
        if !["admin", "user"].contains(&r.as_str()) {
            return Err(AppError::Validation("Invalid role.".into()));
        }
    }

    // Verify target user exists
    user_repo::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    user_repo::update_role_status(
        &state.db,
        user_id,
        body.role.as_deref(),
        body.status.as_deref(),
    )
    .await?;

    if body.status.as_deref() == Some("disabled") {
        crate::audit::log(
            &state.db,
            Some(ctx.user_id),
            "user.disabled",
            Some("user"),
            Some(&user_id.to_string()),
            None,
            None,
        )
        .await?;
    } else if body.status.as_deref() == Some("active") {
        crate::audit::log(
            &state.db,
            Some(ctx.user_id),
            "user.enabled",
            Some("user"),
            Some(&user_id.to_string()),
            None,
            None,
        )
        .await?;
    }
    if body.role.is_some() {
        crate::audit::log(
            &state.db,
            Some(ctx.user_id),
            "user.role_changed",
            Some("user"),
            Some(&user_id.to_string()),
            None,
            Some(json!({ "new_role": body.role })),
        )
        .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Admin-triggered password reset: generates a reset URL for the target user.
pub async fn admin_reset_user(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    Path(user_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    user_repo::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let plaintext = invite_repo::generate_token();
    let token_hash = invite_repo::hash_token(&plaintext);
    let expires_at = OffsetDateTime::now_utc() + time::Duration::minutes(30);

    invite_repo::create_password_reset(&state.db, &token_hash, user_id, expires_at).await?;

    let reset_url = format!(
        "{}/reset-password?token={plaintext}",
        state.config.app_base_url
    );

    crate::audit::log(
        &state.db,
        Some(ctx.user_id),
        "password.reset_requested",
        Some("user"),
        Some(&user_id.to_string()),
        None,
        None,
    )
    .await?;

    Ok(Json(json!({ "reset_url": reset_url })))
}

fn base64_cursor(ts: OffsetDateTime, id: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let s = format!("{}|{}", ts.unix_timestamp_nanos(), id);
    URL_SAFE_NO_PAD.encode(s.as_bytes())
}

// ─── Jobs ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JobsQuery {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub cursor_ts: Option<OffsetDateTime>,
    pub cursor_id: Option<String>,
}

pub async fn list_jobs(
    _: AdminCtx,
    State(state): State<AppState>,
    Query(q): Query<JobsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    // Aggregate counts by kind+status
    let counts: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT json_build_object('kind', kind, 'status', status, 'count', COUNT(*)) \
         FROM jobs GROUP BY kind, status ORDER BY kind, status",
    )
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)?;

    // Oldest queued job age (seconds)
    let oldest_queued_secs: Option<f64> = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (now() - MIN(scheduled_at))) FROM jobs WHERE status='queued'",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .flatten();

    // Longest running job age (seconds)
    let longest_running_secs: Option<f64> = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (now() - MIN(started_at))) FROM jobs WHERE status='running'",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .flatten();

    // Paginated items
    let items: Vec<serde_json::Value> = if let Some(ref cts_str) = q.cursor_id {
        // cursor_ts + cursor_id
        let cursor_ts = q.cursor_ts;
        let cursor_id = Uuid::parse_str(cts_str)
            .map_err(|_| AppError::Validation("invalid cursor_id".into()))?;

        sqlx::query_scalar(
            "SELECT row_to_json(j) FROM \
             (SELECT id, kind, user_id, persona_id, status, attempts, \
                     last_error, scheduled_at, started_at, finished_at \
              FROM jobs \
              WHERE ($1::text IS NULL OR kind=$1) \
                AND ($2::text IS NULL OR status=$2) \
                AND (created_at, id) < ($3::timestamptz, $4::uuid) \
              ORDER BY created_at DESC, id DESC \
              LIMIT $5) j",
        )
        .bind(q.kind.as_deref())
        .bind(q.status.as_deref())
        .bind(cursor_ts)
        .bind(cursor_id)
        .bind(limit + 1)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_scalar(
            "SELECT row_to_json(j) FROM \
             (SELECT id, kind, user_id, persona_id, status, attempts, \
                     last_error, scheduled_at, started_at, finished_at \
              FROM jobs \
              WHERE ($1::text IS NULL OR kind=$1) \
                AND ($2::text IS NULL OR status=$2) \
              ORDER BY created_at DESC, id DESC \
              LIMIT $3) j",
        )
        .bind(q.kind.as_deref())
        .bind(q.status.as_deref())
        .bind(limit + 1)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::Database)?
    };

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();

    // Build next_cursor from the last item's created_at + id
    let next_cursor: Option<String> = if has_more {
        items.last().and_then(|v| {
            let ts_str = v["created_at"].as_str()?;
            let id_str = v["id"].as_str()?;
            let ts = OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
                .ok()?;
            Some(base64_cursor(ts, id_str))
        })
    } else {
        None
    };

    Ok(Json(json!({
        "counts": counts,
        "oldest_queued_secs": oldest_queued_secs,
        "longest_running_secs": longest_running_secs,
        "items": items,
        "next_cursor": next_cursor,
    })))
}

pub async fn retry_job(
    _: AdminCtx,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let result = sqlx::query(
        "UPDATE jobs SET status='queued', scheduled_at=now(), attempts=0, \
         last_error=NULL, worker_id=NULL, started_at=NULL, finished_at=NULL \
         WHERE id=$1 AND status='failed'",
    )
    .bind(job_id)
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_job(
    _: AdminCtx,
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let result = sqlx::query("DELETE FROM jobs WHERE id=$1 AND status='queued'")
        .bind(job_id)
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Errors ───────────────────────────────────────────────────────────────────

pub async fn list_errors(
    _: AdminCtx,
    State(state): State<AppState>,
    Query(q): Query<PaginationQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    // The errors table exists per migration; handle gracefully if it doesn't.
    let result: Result<Vec<serde_json::Value>, sqlx::Error> =
        if let (Some(ts), Some(ref id_str)) = (q.cursor_ts, &q.cursor_id) {
            let cursor_id = match Uuid::parse_str(id_str) {
                Ok(id) => id,
                Err(_) => {
                    return Err(AppError::Validation("invalid cursor_id".into()));
                }
            };
            sqlx::query_scalar(
                "SELECT row_to_json(e) FROM \
             (SELECT id, user_id, route, code, message, request_id, ip, created_at \
              FROM errors \
              WHERE (created_at, id) < ($1::timestamptz, $2::uuid) \
              ORDER BY created_at DESC, id DESC \
              LIMIT $3) e",
            )
            .bind(ts)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&state.db)
            .await
        } else {
            sqlx::query_scalar(
                "SELECT row_to_json(e) FROM \
             (SELECT id, user_id, route, code, message, request_id, ip, created_at \
              FROM errors \
              ORDER BY created_at DESC, id DESC \
              LIMIT $1) e",
            )
            .bind(limit + 1)
            .fetch_all(&state.db)
            .await
        };

    let items = match result {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error=%e, "list_errors: query failed (table may not exist)");
            Vec::new()
        }
    };

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();

    let next_cursor: Option<String> = if has_more {
        items.last().and_then(|v| {
            let ts_str = v["created_at"].as_str()?;
            let id_str = v["id"].as_str()?;
            let ts = OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
                .ok()?;
            Some(base64_cursor(ts, id_str))
        })
    } else {
        None
    };

    Ok(Json(json!({
        "items": items,
        "next_cursor": next_cursor,
    })))
}

// ─── Audit ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    pub user_id: Option<Uuid>,
    pub action: Option<String>,
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
    pub limit: Option<i64>,
    pub cursor_ts: Option<OffsetDateTime>,
    pub cursor_id: Option<i64>,
}

pub async fn list_audit(
    _: AdminCtx,
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let items: Vec<serde_json::Value> = if let (Some(ts), Some(cid)) = (q.cursor_ts, q.cursor_id) {
        sqlx::query_scalar(
            "SELECT row_to_json(a) FROM \
             (SELECT id, user_id, action, resource_type, resource_id, ip, metadata, created_at \
              FROM audit_log \
              WHERE ($1::uuid IS NULL OR user_id=$1) \
                AND ($2::text IS NULL OR action=$2) \
                AND ($3::timestamptz IS NULL OR created_at >= $3) \
                AND ($4::timestamptz IS NULL OR created_at <= $4) \
                AND (created_at, id) < ($5::timestamptz, $6::bigint) \
              ORDER BY created_at DESC, id DESC \
              LIMIT $7) a",
        )
        .bind(q.user_id)
        .bind(q.action.as_deref())
        .bind(q.from)
        .bind(q.to)
        .bind(ts)
        .bind(cid)
        .bind(limit + 1)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::Database)?
    } else {
        sqlx::query_scalar(
            "SELECT row_to_json(a) FROM \
             (SELECT id, user_id, action, resource_type, resource_id, ip, metadata, created_at \
              FROM audit_log \
              WHERE ($1::uuid IS NULL OR user_id=$1) \
                AND ($2::text IS NULL OR action=$2) \
                AND ($3::timestamptz IS NULL OR created_at >= $3) \
                AND ($4::timestamptz IS NULL OR created_at <= $4) \
              ORDER BY created_at DESC, id DESC \
              LIMIT $5) a",
        )
        .bind(q.user_id)
        .bind(q.action.as_deref())
        .bind(q.from)
        .bind(q.to)
        .bind(limit + 1)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::Database)?
    };

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();

    let next_cursor: Option<String> = if has_more {
        items.last().and_then(|v| {
            let ts_str = v["created_at"].as_str()?;
            let id_str = v["id"].as_i64()?;
            let ts = OffsetDateTime::parse(ts_str, &time::format_description::well_known::Rfc3339)
                .ok()?;
            Some(base64_cursor(ts, &id_str.to_string()))
        })
    } else {
        None
    };

    Ok(Json(json!({
        "items": items,
        "next_cursor": next_cursor,
    })))
}
