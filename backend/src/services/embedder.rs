/// Embedding service using fastembed (BGE-small-EN-v1.5).
/// Runs inside `tokio::task::spawn_blocking`.
/// Models must be pre-downloaded to `model_dir` (privacy requirement — no network calls).
use std::path::Path;

pub struct Embedder {
    model: fastembed::TextEmbedding,
}

impl Embedder {
    /// Load the embedding model from a local directory.
    /// Expected files in `model_dir/bge-small-en-v1.5/`:
    ///   - model.onnx
    ///   - tokenizer.json
    ///   - config.json
    ///   - special_tokens_map.json
    ///   - tokenizer_config.json
    pub fn new(model_dir: &Path) -> Result<Self, anyhow::Error> {
        // Locate BGE-small-EN-v1.5 files under model_dir
        let bge_dir = model_dir.join("bge-small-en-v1.5");

        let read = |name: &str| -> Result<Vec<u8>, anyhow::Error> {
            let p = bge_dir.join(name);
            std::fs::read(&p).map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))
        };

        let onnx_file = read("model.onnx")?;
        let tokenizer_file = read("tokenizer.json")?;
        let config_file = read("config.json")?;
        let special_tokens_map_file = read("special_tokens_map.json")?;
        let tokenizer_config_file = read("tokenizer_config.json")?;

        let model_def = fastembed::UserDefinedEmbeddingModel {
            onnx_file,
            tokenizer_files: fastembed::TokenizerFiles {
                tokenizer_file,
                config_file,
                special_tokens_map_file,
                tokenizer_config_file,
            },
        };

        let options = fastembed::InitOptionsUserDefined::default();
        let model = fastembed::TextEmbedding::try_new_from_user_defined(model_def, options)
            .map_err(|e| anyhow::anyhow!("fastembed init: {e}"))?;

        Ok(Self { model })
    }

    /// Embed a batch of texts. Returns L2-normalised 384-dim vectors.
    /// Batch size of 16 keeps memory usage below ~300 MB.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, anyhow::Error> {
        let mut embeddings = self
            .model
            .embed(texts.to_vec(), Some(16))
            .map_err(|e| anyhow::anyhow!("fastembed embed: {e}"))?;

        // L2-normalise so cosine similarity == dot product
        for emb in &mut embeddings {
            let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in emb.iter_mut() {
                    *x /= norm;
                }
            }
        }

        Ok(embeddings)
    }
}
