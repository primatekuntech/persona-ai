use crate::{error::AppError, services::embedder::Embedder};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub id: Uuid,
    pub text: String,
    #[allow(dead_code)] // metadata available for future citation display
    pub document_id: Uuid,
    #[allow(dead_code)] // metadata available for future citation display
    pub doc_title: Option<String>,
}

pub struct RetrievalQuery<'a> {
    pub user_id: Uuid,
    pub persona_id: Uuid,
    pub era_id: Option<Uuid>,
    pub query_text: &'a str,
    pub k: usize,
}

/// Extract keywords by casefold + remove stop-words.
fn extract_keywords(text: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "the", "in", "on", "at", "to", "of", "is", "it", "be", "as", "by", "or", "and",
        "but", "so", "for", "with", "from", "that", "this", "was", "are", "have", "has", "had",
        "not", "do", "did", "can", "will", "would", "could", "should", "i", "me", "my", "we",
        "you", "your", "he", "she", "they", "their",
    ];
    text.split_whitespace()
        .filter_map(|w| {
            let lower = w
                .chars()
                .filter(|c| c.is_alphabetic())
                .collect::<String>()
                .to_lowercase();
            if lower.len() >= 3 && !STOPWORDS.contains(&lower.as_str()) {
                Some(lower)
            } else {
                None
            }
        })
        .take(5)
        .collect()
}

/// Hybrid retrieval with Reciprocal Rank Fusion.
pub async fn retrieve(
    pool: &PgPool,
    embedder: &Embedder,
    q: &RetrievalQuery<'_>,
) -> Result<Vec<RetrievedChunk>, AppError> {
    // 1. Embed the query
    let embeddings = embedder
        .embed(&[q.query_text])
        .map_err(|e| AppError::Internal(anyhow::anyhow!("embed error: {e}")))?;
    let vec = pgvector::Vector::from(embeddings[0].clone());

    type ChunkRow = (Uuid, String, Uuid, Option<String>);

    // 2. Vector search — top 20 by cosine distance
    let vector_rows: Vec<ChunkRow> = sqlx::query_as(
        "SELECT c.id, c.text, c.document_id, d.title
         FROM chunks c
         LEFT JOIN documents d ON d.id = c.document_id
         WHERE c.user_id = $1 AND c.persona_id = $2
           AND ($3::uuid IS NULL OR c.era_id = $3)
           AND c.embedding IS NOT NULL
         ORDER BY c.embedding <=> $4
         LIMIT 20",
    )
    .bind(q.user_id)
    .bind(q.persona_id)
    .bind(q.era_id)
    .bind(&vec)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // 3. Trigram / keyword search — top 20
    let keywords = extract_keywords(q.query_text);
    let trigram_rows: Vec<ChunkRow> = if keywords.is_empty() {
        vec![]
    } else {
        let patterns: Vec<String> = keywords.iter().map(|k| format!("%{k}%")).collect();
        let patterns_arr: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
        sqlx::query_as(
            "SELECT c.id, c.text, c.document_id, d.title
             FROM chunks c
             LEFT JOIN documents d ON d.id = c.document_id
             WHERE c.user_id = $1 AND c.persona_id = $2
               AND ($3::uuid IS NULL OR c.era_id = $3)
               AND c.text ILIKE ANY ($4)
             ORDER BY similarity(c.text, $5) DESC
             LIMIT 20",
        )
        .bind(q.user_id)
        .bind(q.persona_id)
        .bind(q.era_id)
        .bind(&patterns_arr[..])
        .bind(q.query_text)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?
    };

    // 4. RRF: score(c) = sum 1/(60 + rank_i)
    let k_rrf: f64 = 60.0;
    let mut scores: std::collections::HashMap<Uuid, (f64, String, Uuid, Option<String>)> =
        std::collections::HashMap::new();

    for (rank, (id, text, doc_id, title)) in vector_rows.iter().enumerate() {
        let entry = scores
            .entry(*id)
            .or_insert((0.0, text.clone(), *doc_id, title.clone()));
        entry.0 += 1.0 / (k_rrf + rank as f64 + 1.0);
    }
    for (rank, (id, text, doc_id, title)) in trigram_rows.iter().enumerate() {
        let entry = scores
            .entry(*id)
            .or_insert((0.0, text.clone(), *doc_id, title.clone()));
        entry.0 += 1.0 / (k_rrf + rank as f64 + 1.0);
    }

    // 5. Sort by fused score and return top-k
    let mut ranked: Vec<(Uuid, f64, String, Uuid, Option<String>)> = scores
        .into_iter()
        .map(|(id, (score, text, doc_id, title))| (id, score, text, doc_id, title))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(q.k);

    Ok(ranked
        .into_iter()
        .map(|(id, _, text, document_id, doc_title)| RetrievedChunk {
            id,
            text,
            document_id,
            doc_title,
        })
        .collect())
}
