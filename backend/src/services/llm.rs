/// LLM wrapper (llama-cpp-2). Gated behind `--features llm`.
/// Without the feature, `Llm::new` always fails and `AppState.llm` is None.
use std::path::Path;

pub struct GenerationResult {
    pub tokens: Vec<String>,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub finish_reason: String,
}

#[allow(dead_code)] // fields used when `llm` feature is enabled
pub struct CompletionRequest {
    pub system: String,
    pub messages: Vec<(Role, String)>,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
}

#[derive(Clone)]
pub enum Role {
    User,
    Assistant,
}

pub struct Llm {
    #[allow(dead_code)] // used when `llm` feature is enabled
    model_path: std::path::PathBuf,
}

impl Llm {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        #[cfg(not(feature = "llm"))]
        {
            let _ = path;
            anyhow::bail!(
                "LLM not compiled. Build with --features llm and ensure the GGUF model file exists."
            );
        }
        #[cfg(feature = "llm")]
        {
            if !path.exists() {
                anyhow::bail!("LLM model not found at {}", path.display());
            }
            // TODO: load model with llama-cpp-2 here
            Ok(Self {
                model_path: path.to_owned(),
            })
        }
    }

    /// Generate a buffered response. Retries up to `max_retries` times if AI leakage detected.
    pub fn generate(
        &self,
        req: &CompletionRequest,
        max_retries: u8,
    ) -> anyhow::Result<GenerationResult> {
        #[cfg(not(feature = "llm"))]
        {
            let _ = (req, max_retries);
            anyhow::bail!("LLM not compiled");
        }
        #[cfg(feature = "llm")]
        {
            let _ = max_retries; // TODO: implement retry loop
                                 // TODO: implement with llama-cpp-2 API
                                 // The full implementation loads context, applies chat template,
                                 // generates tokens, and collects them into a Vec<String>.
            let _ = &self.model_path;
            anyhow::bail!("LLM generation not yet implemented (stub — add llama-cpp-2 API calls)");
        }
    }
}
