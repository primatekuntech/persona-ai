/// Authentication route handlers.
/// Covers: login, logout, revoke-all, me, forgot/reset password, invite validate/accept.
use crate::{
    auth::{
        middleware::UserCtx,
        password::{dummy_verify, hash, validate_new_password, verify},
        session::{record_session, remove_session, revoke_all_sessions, SESSION_USER_ID_KEY},
    },
    error::AppError,
    repositories::{
        invites::{
            create_password_reset, find_active_invite, generate_token,
            hash_token, use_invite, use_reset_token,
        },
        users::{find_by_email, touch_login},
    },
    state::AppState,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::IpAddr;
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

// ─── Login / Logout ─────────────────────────────────────────────────────────

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let email = body.email.trim().to_lowercase();
    let ip = extract_ip(&headers);

    // Per-account brute-force check
    let recent_failures = count_recent_failures(&state.db, &email).await?;
    if recent_failures >= 5 {
        let retry_after = oldest_failure_age_secs(&state.db, &email).await?;
        record_attempt(&state.db, &email, ip, false).await?;
        return Err(AppError::RateLimited {
            retry_after_secs: retry_after,
        });
    }

    // Progressive delay: 200ms per failure, capped at 2s
    let delay_ms = (recent_failures as u64 * 200).min(2000);
    if delay_ms > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
    }

    let user = match find_by_email(&state.db, &email).await? {
        Some(u) => u,
        None => {
            dummy_verify(); // constant-time to prevent user enumeration
            record_attempt(&state.db, &email, ip, false).await?;
            crate::audit::log(
                &state.db,
                None,
                "user.login_failed",
                None,
                None,
                ip,
                Some(json!({ "email": email, "reason": "user_not_found" })),
            )
            .await?;
            return Err(AppError::Unauthorized);
        }
    };

    // Treat any verify error (e.g. malformed hash) the same as a wrong password so we
    // never leak information about hash validity through a differing HTTP status code.
    let password_ok = verify(&body.password, &user.password_hash).unwrap_or(false);
    if !password_ok {
        record_attempt(&state.db, &email, ip, false).await?;
        crate::audit::log(
            &state.db,
            Some(user.id),
            "user.login_failed",
            None,
            None,
            ip,
            Some(json!({ "email": email, "reason": "wrong_password" })),
        )
        .await?;
        return Err(AppError::Unauthorized);
    }

    if user.status != "active" {
        return Err(AppError::Forbidden {
            code: "account_disabled",
        });
    }

    // Clear failed attempts on success
    clear_failures(&state.db, &email).await?;
    record_attempt(&state.db, &email, ip, true).await?;

    // Session fixation prevention: cycle the session ID
    session.cycle_id().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("session cycle failed: {e}"))
    })?;
    session
        .insert(SESSION_USER_ID_KEY, user.id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert failed: {e}")))?;

    let ttl_hours = state.config.session_ttl_hours;
    let expires_at = OffsetDateTime::now_utc() + Duration::hours(ttl_hours as i64);
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    record_session(&state.db, &session, user.id, ip, user_agent, expires_at).await?;
    touch_login(&state.db, user.id).await?;

    crate::audit::log(
        &state.db,
        Some(user.id),
        "user.login",
        None,
        None,
        ip,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn logout(
    State(state): State<AppState>,
    ctx: UserCtx,
    session: Session,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let ip = extract_ip(&headers);
    remove_session(&state.db, &session).await?;
    session.delete().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("session delete failed: {e}"))
    })?;

    crate::audit::log(
        &state.db,
        Some(ctx.user_id),
        "user.logout",
        None,
        None,
        ip,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke_all_sessions(
    State(state): State<AppState>,
    ctx: UserCtx,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let ip = extract_ip(&headers);
    crate::auth::session::revoke_all_sessions(&state.db, ctx.user_id).await?;

    crate::audit::log(
        &state.db,
        Some(ctx.user_id),
        "user.sessions_revoked",
        None,
        None,
        ip,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct MeResponse {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub display_name: Option<String>,
}

pub async fn me(
    State(state): State<AppState>,
    ctx: UserCtx,
) -> Result<impl IntoResponse, AppError> {
    let user = crate::repositories::users::find_by_id(&state.db, ctx.user_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    Ok(Json(MeResponse {
        user_id: user.id,
        email: user.email,
        role: user.role,
        display_name: user.display_name,
    }))
}

// ─── Password reset ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

pub async fn forgot_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    let email = body.email.trim().to_lowercase();
    let ip = extract_ip(&headers);

    if let Some(user) = find_by_email(&state.db, &email).await? {
        if user.status == "active" {
            let plaintext = generate_token();
            let token_hash = hash_token(&plaintext);
            let expires_at = OffsetDateTime::now_utc() + Duration::minutes(30);

            create_password_reset(&state.db, &token_hash, user.id, expires_at).await?;

            let reset_url = format!(
                "{}/reset-password?token={plaintext}",
                state.config.app_base_url
            );
            if let Err(e) = state.email.send_password_reset(&email, &reset_url).await {
                tracing::warn!(error = %e, email = %email, "password reset email failed to send (non-fatal)");
            }

            crate::audit::log(
                &state.db,
                Some(user.id),
                "password.reset_requested",
                None,
                None,
                ip,
                None,
            )
            .await?;
        }
    } else {
        // Timing parity: spend a dummy argon2 hash even if user doesn't exist
        dummy_verify();
    }

    // Always 204 — never leak account existence
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

pub async fn reset_password(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    let ip = extract_ip(&headers);
    let token_hash = hash_token(&body.token);

    // Validate the new password before touching the database.
    validate_new_password(&body.new_password)?;
    let new_hash = hash(&body.new_password)?;

    let pool = &state.db;

    // Begin the transaction FIRST so the FOR UPDATE lock inside use_reset_token prevents
    // two simultaneous resets from both succeeding.
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    let reset = use_reset_token(&mut tx, &token_hash)
        .await?
        .ok_or(AppError::Gone { code: "invalid_token" })?;

    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(reset.user_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    // Revoke all existing sessions
    crate::auth::session::revoke_all_sessions(pool, reset.user_id).await?;

    // Issue a fresh session
    session.cycle_id().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("session cycle: {e}"))
    })?;
    session
        .insert(SESSION_USER_ID_KEY, reset.user_id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert: {e}")))?;

    let expires_at =
        OffsetDateTime::now_utc() + Duration::hours(state.config.session_ttl_hours as i64);
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    record_session(pool, &session, reset.user_id, ip, ua, expires_at).await?;

    crate::audit::log(
        pool,
        Some(reset.user_id),
        "password.reset_completed",
        None,
        None,
        ip,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Invite flow ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ValidateInviteQuery {
    pub token: String,
}

#[derive(Serialize)]
pub struct InviteValidateResponse {
    pub email: String,
    pub role: String,
    pub expires_at: OffsetDateTime,
}

pub async fn validate_invite(
    State(state): State<AppState>,
    Query(q): Query<ValidateInviteQuery>,
) -> Result<impl IntoResponse, AppError> {
    let token_hash = hash_token(&q.token);
    let invite = find_active_invite(&state.db, &token_hash)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(InviteValidateResponse {
        email: invite.email,
        role: invite.role,
        expires_at: invite.expires_at,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptInviteRequest {
    pub token: String,
    pub password: String,
    pub display_name: String,
}

pub async fn accept_invite(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(body): Json<AcceptInviteRequest>,
) -> Result<impl IntoResponse, AppError> {
    let ip = extract_ip(&headers);
    let token_hash = hash_token(&body.token);

    validate_new_password(&body.password)?;

    let display_name = body.display_name.trim().to_owned();
    if display_name.is_empty() {
        return Err(AppError::Validation("Display name is required.".into()));
    }

    let pool = &state.db;
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // use_invite acquires a FOR UPDATE lock, marks used_at, and returns (email, role)
    // atomically.  A concurrent accept will block here and then find used_at IS NOT NULL.
    let (email, role) = use_invite(&mut tx, &token_hash)
        .await?
        .ok_or(AppError::Gone { code: "invalid_token" })?;

    let pw_hash = hash(&body.password)?;

    let user = sqlx::query_as::<_, crate::repositories::users::User>(
        r#"INSERT INTO users (email, password_hash, role, display_name)
           VALUES ($1, $2, $3, $4)
           RETURNING id, email, password_hash, role, status, display_name, created_at, last_login_at"#,
    )
    .bind(&email)
    .bind(&pw_hash)
    .bind(&role)
    .bind(&display_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // Set used_by now that we have the real user ID (same transaction — atomic).
    sqlx::query("UPDATE invite_tokens SET used_by = $1 WHERE token_hash = $2")
        .bind(user.id)
        .bind(&token_hash)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    // Start session
    session.cycle_id().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("session cycle: {e}"))
    })?;
    session
        .insert(SESSION_USER_ID_KEY, user.id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert: {e}")))?;

    let expires_at =
        OffsetDateTime::now_utc() + Duration::hours(state.config.session_ttl_hours as i64);
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    record_session(pool, &session, user.id, ip, ua, expires_at).await?;

    crate::audit::log(
        pool,
        Some(user.id),
        "invite.accepted",
        Some("invite_token"),
        Some(&token_hash),
        ip,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Brute-force helpers ────────────────────────────────────────────────────

async fn count_recent_failures(pool: &sqlx::PgPool, email: &str) -> Result<i64, AppError> {
    sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)
}

/// Returns seconds until the oldest failure in the window ages out (≈ 15 min).
async fn oldest_failure_age_secs(pool: &sqlx::PgPool, email: &str) -> Result<u64, AppError> {
    let oldest: Option<OffsetDateTime> = sqlx::query_scalar(
        r#"SELECT MIN(attempted_at) FROM login_attempts
           WHERE email = $1 AND success = false
             AND attempted_at > now() - interval '15 minutes'"#,
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let secs = oldest
        .map(|ts| {
            let age_out = ts + Duration::minutes(15);
            let remaining = age_out - OffsetDateTime::now_utc();
            remaining.whole_seconds().max(0) as u64
        })
        .unwrap_or(900);

    Ok(secs)
}

async fn record_attempt(
    pool: &sqlx::PgPool,
    email: &str,
    ip: Option<IpAddr>,
    success: bool,
) -> Result<(), AppError> {
    let ip_str = ip.map(|a| a.to_string());
    sqlx::query(
        "INSERT INTO login_attempts (email, ip, success) VALUES ($1, $2::inet, $3)",
    )
    .bind(email)
    .bind(ip_str.as_deref())
    .bind(success)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

async fn clear_failures(pool: &sqlx::PgPool, email: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM login_attempts WHERE email = $1 AND success = false")
        .bind(email)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

fn extract_ip(headers: &HeaderMap) -> Option<IpAddr> {
    // Prefer X-Forwarded-For (set by Caddy/load balancer)
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
        })
}
