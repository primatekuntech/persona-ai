use super::Chunk;
use std::cmp::Reverse;
use std::collections::HashMap;

static SENTIMENT_LEXICON: std::sync::OnceLock<(Vec<String>, Vec<String>)> =
    std::sync::OnceLock::new();

fn load_sentiment() -> &'static (Vec<String>, Vec<String>) {
    SENTIMENT_LEXICON.get_or_init(|| {
        let raw = include_str!("../../../assets/sentiment_lexicon.json");
        #[derive(serde::Deserialize)]
        struct Lex {
            positive: Vec<String>,
            negative: Vec<String>,
        }
        let lex: Lex = serde_json::from_str(raw).unwrap_or(Lex {
            positive: vec![],
            negative: vec![],
        });
        (lex.positive, lex.negative)
    })
}

pub struct SemanticProfile {
    pub top_topics: Vec<TopicEntry>,
    pub recurring_entities: Vec<EntityEntry>,
    pub sentiment_polarity: f64,
    pub sentiment_subjectivity: f64,
}

pub struct TopicEntry {
    pub label: String,
    pub weight: f64,
    pub keywords: Vec<String>,
}

pub struct EntityEntry {
    pub entity: String,
    pub count: usize,
}

impl SemanticProfile {
    pub fn to_json(&self) -> serde_json::Value {
        let topics: Vec<serde_json::Value> = self
            .top_topics
            .iter()
            .map(|t| {
                serde_json::json!({
                    "label": t.label,
                    "weight": t.weight,
                    "keywords": t.keywords,
                })
            })
            .collect();
        let entities: Vec<serde_json::Value> = self
            .recurring_entities
            .iter()
            .map(|e| {
                serde_json::json!({
                    "entity": e.entity,
                    "count": e.count,
                    "kind": "unknown",
                })
            })
            .collect();
        serde_json::json!({
            "top_topics": topics,
            "recurring_entities": entities,
            "sentiment_baseline": {
                "polarity": self.sentiment_polarity,
                "subjectivity": self.sentiment_subjectivity,
            }
        })
    }
}

