// Many items in this module form a future-facing public API; suppress dead code
// lints until all call sites are wired in.
#![allow(dead_code)]
/// Provider abstraction layer for transcription, LLM, and embeddings.
///
/// Local models are the default; cloud providers can be added via the
/// `provider_configs` table and are tried in priority order (lowest first).
pub mod detect;
pub mod embeddings;
pub mod encrypt;
pub mod llm;
pub mod transcription;

pub use embeddings::EmbeddingProvider;
pub use llm::LlmProvider;
pub use transcription::TranscriptionProvider;

use crate::error::AppError;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

// ─── Language ─────────────────────────────────────────────────────────────────

/// Languages supported by the Malaysian-focused feature set.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Malay,
    MandarinSimplified,
    MandarinTraditional,
    Cantonese,
    Tamil,
    /// Any other language, identified by BCP-47 code (e.g. `"iba"` for Iban).
    Other(String),
}

impl Language {
    /// BCP-47 / Whisper language code.
    pub fn whisper_code(&self) -> &str {
        match self {
            Language::English => "en",
            Language::Malay => "ms",
            Language::MandarinSimplified => "zh",
            Language::MandarinTraditional => "zh",
            Language::Cantonese => "yue",
            Language::Tamil => "ta",
            Language::Other(code) => code.as_str(),
        }
    }

    /// Google Speech BCP-47 code.
    pub fn google_code(&self) -> &str {
        match self {
            Language::English => "en-MY",
            Language::Malay => "ms-MY",
            Language::MandarinSimplified => "cmn-Hans-CN",
            Language::MandarinTraditional => "cmn-Hant-TW",
            Language::Cantonese => "yue-Hant-HK",
            Language::Tamil => "ta-MY",
            Language::Other(code) => code.as_str(),
        }
    }

    /// Storage representation (stored in `documents.detected_language`).
    pub fn as_bcp47(&self) -> &str {
        match self {
            Language::English => "en",
            Language::Malay => "ms",
            Language::MandarinSimplified => "zh-Hans",
            Language::MandarinTraditional => "zh-Hant",
            Language::Cantonese => "yue",
            Language::Tamil => "ta",
            Language::Other(code) => code.as_str(),
        }
    }

    /// Parse from a BCP-47 string stored in the database.
    pub fn from_bcp47(code: &str) -> Language {
        match code {
            "en" => Language::English,
            "ms" => Language::Malay,
            "zh" | "zh-Hans" | "zh-CN" => Language::MandarinSimplified,
            "zh-Hant" | "zh-TW" | "zh-HK" => Language::MandarinTraditional,
            "yue" | "yue-HK" | "yue-Hant-HK" => Language::Cantonese,
            "ta" => Language::Tamil,
            other => Language::Other(other.to_owned()),
        }
    }
}

// ─── LanguageHint ──────────────────────────────────────────────────────────────

/// Language context passed to transcription providers.
#[derive(Default)]
pub struct LanguageHint {
    /// Language detected from prior text analysis (e.g. lingua detection).
    pub detected: Option<Language>,
    /// Explicit language override set by the user.
    pub user_override: Option<Language>,
}

// ─── Transcript ───────────────────────────────────────────────────────────────

pub struct Transcript {
    pub text: String,
    pub detected_language: Option<Language>,
    pub confidence: Option<f32>,
    pub segments: Vec<TranscriptSegment>,
}

pub struct TranscriptSegment {
    pub start_sec: f32,
    pub end_sec: f32,
    pub text: String,
}

// ─── GenerateRequest ──────────────────────────────────────────────────────────

pub struct GenerateRequest {
    pub system: String,
    /// Pairs of (role, content); role is "user" or "assistant".
    pub messages: Vec<(String, String)>,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
}

// ─── ProviderRegistry ─────────────────────────────────────────────────────────

/// Central registry of all available providers for each service type.
/// Populated at startup with local providers; cloud providers are resolved
/// dynamically from the `provider_configs` table at request time.
#[derive(Clone)]
pub struct ProviderRegistry {
    pub transcription: HashMap<String, Arc<dyn TranscriptionProvider>>,
    pub llm: HashMap<String, Arc<dyn LlmProvider>>,
    pub embeddings: HashMap<String, Arc<dyn EmbeddingProvider>>,
}

impl ProviderRegistry {
    /// Build a registry containing only the local providers.
    pub fn new_local(
        whisper_model_path: std::path::PathBuf,
        llm: Option<Arc<crate::services::llm::Llm>>,
        model_dir: std::path::PathBuf,
    ) -> Self {
        let mut transcription: HashMap<String, Arc<dyn TranscriptionProvider>> = HashMap::new();
        transcription.insert(
            "local_whisper".to_owned(),
            Arc::new(transcription::LocalWhisperProvider::new(&whisper_model_path)),
        );

        let mut llm_map: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();
        llm_map.insert(
            "local_llama".to_owned(),
            Arc::new(llm::LocalLlamaProvider::new(llm)),
        );

        let mut embeddings: HashMap<String, Arc<dyn EmbeddingProvider>> = HashMap::new();
        embeddings.insert(
            "local_bge".to_owned(),
            Arc::new(embeddings::LocalBgeProvider::new(&model_dir)),
        );

        Self {
            transcription,
            llm: llm_map,
            embeddings,
        }
    }

