use super::Chunk;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::HashMap;

static ENGLISH_FREQ: std::sync::OnceLock<HashMap<String, f64>> = std::sync::OnceLock::new();

fn load_english_freq() -> &'static HashMap<String, f64> {
    ENGLISH_FREQ.get_or_init(|| {
        let raw = include_str!("../../../assets/english_word_freq.json");
        serde_json::from_str::<HashMap<String, f64>>(raw).unwrap_or_default()
    })
}

const STOPLIST_SHORT: &[&str] = &[
    "a", "an", "the", "in", "on", "at", "to", "of", "is", "it", "be", "as", "by", "or", "up", "so",
    "if", "we", "he", "she", "do", "my", "no",
];

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            let lower = word.to_lowercase();
            lower
                .chars()
                .filter(|c| c.is_alphabetic() || *c == '\'')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

pub struct LexicalProfile {
    pub type_token_ratio: f64,
    pub avg_word_length: f64,
    pub vocabulary_level: String,
    pub distinctive_words: Vec<(String, f32)>,
    pub characteristic_bigrams: Vec<(String, usize)>,
    pub characteristic_trigrams: Vec<(String, usize)>,
    pub function_word_profile: HashMap<String, f64>,
    /// fraction of total tokens that are "i"
    pub first_person_rate: f64,
    /// fraction of tokens that are contractions
    pub contractions_rate: f64,
}

impl LexicalProfile {
    pub fn to_json(&self) -> serde_json::Value {
        let distinctive: Vec<serde_json::Value> = self
            .distinctive_words
            .iter()
            .map(|(w, s)| serde_json::json!({"word": w, "score": s}))
            .collect();
        let bigrams: Vec<serde_json::Value> = self
            .characteristic_bigrams
            .iter()
            .map(|(p, c)| serde_json::json!({"phrase": p, "count": c}))
            .collect();
        let trigrams: Vec<serde_json::Value> = self
            .characteristic_trigrams
            .iter()
            .map(|(p, c)| serde_json::json!({"phrase": p, "count": c}))
            .collect();
        serde_json::json!({
            "type_token_ratio": self.type_token_ratio,
            "avg_word_length": self.avg_word_length,
            "vocabulary_level": self.vocabulary_level,
            "distinctive_words": distinctive,
            "characteristic_bigrams": bigrams,
            "characteristic_trigrams": trigrams,
            "function_word_profile": self.function_word_profile,
        })
    }
}

