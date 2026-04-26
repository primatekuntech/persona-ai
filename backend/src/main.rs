mod audit;
mod auth;
mod config;
mod db;
mod email;
mod error;
mod repositories;
mod routes;
mod services;
mod state;

// Parser sandboxing: cap virtual memory at 512 MB for lopdf/docx-rs safety.
// This is a process-wide limit shared across all ingest jobs.
// On non-Linux platforms this block compiles away.
#[cfg(target_os = "linux")]
fn set_rlimit_as() {
    let limit = libc::rlimit {
        rlim_cur: 512 * 1024 * 1024,
        rlim_max: 512 * 1024 * 1024,
    };
    // SAFETY: setrlimit is always safe to call; the worst case is EINVAL which we ignore.
    let _ = unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) };
}

#[cfg(not(target_os = "linux"))]
fn set_rlimit_as() {}

use auth::password::hash as hash_password;
use config::AppConfig;
use db::connect_and_migrate;
use email::ResendClient;
use repositories::users::{admin_exists, create as create_user};
use routes::build_router;
use services::model_check::run_integrity_checks;
use state::{AppState, ReadinessState};
use std::sync::{Arc, RwLock};
use time::Duration;
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::PostgresStore;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ──────────────────────────────────────────────────────────────
    let is_production = std::env::var("RUST_ENV").as_deref() == Ok("production");
    if is_production {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,persona_ai=info,sqlx=warn".parse().unwrap()),
            )
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,persona_ai=debug,sqlx=warn".parse().unwrap()),
            )
            .pretty()
            .init();
    }

    // ── Config ───────────────────────────────────────────────────────────────
    let config = AppConfig::load().map_err(|e| anyhow::anyhow!("config error: {e}"))?;
    tracing::info!(bind = %config.bind_addr, "persona-ai starting");

    // ── Database + migrations ────────────────────────────────────────────────
    let pool = connect_and_migrate(&config.database_url).await?;
    tracing::info!("database connected and migrations applied");

    // ── Sessions ─────────────────────────────────────────────────────────────
    let session_store = PostgresStore::new(pool.clone());
    session_store.migrate().await?;

    let _secret = hex::decode(&config.session_secret)
        .map_err(|e| anyhow::anyhow!("SESSION_SECRET must be 64 hex chars: {e}"))?;

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(is_production)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_http_only(true)
        .with_name("pai_session")
        .with_expiry(Expiry::OnInactivity(Duration::hours(
            config.session_ttl_hours as i64,
        )));

    // ── Admin bootstrap ──────────────────────────────────────────────────────
    if let Some(ref email) = config.admin_bootstrap_email {
        if !admin_exists(&pool).await? {
            if let Some(ref pw) = config.admin_bootstrap_password {
                let pw_hash = hash_password(pw)?;
                let user = create_user(&pool, email, &pw_hash, "admin", None).await?;
                tracing::info!(email = %email, user_id = %user.id, "bootstrap admin created: email={}", email);
                audit::log(
                    &pool,
                    Some(user.id),
                    "admin.bootstrapped",
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .ok();
            } else {
                tracing::warn!(
                    "ADMIN_BOOTSTRAP_EMAIL set but ADMIN_BOOTSTRAP_PASSWORD missing; skipping"
                );
            }
        }
    }

    // ── Model integrity check ────────────────────────────────────────────────
    let model_statuses = run_integrity_checks(config.model_dir.clone()).await;
    for m in model_statuses.iter().filter(|m| !m.ok) {
        tracing::warn!(
            model = %m.name,
            reason = ?m.reason,
            "model integrity check failed — running in degraded mode"
        );
    }

    let readiness = Arc::new(RwLock::new(ReadinessState {
        models: model_statuses,
    }));

    // ── RLIMIT_AS (parser sandboxing) ────────────────────────────────────────
    set_rlimit_as();

    // ── App state ────────────────────────────────────────────────────────────
    let email_client = ResendClient::new(&config.resend_api_key, &config.resend_from);
    let ingest_tx = services::broadcast::new_channel();
    let state = AppState {
        db: pool,
        config: Arc::new(config.clone()),
        email: Arc::new(email_client),
        readiness,
        ingest_tx,
    };

    // ── Background workers ───────────────────────────────────────────────────
    services::worker::start_workers(state.clone(), config.worker_threads);

    // ── Router ───────────────────────────────────────────────────────────────
    use axum::http::HeaderValue;

    let mut app = build_router(state)
        .layer(session_layer)
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("same-origin"),
        ));

    if config.dev_cors {
        tracing::warn!("DEV CORS enabled — do not use in production");
        use tower_http::cors::{Any, CorsLayer};
        app = app.layer(
            CorsLayer::new()
                .allow_origin(
                    config
                        .app_base_url
                        .parse::<HeaderValue>()
                        .unwrap_or(HeaderValue::from_static("http://localhost:5173")),
                )
                .allow_credentials(true)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }

    // ── Serve ────────────────────────────────────────────────────────────────
    // SECURITY: The application trusts X-Forwarded-For / X-Real-IP for client IP
    // attribution (audit logs, brute-force records). In production this REQUIRES a
    // reverse proxy (Caddy/nginx) that strips or overwrites these headers before
    // forwarding — otherwise an attacker can spoof their IP.
    if !is_production {
        tracing::debug!(
            "dev mode: X-Forwarded-For is trusted; ensure a reverse proxy in production"
        );
    }
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}
