// Provider types are wired at runtime from DB config; suppress dead code lints.
#![allow(dead_code)]
/// Embedding provider trait + implementations.
use crate::error::AppError;
use async_trait::async_trait;

/// Embedding provider abstraction.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Provider identifier.
    fn name(&self) -> &str;
    /// Embed a batch of texts. Returns L2-normalised vectors.
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, AppError>;
}

// ─── LocalBgeProvider ────────────────────────────────────────────────────────

/// Wraps the existing `services::embedder::Embedder` logic.
/// Default model upgraded to `bge-m3` (multilingual, 100+ languages).
pub struct LocalBgeProvider {
    pub model_dir: std::path::PathBuf,
}

impl LocalBgeProvider {
    pub fn new(model_dir: &std::path::Path) -> Self {
        Self {
            model_dir: model_dir.to_owned(),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for LocalBgeProvider {
    fn name(&self) -> &str {
        "local_bge"
    }

    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, AppError> {
        let model_dir = self.model_dir.clone();
        tokio::task::spawn_blocking(move || {
            let embedder = crate::services::embedder::Embedder::new(&model_dir)
                .map_err(AppError::Internal)?;
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            embedder.embed(&text_refs).map_err(AppError::Internal)
        })
        .await
        .map_err(|e| AppError::IngestFailed {
            reason: format!("embed spawn_blocking join: {e}"),
        })?
    }
}
