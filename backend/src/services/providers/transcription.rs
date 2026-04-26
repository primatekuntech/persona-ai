// Provider types are wired at runtime from DB config; suppress dead code lints.
#![allow(dead_code)]
/// Transcription provider trait + implementations.
use super::{Language, LanguageHint, Transcript, TranscriptSegment};
use crate::error::AppError;
use async_trait::async_trait;
use std::path::Path;

/// Transcription provider abstraction.
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Provider identifier (matches `provider_configs.provider`).
    fn name(&self) -> &str;
    /// Whether this provider supports the given language.
    fn supports_language(&self, lang: &Language) -> bool;
    /// Transcribe an audio file, returning a `Transcript`.
    async fn transcribe(&self, audio_path: &Path, hint: LanguageHint) -> Result<Transcript, AppError>;
}

// ─── LocalWhisperProvider ────────────────────────────────────────────────────

/// Wraps the existing `services::transcriber::Transcriber` logic.
pub struct LocalWhisperProvider {
    pub model_path: std::path::PathBuf,
}

impl LocalWhisperProvider {
    pub fn new(model_path: &Path) -> Self {
        Self {
            model_path: model_path.to_owned(),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for LocalWhisperProvider {
    fn name(&self) -> &str {
        "local_whisper"
    }

    fn supports_language(&self, _lang: &Language) -> bool {
        // Whisper large-v3 supports all target languages including Cantonese (yue)
        true
    }

    async fn transcribe(&self, audio_path: &Path, hint: LanguageHint) -> Result<Transcript, AppError> {
        let model_path = self.model_path.clone();
        let audio_path = audio_path.to_owned();

        tokio::task::spawn_blocking(move || {
            let transcriber = crate::services::transcriber::Transcriber::new(&model_path)
                .map_err(AppError::Internal)?;

            // Determine language hint for Whisper
            let lang_code = hint
                .user_override
                .as_ref()
                .or(hint.detected.as_ref())
                .map(|l| l.whisper_code().to_owned());

            // Progress callback is a no-op in the provider abstraction layer;
            // callers that need progress should use the lower-level transcriber directly.
            let (text, duration_sec) = transcriber
                .transcribe_with_language(&audio_path, |_| {}, lang_code.as_deref())
                .map_err(|e| match e {
                    crate::services::transcriber::TranscriberError::AudioTooLong => AppError::AudioTooLong,
                    crate::services::transcriber::TranscriberError::Other(inner) => {
                        AppError::IngestFailed { reason: inner.to_string() }
                    }
                })?;

            Ok(Transcript {
                text,
                detected_language: None, // Whisper detection handled at parse time
                confidence: None,
                segments: vec![TranscriptSegment {
                    start_sec: 0.0,
                    end_sec: duration_sec as f32,
                    text: String::new(), // full text stored separately
                }],
            })
        })
        .await
        .map_err(|e| AppError::IngestFailed {
            reason: format!("spawn_blocking join error: {e}"),
        })?
    }
}

// ─── GoogleSpeechProvider ────────────────────────────────────────────────────

/// Google Cloud Speech-to-Text v2 REST API provider.
pub struct GoogleSpeechProvider {
    pub api_key: String,
    pub region: String,
}

impl GoogleSpeechProvider {
    pub fn new(api_key: String, region: String) -> Self {
        Self { api_key, region }
    }

    fn google_language_code(lang: &Language) -> &'static str {
        match lang {
            Language::English => "en-MY",
            Language::Malay => "ms-MY",
            Language::MandarinSimplified => "cmn-Hans-CN",
            Language::MandarinTraditional => "cmn-Hant-TW",
            Language::Cantonese => "yue-Hant-HK",
            Language::Tamil => "ta-MY",
            Language::Other(_) => "en-MY",
        }
    }
}

#[async_trait]
impl TranscriptionProvider for GoogleSpeechProvider {
    fn name(&self) -> &str {
        "google_speech"
    }

    fn supports_language(&self, _lang: &Language) -> bool {
        // Google Speech supports all target languages including Cantonese
        true
    }

    async fn transcribe(&self, audio_path: &Path, hint: LanguageHint) -> Result<Transcript, AppError> {
        // Determine the language code
        let lang_code = hint
            .user_override
            .as_ref()
            .or(hint.detected.as_ref())
            .map(|l| Self::google_language_code(l))
            .unwrap_or("en-MY");

        // Read audio file
        let audio_bytes = tokio::fs::read(audio_path)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("read audio: {e}")))?;

        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);

        // Build request payload (Google Speech-to-Text v1 REST)
        let payload = serde_json::json!({
            "config": {
                "languageCode": lang_code,
                "enableAutomaticPunctuation": true,
                "model": "latest_long"
            },
            "audio": {
                "content": encoded
            }
        });

        let url = format!(
            "https://speech.googleapis.com/v1/speech:recognize?key={}",
            self.api_key
        );

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Google Speech request: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(anyhow::anyhow!(
                "Google Speech API error {status}: {body}"
            )));
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Google Speech response parse: {e}")))?;

        // Extract transcript from response
        let mut full_text = String::new();
        let mut segments = Vec::new();

        if let Some(results) = result["results"].as_array() {
            for r in results {
                if let Some(alt) = r["alternatives"].get(0) {
                    if let Some(transcript) = alt["transcript"].as_str() {
                        if !full_text.is_empty() {
                            full_text.push(' ');
                        }
                        full_text.push_str(transcript);
                        segments.push(TranscriptSegment {
                            start_sec: 0.0,
                            end_sec: 0.0,
                            text: transcript.to_owned(),
                        });
                    }
                }
            }
        }

        Ok(Transcript {
            text: full_text,
            detected_language: None,
            confidence: None,
            segments,
        })
    }
}
