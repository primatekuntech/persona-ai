/// /healthz and /readyz endpoints.
///
/// /healthz: 200 always if DB is reachable, even if models are missing (degraded).
/// /readyz:  503 until all model integrity checks pass.
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    // Check DB connectivity
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .is_ok();

    let readiness = state.readiness.read().expect("readiness lock");
    let degraded_models: Vec<_> = readiness
        .degraded_models()
        .iter()
        .map(|m| json!({ "name": m.name, "reason": m.reason }))
        .collect();

    if !db_ok {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unhealthy",
                "db": false
            })),
        );
    }

    if degraded_models.is_empty() {
        (
            StatusCode::OK,
            Json(json!({ "status": "ok", "db": true })),
        )
    } else {
        (
            StatusCode::OK,
            Json(json!({
                "status": "degraded",
                "db": true,
                "missing_models": degraded_models
            })),
        )
    }
}

pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let readiness = state.readiness.read().expect("readiness lock");
    if readiness.is_ready() {
        (StatusCode::OK, Json(json!({ "status": "ready" })))
    } else {
        let missing: Vec<_> = readiness
            .degraded_models()
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "not_ready", "missing_models": missing })),
        )
    }
}
