use super::{lexical::LexicalProfile, syntactic::SyntacticProfile, Chunk};

pub fn pick(
    chunks: &[Chunk],
    lex: &LexicalProfile,
    syn: &SyntacticProfile,
) -> Vec<serde_json::Value> {
    if chunks.is_empty() {
        return vec![];
    }

    let avg_sentence_length = syn.avg_sentence_length.max(1.0);

    // Feature vector for each chunk: [normalized_length, chunk_ttr, comma_density]
    let features: Vec<[f64; 3]> = chunks
        .iter()
        .map(|c| {
            let wc = c.text.split_whitespace().count() as f64;
            let tokens: Vec<String> = c
                .text
                .split_whitespace()
                .map(|w| {
                    w.to_lowercase()
                        .chars()
                        .filter(|ch| ch.is_alphabetic() || *ch == '\'')
                        .collect()
                })
                .filter(|w: &String| !w.is_empty())
                .collect();
            let unique = tokens
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len() as f64;
            let chunk_ttr = if tokens.is_empty() {
                0.0
            } else {
                unique / tokens.len() as f64
            };
            let comma_density = c.text.chars().filter(|&ch| ch == ',').count() as f64 / wc.max(1.0);
            [wc / avg_sentence_length, chunk_ttr, comma_density]
        })
        .collect();

    // Centroid
    let n = features.len() as f64;
    let centroid = [
        features.iter().map(|f| f[0]).sum::<f64>() / n,
        features.iter().map(|f| f[1]).sum::<f64>() / n,
        features.iter().map(|f| f[2]).sum::<f64>() / n,
    ];

    // L2 distance to centroid
    let distances: Vec<f64> = features
        .iter()
        .map(|f| {
            let d0 = f[0] - centroid[0];
            let d1 = f[1] - centroid[1];
            let d2 = f[2] - centroid[2];
            (d0 * d0 + d1 * d1 + d2 * d2).sqrt()
        })
        .collect();

    let max_dist = distances.iter().cloned().fold(0.0f64, f64::max).max(1e-9);

    // Pick 5 closest
    let mut indexed: Vec<(usize, f64)> =
        distances.iter().enumerate().map(|(i, &d)| (i, d)).collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(5);

    // Determine the median for each feature dimension
    let mut feat0: Vec<f64> = features.iter().map(|f| f[0]).collect();
    let mut feat1: Vec<f64> = features.iter().map(|f| f[1]).collect();
    let mut feat2: Vec<f64> = features.iter().map(|f| f[2]).collect();
    feat0.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    feat1.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    feat2.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med0 = median_of(&feat0);
    let med1 = median_of(&feat1);
    let med2 = median_of(&feat2);

    // Median TTR and avg_word_length from lex for reason text
    let corpus_ttr = lex.type_token_ratio;

    indexed
        .iter()
        .map(|(i, dist)| {
            let f = &features[*i];
            // Closest feature to its median determines reason
            let d0 = (f[0] - med0).abs();
            let d1 = (f[1] - med1).abs();
            let d2 = (f[2] - med2).abs();
            let reason = if d0 <= d1 && d0 <= d2 {
                "typical sentence length"
            } else if d1 <= d2 {
                if f[1] > corpus_ttr {
                    "high TTR"
                } else {
                    "typical vocabulary variety"
                }
            } else {
                "typical punctuation rhythm"
            };
            let score = 1.0 - dist / max_dist;
            serde_json::json!({
                "chunk_id": chunks[*i].id,
                "score": (score * 100.0).round() / 100.0,
                "reason": reason,
            })
        })
        .collect()
}

fn median_of(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}
