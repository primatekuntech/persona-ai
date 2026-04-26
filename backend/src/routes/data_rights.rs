/// Data rights route handlers: export and account deletion.
///
/// - POST /api/auth/export      → 202 { "job_id": "..." }
/// - GET  /api/auth/export/:job_id → { "status": ..., "progress_pct": N, "download_url": null|"..." }
/// - GET  /api/auth/export/:job_id/download → zip file (HMAC-signed URL)
/// - POST /api/auth/delete      → 202 { "job_id": "..." }
use crate::{
    auth::{middleware::UserCtx, password::verify as verify_password},
    error::AppError,
    state::AppState,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use time::OffsetDateTime;
use uuid::Uuid;

// ─── HMAC signing helpers ─────────────────────────────────────────────────────

fn sign_export_url(job_id: Uuid, user_id: Uuid, expires_at: i64, secret: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{job_id}:{user_id}:{expires_at}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn build_download_url(base_url: &str, job_id: Uuid, user_id: Uuid, secret: &str) -> String {
    let expires_at = (OffsetDateTime::now_utc() + time::Duration::minutes(15)).unix_timestamp();
    let sig = sign_export_url(job_id, user_id, expires_at, secret);
    format!("{base_url}/api/auth/export/{job_id}/download?expires={expires_at}&sig={sig}")
}

// ─── POST /api/auth/export ────────────────────────────────────────────────────

pub async fn request_export(
    State(state): State<AppState>,
    ctx: UserCtx,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;

    // Check for an existing pending/running export job
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE kind='user_export' AND user_id=$1 \
         AND status IN ('queued','running') LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    if existing.is_some() {
        return Err(AppError::Conflict {
            code: "export_in_progress",
        });
    }

    // 429 if a completed export exists within the last 24 h
    let recent: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE kind='user_export' AND user_id=$1 \
         AND status='done' AND finished_at > now() - interval '24 hours' LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    if recent.is_some() {
        return Err(AppError::RateLimited {
            retry_after_secs: 86400,
        });
    }

    // Enqueue the job
    let job_id: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs (kind, user_id, payload) VALUES ('user_export', $1, '{}'::jsonb) \
         RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

// ─── GET /api/auth/export/:job_id ─────────────────────────────────────────────

pub async fn get_export_status(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(job_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;

    let row: Option<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT status, payload FROM jobs \
         WHERE id=$1 AND user_id=$2 AND kind='user_export'",
    )
    .bind(job_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?;

    let (status, payload) = row.ok_or(AppError::NotFound)?;
    let progress_pct: i32 = payload
        .get("progress_pct")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    // Build download URL only if done and file exists
    let download_url: Option<String> = if status == "done" {
        let export_path = state
            .config
            .data_dir
            .join("exports")
            .join(user_id.to_string())
            .join(format!("{job_id}.zip"));

        if export_path.exists() {
            Some(build_download_url(
                &state.config.app_base_url,
                job_id,
                user_id,
                &state.config.session_secret,
            ))
        } else {
            // File missing — mark as expired if payload doesn't already say expired
            let _ = sqlx::query(
                "UPDATE jobs SET status='failed', last_error='export file missing' \
                 WHERE id=$1 AND status='done'",
            )
            .bind(job_id)
            .execute(&state.db)
            .await;
            None
        }
    } else {
        None
    };

    // Reflect expiry if file was deleted
    let effective_status = if status == "done" && download_url.is_none() {
        "expired"
    } else {
        &status
    };

    // Expiry: check if the job's file is older than 7 days
    let final_status = if status == "done" {
        let is_old: bool = sqlx::query_scalar(
            "SELECT finished_at < now() - interval '7 days' FROM jobs WHERE id=$1",
        )
        .bind(job_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or(false);

        if is_old {
            "expired"
        } else {
            effective_status
        }
    } else {
        effective_status
    };

    Ok(Json(json!({
        "status": final_status,
        "progress_pct": progress_pct,
        "download_url": download_url,
    })))
}

// ─── GET /api/auth/export/:job_id/download ────────────────────────────────────

#[derive(Deserialize)]
pub struct DownloadQuery {
    pub expires: i64,
    pub sig: String,
}

pub async fn download_export(
    State(state): State<AppState>,
    Path(job_id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Verify expiry
    let now_ts = OffsetDateTime::now_utc().unix_timestamp();
    if now_ts > q.expires {
        return Err(AppError::Gone {
            code: "download_expired",
        });
    }

    // Fetch the job to get user_id (we need it to verify the HMAC)
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT user_id, status FROM jobs WHERE id=$1 AND kind='user_export'")
            .bind(job_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::Database)?;

    let (user_id, status) = row.ok_or(AppError::NotFound)?;

    if status != "done" {
        return Err(AppError::NotFound);
    }

    // Verify HMAC signature
    let expected_sig = sign_export_url(job_id, user_id, q.expires, &state.config.session_secret);
    use subtle::ConstantTimeEq;
    let sig_match: bool = expected_sig.as_bytes().ct_eq(q.sig.as_bytes()).into();
    if !sig_match {
        return Err(AppError::Forbidden {
            code: "invalid_signature",
        });
    }

    // Locate file
    let export_path = state
        .config
        .data_dir
        .join("exports")
        .join(user_id.to_string())
        .join(format!("{job_id}.zip"));

    if !export_path.exists() {
        return Err(AppError::NotFound);
    }

    let file_bytes = tokio::fs::read(&export_path)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("read export file: {e}")))?;

    let filename = format!("export-{job_id}.zip");
    let mut resp = axum::response::Response::new(Body::from(file_bytes));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or(HeaderValue::from_static("attachment")),
    );
    Ok(resp)
}

// ─── POST /api/auth/delete ────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteAccountRequest {
    pub password: String,
    pub confirm: String,
}

#[derive(Serialize)]
pub struct DeleteAccountResponse {
    pub job_id: Uuid,
}

pub async fn delete_account(
    State(state): State<AppState>,
    ctx: UserCtx,
    Json(body): Json<DeleteAccountRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;

    // Validate confirm field
    if body.confirm != "DELETE" {
        return Err(AppError::Validation(
            "confirm must be exactly 'DELETE'.".into(),
        ));
    }

    // Fetch user record
    let user = crate::repositories::users::find_by_id(&state.db, user_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    // Re-verify password
    let ok = verify_password(&body.password, &user.password_hash).unwrap_or(false);
    if !ok {
        return Err(AppError::Forbidden {
            code: "invalid_password",
        });
    }

    // Guard: must not be the last active admin
    if user.role == "admin" {
        let active_admin_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin' AND status='active'")
                .fetch_one(&state.db)
                .await
                .map_err(AppError::Database)?;
        if active_admin_count <= 1 {
            return Err(AppError::Conflict { code: "last_admin" });
        }
    }

    // Delete all sessions from session_index
    sqlx::query("DELETE FROM session_index WHERE user_id=$1")
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;

    // Anonymise user record
    let deleted_email = format!("deleted-{user_id}@invalid");
    sqlx::query("UPDATE users SET status='disabled', email=$1, password_hash='' WHERE id=$2")
        .bind(&deleted_email)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;

    // Enqueue user_delete job
    let job_id: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs (kind, user_id, payload) VALUES ('user_delete', $1, '{}'::jsonb) \
         RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    Ok((StatusCode::ACCEPTED, Json(DeleteAccountResponse { job_id })))
}