pub fn compute(chunks: &[Chunk]) -> LexicalProfile {
    // Tokenise all chunks in parallel; each chunk returns its token list.
    let chunk_tokens: Vec<Vec<String>> = chunks.par_iter().map(|c| tokenize(&c.text)).collect();

    let all_tokens: Vec<String> = chunk_tokens.into_iter().flatten().collect();
    let total_tokens = all_tokens.len();

    if total_tokens == 0 {
        return LexicalProfile {
            type_token_ratio: 0.0,
            avg_word_length: 0.0,
            vocabulary_level: "basic".into(),
            distinctive_words: vec![],
            characteristic_bigrams: vec![],
            characteristic_trigrams: vec![],
            function_word_profile: HashMap::new(),
            first_person_rate: 0.0,
            contractions_rate: 0.0,
        };
    }

    // TTR — sample 100k tokens if corpus is very large
    let ttr_tokens: &[String] = if total_tokens > 100_000 {
        &all_tokens[..100_000]
    } else {
        &all_tokens
    };
    let unique_count = ttr_tokens
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    let type_token_ratio = unique_count as f64 / ttr_tokens.len() as f64;

    let avg_word_length =
        all_tokens.iter().map(|w| w.chars().count()).sum::<usize>() as f64 / total_tokens as f64;

    let vocabulary_level = if type_token_ratio < 0.05 {
        "basic"
    } else if type_token_ratio < 0.15 {
        "intermediate"
    } else {
        "advanced"
    }
    .to_string();

    // Corpus frequency counts
    let mut freq_map: HashMap<&str, usize> = HashMap::new();
    for tok in &all_tokens {
        *freq_map.entry(tok.as_str()).or_insert(0) += 1;
    }

    // Distinctive words
    let eng_freq = load_english_freq();
    let total_f = total_tokens as f64;
    let mut distinctive_words: Vec<(String, f32)> = freq_map
        .iter()
        .filter(|(_, &cnt)| cnt >= 3)
        .map(|(&word, &cnt)| {
            let corpus_rate = cnt as f64 / total_f;
            let eng_rate = eng_freq.get(word).copied().unwrap_or(0.0) / 1_000_000.0 + 0.0001;
            let score = (corpus_rate / eng_rate) as f32;
            (word.to_string(), score)
        })
        .collect();
    distinctive_words.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // Filter out words that are in the top english freq (the, be, etc.) unless score is very high
    distinctive_words.retain(|(w, _)| {
        let eng_rate = eng_freq.get(w.as_str()).copied().unwrap_or(0.0);
        eng_rate < 5000.0
    });
    distinctive_words.truncate(20);

    // Bigrams
    let bigram_map = ngrams_of(&all_tokens, 2);
    let mut characteristic_bigrams: Vec<(String, usize)> = bigram_map
        .into_iter()
        .filter(|(phrase, cnt)| {
            *cnt >= 3 && {
                let parts: Vec<&str> = phrase.split(' ').collect();
                parts.len() == 2
                    && !(parts[0].len() <= 2 && STOPLIST_SHORT.contains(&parts[0]))
                    && !(parts[1].len() <= 2 && STOPLIST_SHORT.contains(&parts[1]))
            }
        })
        .collect();
    characteristic_bigrams.sort_by_key(|b| Reverse(b.1));
    characteristic_bigrams.truncate(15);

    // Trigrams
    let trigram_map = ngrams_of(&all_tokens, 3);
    let mut characteristic_trigrams: Vec<(String, usize)> = trigram_map
        .into_iter()
        .filter(|(_, cnt)| *cnt >= 3)
        .collect();
    characteristic_trigrams.sort_by_key(|b| Reverse(b.1));
    characteristic_trigrams.truncate(10);

    // Function word profile
    const FUNCTION_WORDS: &[&str] = &[
        "i", "the", "and", "but", "so", "yet", "or", "nor", "for", "a", "an", "it", "this", "that",
        "these", "those", "my", "your", "we", "they",
    ];
    let mut function_word_profile: HashMap<String, f64> = HashMap::new();
    for fw in FUNCTION_WORDS {
        let cnt = freq_map.get(fw).copied().unwrap_or(0);
        function_word_profile.insert(fw.to_string(), cnt as f64 / total_f);
    }

    // First-person rate
    let i_count = freq_map.get("i").copied().unwrap_or(0);
    let first_person_rate = i_count as f64 / total_f;

    // Contractions rate: tokens with apostrophe where second part is common contraction suffix
    const CONTRACTION_SUFFIXES: &[&str] = &["t", "s", "re", "ve", "ll", "d", "m"];
    let contractions_count = all_tokens
        .iter()
        .filter(|tok| {
            if let Some(pos) = tok.find('\'') {
                let suffix = &tok[pos + 1..];
                CONTRACTION_SUFFIXES.contains(&suffix)
            } else {
                false
            }
        })
        .count();
    let contractions_rate = contractions_count as f64 / total_f;

    LexicalProfile {
        type_token_ratio,
        avg_word_length,
        vocabulary_level,
        distinctive_words,
        characteristic_bigrams,
        characteristic_trigrams,
        function_word_profile,
        first_person_rate,
        contractions_rate,
    }
}

fn ngrams_of(tokens: &[String], n: usize) -> HashMap<String, usize> {
    let mut map: HashMap<String, usize> = HashMap::new();
    if tokens.len() < n {
        return map;
    }
    for window in tokens.windows(n) {
        let phrase = window.join(" ");
        *map.entry(phrase).or_insert(0) += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::analysis::Chunk;
    use uuid::Uuid;

    fn chunk(text: &str) -> Chunk {
        Chunk {
            id: Uuid::new_v4(),
            text: text.to_string(),
            token_count: text.split_whitespace().count() as i32,
            embedding: None,
        }
    }

    #[test]
    fn ttr_basic() {
        let chunks = vec![chunk("the cat sat on the mat the cat")];
        let profile = compute(&chunks);
        // 5 unique / 8 total = 0.625
        assert!(profile.type_token_ratio > 0.0);
        assert!(profile.type_token_ratio <= 1.0);
    }

    #[test]
    fn bigrams_counted_correctly() {
        let text = "hello world hello world hello world hello world";
        let chunks = vec![chunk(text)];
        let profile = compute(&chunks);
        let found = profile
            .characteristic_bigrams
            .iter()
            .any(|(p, c)| p == "hello world" && *c >= 3);
        assert!(found, "expected hello world bigram with count >= 3");
    }

    #[test]
    fn vocabulary_level_thresholds() {
        // Very repetitive text → low TTR → basic
        let repetitive = "the the the the the the the the the the ".repeat(50);
        let chunks = vec![chunk(&repetitive)];
        let p = compute(&chunks);
        assert_eq!(p.vocabulary_level, "basic");

        // Diverse text → higher TTR → advanced
        let words: Vec<String> = ('a'..='z')
            .flat_map(|a| ('a'..='z').map(move |b| format!("{a}{b}")))
            .take(300)
            .collect();
        let diverse = words.join(" ");
        let chunks2 = vec![chunk(&diverse)];
        let p2 = compute(&chunks2);
        assert_eq!(p2.vocabulary_level, "advanced");
    }

    #[test]
    fn distinctive_words_not_common_english() {
        let text = "the the the the the the the the the the the the the the".repeat(10);
        let chunks = vec![chunk(&text)];
        let p = compute(&chunks);
        let has_the = p.distinctive_words.iter().any(|(w, _)| w == "the");
        assert!(!has_the, "'the' should not appear in distinctive words");
    }
}
