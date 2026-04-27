/// Shared application state threaded through axum via `State<AppState>`.
use crate::{
    config::AppConfig,
    email::ResendClient,
    services::{broadcast, llm::Llm, providers::ProviderRegistry},
};
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::{atomic::AtomicU8, Arc};
use uuid::Uuid;

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
    /// Server-wide LLM generation semaphore (capacity = config.max_concurrent_generation).
    pub generation_semaphore: Arc<tokio::sync::Semaphore>,
    /// Per-user in-flight generation count (cap 2 per user).
    pub user_generation_counts: Arc<DashMap<Uuid, Arc<AtomicU8>>>,
    /// Local LLM; None if feature absent or model file missing.
    pub llm: Option<Arc<Llm>>,
    /// Provider registry: maps provider names to their implementations.
    /// Used at runtime to resolve provider chains from `provider_configs` table.
    #[allow(dead_code)]
    pub providers: ProviderRegistry,
}
