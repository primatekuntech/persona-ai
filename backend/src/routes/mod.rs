pub mod admin;
pub mod auth;
pub mod documents;
pub mod health;
pub mod personas;

use crate::state::AppState;
use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post},
    Router,
};
use std::sync::Arc;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

pub fn build_router(state: AppState) -> Router {
    // Strict rate limit for unauthenticated auth endpoints (10 req / 60 s per IP, burst 10).
    let auth_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_millisecond(6000) // 1 req per 6 s = 10 per minute
            .burst_size(10)
            .finish()
            .expect("valid governor config"),
    );

    // Lighter rate limit for authenticated admin write actions (30 req / 60 s per IP, burst 5).
    // Still required by sprint spec §1.11 to prevent invite spam.
    let admin_write_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_millisecond(2000) // 1 req per 2 s = 30 per minute
            .burst_size(5)
            .finish()
            .expect("valid admin governor config"),
    );

    let auth_routes = Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/password/forgot", post(auth::forgot_password))
        .route("/api/auth/password/reset", post(auth::reset_password))
        .route("/api/invites/accept", post(auth::accept_invite))
        .route("/api/invites/validate", get(auth::validate_invite))
        .layer(GovernorLayer {
            config: auth_governor,
        });

    let protected_routes = Router::new()
        .route("/api/auth/logout", post(auth::logout))
        .route(
            "/api/auth/sessions/revoke-all",
            post(auth::revoke_all_sessions),
        )
        .route("/api/auth/me", get(auth::me))
        // Persona CRUD
        .route("/api/personas", get(personas::list_personas))
        .route("/api/personas", post(personas::create_persona))
        .route("/api/personas/:id", get(personas::get_persona))
        .route("/api/personas/:id", patch(personas::patch_persona))
        .route("/api/personas/:id", delete(personas::delete_persona))
        // Avatar — raise body limit so our in-handler 2 MB check fires with the correct error envelope.
        .route(
            "/api/personas/:id/avatar",
            post(personas::upload_avatar).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route("/api/personas/:id/avatar", get(personas::get_avatar))
        .route("/api/personas/:id/avatar", delete(personas::delete_avatar))
        // Eras
        .route("/api/personas/:id/eras", get(personas::list_eras))
        .route("/api/personas/:id/eras", post(personas::create_era))
        .route("/api/personas/:id/eras/:era_id", patch(personas::patch_era))
        .route(
            "/api/personas/:id/eras/:era_id",
            delete(personas::delete_era),
        )
        // Documents — list + SSE (no body limit adjustment needed for these)
        .route(
            "/api/personas/:id/documents",
            get(documents::list_documents),
        )
        .route(
            "/api/personas/:id/documents/events",
            get(documents::document_events),
        )
        .route(
            "/api/personas/:id/documents/:doc_id",
            get(documents::get_document),
        )
        .route(
            "/api/personas/:id/documents/:doc_id",
            delete(documents::delete_document),
        )
        .route(
            "/api/personas/:id/documents/:doc_id/reingest",
            post(documents::reingest_document),
        )
        .route(
            "/api/personas/:id/documents/:doc_id/transcript",
            get(documents::get_transcript),
        )
        // Document upload — 512 MB body limit for audio
        .route(
            "/api/personas/:id/documents",
            post(documents::upload_document).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        );

    // POST /api/admin/invites is rate-limited per sprint spec §1.11 to prevent spam.
    let admin_write_routes = Router::new()
        .route("/api/admin/invites", post(admin::create_invite))
        .layer(GovernorLayer {
            config: admin_write_governor,
        });

    let admin_routes = Router::new()
        .route("/api/admin/invites", get(admin::list_invites))
        .route("/api/admin/invites/:id", delete(admin::revoke_invite))
        .route("/api/admin/users", get(admin::list_users))
        .route("/api/admin/users/:id", patch(admin::patch_user))
        .route("/api/admin/users/:id/reset", post(admin::admin_reset_user));

    let health_routes = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz));

    Router::new()
        .merge(auth_routes)
        .merge(protected_routes)
        .merge(admin_write_routes)
        .merge(admin_routes)
        .merge(health_routes)
        .with_state(state)
}