/// k-means clustering on f32 vectors with k-means++ initialization.
fn kmeans(embeddings: &[Vec<f32>], k: usize, max_iter: usize) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    let k = k.min(n);
    let dim = embeddings[0].len();

    // k-means++ initialization
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);
    // Pick first centroid randomly (use index 0 for determinism)
    centroids.push(embeddings[0].clone());

    for _ in 1..k {
        // Compute min distance to existing centroids for each point
        let distances: Vec<f32> = embeddings
            .iter()
            .map(|e| {
                centroids
                    .iter()
                    .map(|c| l2_sq(e, c))
                    .fold(f32::MAX, f32::min)
            })
            .collect();
        let total: f32 = distances.iter().sum();
        if total <= 0.0 {
            break;
        }
        // Pick next centroid proportional to distance squared (deterministic: pick max)
        let max_idx = distances
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        centroids.push(embeddings[max_idx].clone());
    }

    let mut assignments = vec![0usize; n];

    for _ in 0..max_iter {
        // Assign
        let mut changed = false;
        for (i, emb) in embeddings.iter().enumerate() {
            let best = centroids
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    l2_sq(emb, a.1)
                        .partial_cmp(&l2_sq(emb, b.1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }

        // Recompute centroids
        let mut new_centroids = vec![vec![0.0f32; dim]; k];
        let mut counts = vec![0usize; k];
        for (i, &cluster) in assignments.iter().enumerate() {
            counts[cluster] += 1;
            for (d, &v) in embeddings[i].iter().enumerate() {
                new_centroids[cluster][d] += v;
            }
        }
        for (c, &cnt) in new_centroids.iter_mut().zip(counts.iter()) {
            if cnt > 0 {
                let cnt_f = cnt as f32;
                for v in c.iter_mut() {
                    *v /= cnt_f;
                }
            }
        }
        centroids = new_centroids;
    }

    assignments
}

fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Compute TF-IDF top words for a set of texts against a corpus.
fn tfidf_top_words(cluster_texts: &[&str], all_texts: &[&str], top_n: usize) -> Vec<String> {
    let n_docs = all_texts.len() as f64;
    if n_docs == 0.0 {
        return vec![];
    }

    // IDF: document frequency across all_texts
    let mut doc_freq: HashMap<&str, usize> = HashMap::new();
    for text in all_texts {
        let words: std::collections::HashSet<&str> = text.split_whitespace().collect();
        for w in words {
            *doc_freq.entry(w).or_insert(0) += 1;
        }
    }

    // TF in cluster
    let mut tf: HashMap<String, usize> = HashMap::new();
    let mut cluster_total = 0usize;
    for text in cluster_texts {
        for w in text.split_whitespace() {
            let lower = w.to_lowercase();
            let clean: String = lower.chars().filter(|c| c.is_alphabetic()).collect();
            if !clean.is_empty() && clean.len() > 2 {
                *tf.entry(clean).or_insert(0) += 1;
                cluster_total += 1;
            }
        }
    }

    if cluster_total == 0 {
        return vec![];
    }

    let mut scores: Vec<(String, f64)> = tf
        .iter()
        .map(|(word, &cnt)| {
            let tf_val = cnt as f64 / cluster_total as f64;
            let df = doc_freq.get(word.as_str()).copied().unwrap_or(0);
            let idf = (n_docs / (df as f64 + 1.0)).ln();
            (word.clone(), tf_val * idf)
        })
        .collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.into_iter().take(top_n).map(|(w, _)| w).collect()
}

pub fn compute(chunks: &[Chunk]) -> SemanticProfile {
    // Topics via k-means on embeddings
    let embedded_chunks: Vec<(usize, &Vec<f32>)> = chunks
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.embedding.as_ref().map(|e| (i, e)))
        .collect();

    let top_topics = if embedded_chunks.len() >= 10 {
        let k = (embedded_chunks.len() / 5).clamp(2, 8);
        let embeddings: Vec<Vec<f32>> = embedded_chunks.iter().map(|(_, e)| (*e).clone()).collect();
        let assignments = kmeans(&embeddings, k, 50);

        let all_texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let mut cluster_indices: HashMap<usize, Vec<usize>> = HashMap::new();
        for (pos, &cluster) in assignments.iter().enumerate() {
            let chunk_idx = embedded_chunks[pos].0;
            cluster_indices.entry(cluster).or_default().push(chunk_idx);
        }

        let total_chunks = embedded_chunks.len() as f64;
        let mut topics: Vec<TopicEntry> = cluster_indices
            .values()
            .map(|indices| {
                let cluster_texts: Vec<&str> =
                    indices.iter().map(|&i| chunks[i].text.as_str()).collect();
                let keywords = tfidf_top_words(&cluster_texts, &all_texts, 5);
                let label = keywords
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" and ");
                let weight = indices.len() as f64 / total_chunks;
                TopicEntry {
                    label,
                    weight,
                    keywords,
                }
            })
            .collect();
        topics.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        topics
    } else {
        vec![]
    };

    // Entities: capitalized bigrams not at sentence start
    let mut entity_counts: HashMap<String, usize> = HashMap::new();
    for chunk in chunks {
        let words: Vec<&str> = chunk.text.split_whitespace().collect();
        for window in words.windows(2) {
            let w1 = window[0];
            let w2 = window[1];
            let w1_clean: String = w1.chars().filter(|c| c.is_alphabetic()).collect();
            let w2_clean: String = w2.chars().filter(|c| c.is_alphabetic()).collect();
            if w1_clean.len() > 1
                && w2_clean.len() > 1
                && w1_clean
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                && w2_clean
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
            {
                let entity = format!("{w1_clean} {w2_clean}");
                *entity_counts.entry(entity).or_insert(0) += 1;
            }
        }
    }
    let mut recurring_entities: Vec<EntityEntry> = entity_counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .map(|(e, c)| EntityEntry {
            entity: e,
            count: c,
        })
        .collect();
    recurring_entities.sort_by_key(|e| Reverse(e.count));
    recurring_entities.truncate(15);

    // Sentiment
    let (pos_words, neg_words) = load_sentiment();
    let pos_set: std::collections::HashSet<&str> = pos_words.iter().map(|s| s.as_str()).collect();
    let neg_set: std::collections::HashSet<&str> = neg_words.iter().map(|s| s.as_str()).collect();

    let (total_pol, total_subj, chunk_count) = chunks.iter().fold(
        (0.0f64, 0.0f64, 0usize),
        |(pol_acc, subj_acc, cnt), chunk| {
            let words: Vec<String> = chunk
                .text
                .split_whitespace()
                .map(|w| {
                    w.to_lowercase()
                        .chars()
                        .filter(|c| c.is_alphabetic())
                        .collect()
                })
                .filter(|w: &String| !w.is_empty())
                .collect();
            let total = words.len() as f64;
            if total == 0.0 {
                return (pol_acc, subj_acc, cnt);
            }
            let pos_count = words
                .iter()
                .filter(|w| pos_set.contains(w.as_str()))
                .count() as f64;
            let neg_count = words
                .iter()
                .filter(|w| neg_set.contains(w.as_str()))
                .count() as f64;
            let polarity = ((pos_count - neg_count) / total).clamp(-1.0, 1.0);
            let subjectivity = ((pos_count + neg_count) / total).clamp(0.0, 1.0);
            (pol_acc + polarity, subj_acc + subjectivity, cnt + 1)
        },
    );

    let (sentiment_polarity, sentiment_subjectivity) = if chunk_count > 0 {
        let n = chunk_count as f64;
        (total_pol / n, total_subj / n)
    } else {
        (0.0, 0.0)
    };

    SemanticProfile {
        top_topics,
        recurring_entities,
        sentiment_polarity,
        sentiment_subjectivity,
    }
}
