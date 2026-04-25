pub mod admin;
pub mod auth;
pub mod health;

use crate::state::AppState;
use axum::{routing::{delete, get, patch, post}, Router};
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use std::sync::Arc;

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
        .route("/api/auth/sessions/revoke-all", post(auth::revoke_all_sessions))
        .route("/api/auth/me", get(auth::me));

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
