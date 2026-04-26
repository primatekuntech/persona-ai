// Provider types are wired at runtime from DB config; suppress dead code lints.
#![allow(dead_code)]
/// LLM provider trait + implementations.
use super::GenerateRequest;
use crate::error::AppError;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

/// LLM provider abstraction.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider identifier.
    fn name(&self) -> &str;
    /// Stream generated tokens.
    async fn generate(
        &self,
        req: GenerateRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError>;
}

// ─── LocalLlamaProvider ──────────────────────────────────────────────────────

/// Wraps the existing `services::llm::Llm` logic.
pub struct LocalLlamaProvider {
    pub llm: Option<std::sync::Arc<crate::services::llm::Llm>>,
}

impl LocalLlamaProvider {
    pub fn new(llm: Option<std::sync::Arc<crate::services::llm::Llm>>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl LlmProvider for LocalLlamaProvider {
    fn name(&self) -> &str {
        "local_llama"
    }

    async fn generate(
        &self,
        req: GenerateRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError> {
        let llm = match &self.llm {
            Some(l) => l.clone(),
            None => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "Local LLM not loaded — model file missing or feature not enabled"
                )));
            }
        };

        let completion_req = crate::services::llm::CompletionRequest {
            system: req.system,
            messages: req
                .messages
                .into_iter()
                .map(|(role, content)| {
                    let r = if role == "assistant" {
                        crate::services::llm::Role::Assistant
                    } else {
                        crate::services::llm::Role::User
                    };
                    (r, content)
                })
                .collect(),
            temperature: req.temperature,
            top_p: req.top_p,
            max_tokens: req.max_tokens,
        };

        let result = tokio::task::spawn_blocking(move || llm.generate(&completion_req, 2))
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking join: {e}")))?
            .map_err(AppError::Internal)?;

        // Convert buffered tokens into a stream
        let tokens = result.tokens;
        let stream = futures::stream::iter(tokens.into_iter().map(Ok::<String, AppError>));
        Ok(Box::pin(stream))
    }
}

// ─── OpenAICompatProvider ────────────────────────────────────────────────────

/// Calls any OpenAI-compatible `/v1/chat/completions` endpoint with SSE streaming.
/// Works with OpenAI, Together AI, Ollama, LM Studio, etc.
pub struct OpenAICompatProvider {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
}

impl OpenAICompatProvider {
    pub fn new(endpoint: String, model: String, api_key: String) -> Self {
        Self {
            endpoint,
            model,
            api_key,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatProvider {
    fn name(&self) -> &str {
        "openai_compat"
    }

    async fn generate(
        &self,
        req: GenerateRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError> {
        let url = format!("{}/v1/chat/completions", self.endpoint.trim_end_matches('/'));

        // Build messages array (system + conversation)
        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": req.system
        })];
        for (role, content) in &req.messages {
            messages.push(serde_json::json!({
                "role": role,
                "content": content
            }));
        }

        let payload = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "top_p": req.top_p
        });

        let client = reqwest::Client::new();
        let mut builder = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload);

        if !self.api_key.is_empty() {
            builder = builder.bearer_auth(&self.api_key);
        }

        let resp = builder.send().await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("OpenAI compat request failed: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(anyhow::anyhow!(
                "OpenAI compat API error {status}: {body}"
            )));
        }

        // Parse the SSE stream
        use futures::StreamExt;
        let byte_stream = resp.bytes_stream();
        let token_stream = byte_stream
            .filter_map(|chunk_result| async move {
                let chunk = chunk_result.ok()?;
                let text = String::from_utf8_lossy(&chunk).into_owned();
                // SSE lines start with "data: "
                let mut tokens = Vec::new();
                for line in text.lines() {
                    let line = line.trim();
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = val["choices"][0]["delta"]["content"].as_str() {
                                if !content.is_empty() {
                                    tokens.push(content.to_owned());
                                }
                            }
                        }
                    }
                }
                if tokens.is_empty() {
                    None
                } else {
                    Some(Ok::<String, AppError>(tokens.join("")))
                }
            });

        Ok(Box::pin(token_stream))
    }
}
