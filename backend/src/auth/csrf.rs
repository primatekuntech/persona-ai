/// CSRF double-submit cookie middleware.
///
/// - Cookie name: `pai_csrf` (not HttpOnly so JS can read it)
/// - Header name: `x-csrf-token`
/// - Safe methods (GET/HEAD/OPTIONS) and specific exempt paths bypass the check.
/// - Validation uses constant-time comparison to prevent timing attacks.
use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

const CSRF_COOKIE: &str = "pai_csrf";
const CSRF_HEADER: &str = "x-csrf-token";

static CSRF_SKIP_PATHS: &[&str] = &[
    "/api/auth/login",
    "/api/auth/password/forgot",
    "/api/auth/password/reset",
    "/api/invites/accept",
    "/api/invites/validate",
];

pub async fn csrf_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();

    // Skip safe methods
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }

    // Skip exempt paths
    if CSRF_SKIP_PATHS.iter().any(|p| path == *p) {
        return next.run(req).await;
    }

    // Read cookie
    let cookie_val = req
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix(&format!("{CSRF_COOKIE}="))
                    .map(|v| v.to_owned())
            })
        });

    // Read header
    let header_val = req
        .headers()
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    match (cookie_val, header_val) {
        (Some(c), Some(h)) if c.as_bytes().ct_eq(h.as_bytes()).into() => next.run(req).await,
        _ => (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": { "code": "csrf_failed", "message": "CSRF token mismatch." },
                "request_id": Uuid::now_v7().to_string()
            })),
        )
            .into_response(),
    }
}

/// Generate a 32-byte random CSRF token, hex-encoded.
pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}
