pub mod admin;
pub mod auth;
pub mod chat;
pub mod data_rights;
pub mod documents;
pub mod export;
pub mod health;
pub mod personas;
pub mod profile;
pub mod providers;

use crate::state::AppState;
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
use chat as chat_handlers;
use profile as profile_handlers;
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

    // Upload rate limit: 60 req / 60 min per IP (spec §06 conventions).
    let upload_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_millisecond(60_000) // 1 req per minute sustained
            .burst_size(60) // burst of 60 allowed
            .finish()
            .expect("valid upload governor config"),
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
        // Profile
        .route(
            "/api/personas/:id/profile",
            get(profile_handlers::get_persona_profile),
        )
        .route(
            "/api/personas/:id/profile/recompute",
            post(profile_handlers::recompute_profile),
        )
        .route(
            "/api/personas/:id/eras/:era_id/profile",
            get(profile_handlers::get_era_profile),
        )
        // Chat sessions
        .route(
            "/api/personas/:id/chats",
            post(chat_handlers::create_session),
        )
        .route("/api/personas/:id/chats", get(chat_handlers::list_sessions))
        .route("/api/chats/:session_id", get(chat_handlers::get_session))
        .route(
            "/api/chats/:session_id",
            delete(chat_handlers::delete_session),
        )
        // Post message (SSE) — 32 KB body limit
        .route(
            "/api/chats/:session_id/messages",
            post(chat_handlers::post_message).layer(DefaultBodyLimit::max(32 * 1024)),
        )
        // Export
        .route("/api/chats/:session_id/export", get(export::export_session))
        // Data rights
        .route("/api/auth/export", post(data_rights::request_export))
        .route(
            "/api/auth/export/:job_id",
            get(data_rights::get_export_status),
        )
        .route(
            "/api/auth/export/:job_id/download",
            get(data_rights::download_export),
        )
        .route("/api/auth/delete", post(data_rights::delete_account))
        // Provider configs
        .route("/api/providers", get(providers::list_providers))
        .route("/api/providers", post(providers::create_provider))
        .route("/api/providers/:id", patch(providers::patch_provider))
        .route("/api/providers/:id", delete(providers::delete_provider))
        .route("/api/providers/:id/test", post(providers::test_provider))
        .layer(middleware::from_fn(crate::auth::csrf::csrf_middleware));

    // Document upload — rate-limited (60/60min) + 512 MB body limit for audio
    let upload_route = Router::new()
        .route(
            "/api/personas/:id/documents",
            post(documents::upload_document).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .layer(GovernorLayer {
            config: upload_governor,
        });

    // POST /api/admin/invites is rate-limited per sprint spec §1.11 to prevent spam.
    let admin_write_routes = Router::new()
        .route("/api/admin/invites", post(admin::create_invite))
        .layer(middleware::from_fn(crate::auth::csrf::csrf_middleware))
        .layer(GovernorLayer {
            config: admin_write_governor,
        });

    let admin_routes = Router::new()
        .route("/api/admin/invites", get(admin::list_invites))
        .route("/api/admin/invites/:id", delete(admin::revoke_invite))
        .route("/api/admin/users", get(admin::list_users))
        .route("/api/admin/users/:id", patch(admin::patch_user))
        .route("/api/admin/users/:id/reset", post(admin::admin_reset_user))
        .route("/api/admin/jobs", get(admin::list_jobs))
        .route("/api/admin/jobs/:id/retry", post(admin::retry_job))
        .route("/api/admin/jobs/:id", delete(admin::cancel_job))
        .route("/api/admin/errors", get(admin::list_errors))
        .route("/api/admin/audit", get(admin::list_audit))
        .layer(middleware::from_fn(crate::auth::csrf::csrf_middleware));

    let health_routes = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz));

    Router::new()
        .merge(auth_routes)
        .merge(protected_routes)
        .merge(upload_route)
        .merge(admin_write_routes)
        .merge(admin_routes)
        .merge(health_routes)
        .with_state(state)
}