    /// Returns enabled transcription providers for a user ordered by priority (lowest first).
    pub async fn transcription_chain(
        &self,
        user_id: Uuid,
        db: &PgPool,
        app_secret: &str,
    ) -> Result<Vec<Arc<dyn TranscriptionProvider>>, AppError> {
        let rows = crate::repositories::provider_configs::list_enabled_for_service(
            db,
            user_id,
            "transcription",
        )
        .await?;

        let mut chain: Vec<Arc<dyn TranscriptionProvider>> = Vec::new();
        for row in rows {
            if let Some(provider) = self.transcription.get(&row.provider) {
                chain.push(provider.clone());
            } else {
                // Cloud provider — instantiate from config
                match row.provider.as_str() {
                    "google_speech" => {
                        if let Some(api_key) = extract_decrypted_key(&row.config, app_secret) {
                            let region = row.config["region"]
                                .as_str()
                                .unwrap_or("global")
                                .to_owned();
                            chain.push(Arc::new(
                                transcription::GoogleSpeechProvider::new(api_key, region),
                            ));
                        }
                    }
                    _ => {
                        tracing::warn!(provider=%row.provider, "unknown transcription provider in config — skipping");
                    }
                }
            }
        }

        // Always fall back to local whisper if chain is empty
        if chain.is_empty() {
            if let Some(p) = self.transcription.get("local_whisper") {
                chain.push(p.clone());
            }
        }

        Ok(chain)
    }

    /// Returns enabled LLM providers for a user ordered by priority (lowest first).
    pub async fn llm_chain(
        &self,
        user_id: Uuid,
        db: &PgPool,
        app_secret: &str,
    ) -> Result<Vec<Arc<dyn LlmProvider>>, AppError> {
        let rows =
            crate::repositories::provider_configs::list_enabled_for_service(db, user_id, "llm")
                .await?;

        let mut chain: Vec<Arc<dyn LlmProvider>> = Vec::new();
        for row in rows {
            if let Some(provider) = self.llm.get(&row.provider) {
                chain.push(provider.clone());
            } else {
                match row.provider.as_str() {
                    "openai_compat" => {
                        if let Some(api_key) = extract_decrypted_key(&row.config, app_secret) {
                            let endpoint = row.config["endpoint"]
                                .as_str()
                                .unwrap_or("https://api.openai.com")
                                .to_owned();
                            let model = row.config["model"]
                                .as_str()
                                .unwrap_or("gpt-4o-mini")
                                .to_owned();
                            chain.push(Arc::new(llm::OpenAICompatProvider::new(
                                endpoint, model, api_key,
                            )));
                        }
                    }
                    _ => {
                        tracing::warn!(provider=%row.provider, "unknown LLM provider in config — skipping");
                    }
                }
            }
        }

        // Always fall back to local LLM
        if chain.is_empty() {
            if let Some(p) = self.llm.get("local_llama") {
                chain.push(p.clone());
            }
        }

        Ok(chain)
    }

    /// Returns enabled embedding providers for a user ordered by priority (lowest first).
    pub async fn embeddings_chain(
        &self,
        user_id: Uuid,
        db: &PgPool,
        _app_secret: &str,
    ) -> Result<Vec<Arc<dyn EmbeddingProvider>>, AppError> {
        let rows = crate::repositories::provider_configs::list_enabled_for_service(
            db,
            user_id,
            "embeddings",
        )
        .await?;

        let mut chain: Vec<Arc<dyn EmbeddingProvider>> = Vec::new();
        for row in rows {
            if let Some(provider) = self.embeddings.get(&row.provider) {
                chain.push(provider.clone());
            }
        }

        // Always fall back to local BGE
        if chain.is_empty() {
            if let Some(p) = self.embeddings.get("local_bge") {
                chain.push(p.clone());
            }
        }

        Ok(chain)
    }
}

/// Helper: extract and decrypt an API key from a provider config JSONB.
/// The key is stored as `{"enc_api_key": "<b64>"}` after the route handler encrypts it.
fn extract_decrypted_key(config: &serde_json::Value, app_secret: &str) -> Option<String> {
    let enc = config["enc_api_key"].as_str()?;
    match encrypt::decrypt_api_key(enc, app_secret) {
        Ok(key) => Some(key),
        Err(e) => {
            tracing::warn!(error=%e, "failed to decrypt provider API key — skipping provider");
            None
        }
    }
}
