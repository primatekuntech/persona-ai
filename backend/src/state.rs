/// Shared application state threaded through axum via `State<AppState>`.
use crate::{config::AppConfig, email::ResendClient, services::broadcast};
use sqlx::PgPool;
use std::sync::Arc;

/// Model integrity status populated at startup.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ModelStatus {
    pub name: String,
    pub path: String,
    pub ok: bool,
    pub reason: Option<String>,
}

/// Readiness state: degraded until all model files verify.
#[derive(Debug, Default)]
pub struct ReadinessState {
    pub models: Vec<ModelStatus>,
}

impl ReadinessState {
    pub fn is_ready(&self) -> bool {
        self.models.iter().all(|m| m.ok)
    }

    pub fn degraded_models(&self) -> Vec<&ModelStatus> {
        self.models.iter().filter(|m| !m.ok).collect()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<AppConfig>,
    pub email: Arc<ResendClient>,
    pub readiness: Arc<std::sync::RwLock<ReadinessState>>,
    /// Broadcast channel for document ingest progress events (SSE).
    /// `broadcast::Sender` is already `Clone`, so no Arc needed.
    pub ingest_tx: broadcast::Sender,
}
