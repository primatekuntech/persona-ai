pub mod exemplars;
pub mod lexical;
pub mod semantic;
pub mod stylistic;
pub mod syntactic;

use uuid::Uuid;

pub struct Chunk {
    pub id: Uuid,
    pub text: String,
    pub token_count: i32,
    pub embedding: Option<Vec<f32>>,
}

pub struct ProfileCtx {
    pub document_count: i64,
    pub date_range: Option<(String, String)>,
}

/// Build the full style profile JSON from a set of chunks.
pub fn build_profile(chunks: &[Chunk], ctx: &ProfileCtx) -> serde_json::Value {
    let chunk_count = chunks.len() as i64;
    let word_count: i64 = chunks
        .iter()
        .map(|c| c.text.split_whitespace().count() as i64)
        .sum();
    let corpus_tokens: i32 = chunks.iter().map(|c| c.token_count).sum();

    let corpus = serde_json::json!({
        "document_count": ctx.document_count,
        "chunk_count": chunk_count,
        "word_count": word_count,
        "date_range": ctx.date_range,
    });

    if corpus_tokens < 2000 {
        return serde_json::json!({
            "version": 1,
            "status": "insufficient_corpus",
            "corpus": corpus,
            "message": "Upload at least ~2000 words (roughly 4 pages) to generate a reliable style profile."
        });
    }

    let lex = lexical::compute(chunks);
    let syn = syntactic::compute(chunks);
    let sem = semantic::compute(chunks);
    let sty = stylistic::compute(chunks, &lex, &syn);
    let exem = exemplars::pick(chunks, &lex, &syn);

    serde_json::json!({
        "version": 1,
        "status": "ok",
        "corpus": corpus,
        "lexical": lex.to_json(),
        "syntactic": syn.to_json(),
        "semantic": sem.to_json(),
        "stylistic": sty.to_json(),
        "exemplars": exem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_chunk(text: &str, tokens: i32) -> Chunk {
        Chunk {
            id: Uuid::new_v4(),
            text: text.to_string(),
            token_count: tokens,
            embedding: None,
        }
    }

    #[test]
    fn insufficient_corpus_below_2000_tokens() {
        let chunks = vec![make_chunk("hello world this is a test", 10)];
        let ctx = ProfileCtx {
            document_count: 1,
            date_range: None,
        };
        let profile = build_profile(&chunks, &ctx);
        assert_eq!(profile["status"], "insufficient_corpus");
        assert!(profile["message"].as_str().is_some());
    }

    #[test]
    fn build_profile_returns_ok_status() {
        // Build a synthetic corpus of >2000 tokens by repeating content.
        let text = "The quick brown fox jumps over the lazy dog. \
                    She said that perhaps we could find a way to understand \
                    the deeper meaning behind these words. \
                    I think about this every single day when I wake up. \
                    But somehow things always seem to work out in the end. \
                    Maybe that is just how life goes when you really think about it. ";
        let long_text = text.repeat(20);
        let chunks: Vec<Chunk> = (0..5).map(|_| make_chunk(&long_text, 450)).collect();
        let ctx = ProfileCtx {
            document_count: 2,
            date_range: Some(("2020-01-01".to_string(), "2022-12-31".to_string())),
        };
        let profile = build_profile(&chunks, &ctx);
        assert_eq!(profile["status"], "ok");
        assert_eq!(profile["version"], 1);
        assert!(profile["lexical"].is_object());
        assert!(profile["syntactic"].is_object());
    }
}
