use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

/// Application-wide error type. `IntoResponse` maps each variant to the
/// correct HTTP status and JSON error envelope defined in docs/06-api-conventions.md.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("{code}")]
    Forbidden { code: &'static str },

    #[error("{0}")]
    Validation(String),

    #[error("field validation failed")]
    ValidationFields(HashMap<String, String>),

    #[error("{code}")]
    Conflict { code: &'static str },

    #[error("rate limited")]
    RateLimited { retry_after_secs: u64 },

    #[error("payload too large")]
    PayloadTooLarge,

    #[error("unsupported media type")]
    UnsupportedMediaType,

    #[error("quota exceeded")]
    QuotaExceeded,

    #[error("gone")]
    Gone { code: &'static str },

    /// Returned by transcriber when audio exceeds the duration cap.
    #[allow(dead_code)]
    #[error("audio too long")]
    AudioTooLong,

    #[error("generation concurrency exceeded")]
    GenerationConcurrencyExceeded,

    #[error("server busy")]
    ServerBusy,

    #[error("ingest failed: {reason}")]
    IngestFailed { reason: String },

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let request_id = Uuid::now_v7().to_string();

        let (status, code, message, fields) = match &self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Resource not found.".to_owned(),
                None,
            ),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required.".to_owned(),
                None,
            ),
            AppError::Forbidden { code } => (
                StatusCode::FORBIDDEN,
                *code,
                "Access denied.".to_owned(),
                None,
            ),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, "validation", msg.clone(), None),
            AppError::ValidationFields(f) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_field",
                "Field validation failed.".to_owned(),
                Some(f.clone()),
            ),
            AppError::Conflict { code } => {
                (StatusCode::CONFLICT, *code, "Conflict.".to_owned(), None)
            }
            AppError::RateLimited { retry_after_secs } => {
                let mut resp = (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({
                        "error": {
                            "code": "rate_limited",
                            "message": format!("Too many requests. Retry after {} seconds.", retry_after_secs)
                        },
                        "request_id": request_id
                    })),
                )
                    .into_response();
                resp.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    retry_after_secs
                        .to_string()
                        .parse()
                        .expect("valid header value"),
                );
                return resp;
            }
            AppError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Payload too large.".to_owned(),
                None,
            ),
            AppError::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Unsupported media type.".to_owned(),
                None,
            ),
            AppError::QuotaExceeded => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "quota_exceeded",
                "Quota exceeded.".to_owned(),
                None,
            ),
            AppError::Gone { code } => (
                StatusCode::GONE,
                *code,
                "Resource no longer available.".to_owned(),
                None,
            ),
            AppError::AudioTooLong => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "audio_too_long",
                "Audio file exceeds the duration limit.".to_owned(),
                None,
            ),
            AppError::GenerationConcurrencyExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "generation_concurrency_exceeded",
                "Too many concurrent generations. Try again shortly.".to_owned(),
                None,
            ),
            AppError::ServerBusy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "server_busy",
                "Server is busy. Try again shortly.".to_owned(),
                None,
            ),
            AppError::IngestFailed { reason } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "ingest_failed",
                reason.clone(),
                None,
            ),
            AppError::Database(e) => {
                tracing::error!(error = %e, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "An internal error occurred.".to_owned(),
                    None,
                )
            }
            AppError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "An internal error occurred.".to_owned(),
                    None,
                )
            }
        };

        let mut body = json!({
            "error": {
                "code": code,
                "message": message
            },
            "request_id": request_id
        });

        if let Some(f) = fields {
            body["error"]["fields"] = json!(f);
        }

        (status, Json(body)).into_response()
    }
}

/// Convenience: convert unique constraint violations to the appropriate AppError.
pub fn pg_unique_to_conflict(e: sqlx::Error, code: &'static str) -> AppError {
    match &e {
        sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
            AppError::Conflict { code }
        }
        _ => AppError::Database(e),
    }
}
