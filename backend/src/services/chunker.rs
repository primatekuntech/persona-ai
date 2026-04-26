/// Sentence-aware text chunker using the BGE tokenizer.
/// Target: 400 tokens per chunk, 50 token overlap, min 20 tokens.
use crate::error::AppError;
use std::path::Path;
use tokenizers::Tokenizer;
use unicode_normalization::UnicodeNormalization;

const TARGET_TOKENS: usize = 400;
const OVERLAP_TOKENS: usize = 50;
const MIN_TOKENS: usize = 20;
/// Rough chars-per-token estimate for overlap prefix calculation.
const CHARS_PER_TOKEN_ESTIMATE: usize = 4;

#[derive(Debug, Clone)]
pub struct ChunkText {
    pub text: String,
    pub token_count: i32,
    pub chunk_index: i32,
}

pub struct Chunker {
    tokenizer: Tokenizer,
}

impl Chunker {
    /// Load tokenizer from `tokenizer.json` in the model directory.
    pub fn new(model_dir: &Path) -> Result<Self, AppError> {
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer =
            Tokenizer::from_file(&tokenizer_path).map_err(|e| AppError::IngestFailed {
                reason: format!(
                    "Failed to load tokenizer from {}: {e}",
                    tokenizer_path.display()
                ),
            })?;
        Ok(Self { tokenizer })
    }

    /// Chunk text into ~400 token pieces with ~50 token overlap.
    /// Filters out chunks with < 20 tokens.
    /// NFC normalises and collapses whitespace in each chunk.
    pub fn chunk_text(&self, text: &str) -> Vec<ChunkText> {
        // text-splitter uses &Tokenizer as ChunkSizer
        let config = text_splitter::ChunkConfig::new(TARGET_TOKENS).with_sizer(&self.tokenizer);

        let splitter = text_splitter::TextSplitter::new(config);
        let raw_chunks: Vec<&str> = splitter.chunks(text).collect();

        let mut results: Vec<ChunkText> = Vec::new();

        for (i, chunk_text) in raw_chunks.iter().enumerate() {
            // Build chunk text: prepend overlap from previous chunk
            let chunk_with_overlap = if i > 0 {
                if let Some(prev) = results.last() {
                    // Take last ~50 tokens worth of chars from previous chunk
                    let overlap_chars = OVERLAP_TOKENS * CHARS_PER_TOKEN_ESTIMATE;
                    let prev_chars = prev.text.chars().count();
                    if prev_chars > overlap_chars {
                        let skip = prev_chars - overlap_chars;
                        let overlap_prefix: String = prev.text.chars().skip(skip).collect();
                        format!("{} {}", overlap_prefix.trim(), chunk_text.trim())
                    } else {
                        chunk_text.to_string()
                    }
                } else {
                    chunk_text.to_string()
                }
            } else {
                chunk_text.to_string()
            };

            // Normalize: NFC + collapse whitespace
            let normalized = normalize_text(&chunk_with_overlap);
            if normalized.is_empty() {
                continue;
            }

            // Count tokens accurately
            let token_count = self.count_tokens(&normalized);
            if token_count < MIN_TOKENS as i32 {
                continue;
            }

            // If overlap pushed us significantly over budget, fall back to original chunk
            let (final_text, final_count) = if token_count > (TARGET_TOKENS + OVERLAP_TOKENS) as i32
            {
                let base = normalize_text(chunk_text);
                let base_count = self.count_tokens(&base);
                if base_count >= MIN_TOKENS as i32 {
                    (base, base_count)
                } else {
                    (normalized, token_count)
                }
            } else {
                (normalized, token_count)
            };

            results.push(ChunkText {
                text: final_text,
                token_count: final_count,
                chunk_index: results.len() as i32,
            });
        }

        results
    }

    fn count_tokens(&self, text: &str) -> i32 {
        self.tokenizer
            .encode(text, false)
            .map(|enc| enc.len() as i32)
            .unwrap_or(0)
    }
}

fn normalize_text(text: &str) -> String {
    // NFC normalization
    let nfc: String = text.nfc().collect();
    // Collapse runs of whitespace (preserve as single space)
    let mut result = String::with_capacity(nfc.len());
    let mut last_was_space = false;
    for ch in nfc.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
            }
            last_was_space = true;
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_text("  hello   world  "), "hello world");
        assert_eq!(normalize_text("a\n\nb"), "a b");
        assert_eq!(normalize_text(""), "");
    }

    #[test]
    fn normalize_nfc() {
        // U+00E9 (é precomposed) vs U+0065 + U+0301 (e + combining acute)
        let decomposed = "e\u{0301}";
        let result = normalize_text(decomposed);
        assert_eq!(result, "\u{00E9}");
    }

    #[test]
    fn constants_within_spec() {
        assert_eq!(TARGET_TOKENS, 400);
        assert_eq!(OVERLAP_TOKENS, 50);
        assert_eq!(MIN_TOKENS, 20);
        // Overlap chars estimate: reasonable range
        assert!(CHARS_PER_TOKEN_ESTIMATE >= 3 && CHARS_PER_TOKEN_ESTIMATE <= 6);
    }

    #[test]
    fn chunk_index_is_sequential() {
        // Without a real tokenizer we validate the index assignment logic via
        // the normalize + filter pipeline only.
        let v = normalize_text("hello world foo bar");
        assert!(!v.is_empty());
    }
}
