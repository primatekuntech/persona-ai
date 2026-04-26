use super::{lexical::LexicalProfile, syntactic::SyntacticProfile, Chunk};
use std::cmp::Reverse;
use std::collections::HashMap;

pub struct StylisticProfile {
    pub opening_gambits: Vec<(String, usize)>,
    pub sign_offs: Vec<(String, usize)>,
    pub register: String,
    pub first_person_rate: f64,
    pub contractions_rate: f64,
}

impl StylisticProfile {
    pub fn to_json(&self) -> serde_json::Value {
        let gambits: Vec<&str> = self
            .opening_gambits
            .iter()
            .map(|(p, _)| p.as_str())
            .collect();
        let sign_offs: Vec<&str> = self.sign_offs.iter().map(|(p, _)| p.as_str()).collect();
        serde_json::json!({
            "opening_gambits": gambits,
            "sign_offs": sign_offs,
            "recurring_metaphors": [],
            "register": self.register,
            "first_person_rate": self.first_person_rate,
            "contractions_rate": self.contractions_rate,
        })
    }
}

fn first_sentence(text: &str) -> Option<&str> {
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            return Some(&text[..=i]);
        }
    }
    // No terminator found; return whole text
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn last_sentence(text: &str) -> Option<&str> {
    let trimmed = text.trim_end_matches(['.', '!', '?', ' ', '\n']);
    let mut last_end = trimmed.len();
    for (i, c) in trimmed.char_indices().rev() {
        if matches!(c, '.' | '!' | '?') {
            return Some(trimmed[i + 1..last_end].trim());
        }
        last_end = i;
    }
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn phrase_words(sentence: &str, n_min: usize, n_max: usize) -> Vec<String> {
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| {
            w.to_lowercase()
                .chars()
                .filter(|c| c.is_alphabetic() || *c == '\'')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();

    if words.len() < n_min {
        return vec![];
    }

    let take = words.len().min(n_max);
    words[..take].to_vec()
}

pub fn compute(chunks: &[Chunk], lex: &LexicalProfile, syn: &SyntacticProfile) -> StylisticProfile {
    let mut opening_counts: HashMap<String, usize> = HashMap::new();
    let mut signoff_counts: HashMap<String, usize> = HashMap::new();

    for chunk in chunks {
        let text = chunk.text.trim();
        if text.is_empty() {
            continue;
        }

        if let Some(first) = first_sentence(text) {
            let words = phrase_words(first, 3, 5);
            if !words.is_empty() {
                let phrase = words.join(" ");
                *opening_counts.entry(phrase).or_insert(0) += 1;
            }
        }

        if let Some(last) = last_sentence(text) {
            let words = phrase_words(last, 3, 5);
            if !words.is_empty() {
                let phrase = words.join(" ");
                *signoff_counts.entry(phrase).or_insert(0) += 1;
            }
        }
    }

    let mut opening_gambits: Vec<(String, usize)> = opening_counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .collect();
    opening_gambits.sort_by_key(|b| Reverse(b.1));
    opening_gambits.truncate(5);

    let mut sign_offs: Vec<(String, usize)> = signoff_counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .collect();
    sign_offs.sort_by_key(|b| Reverse(b.1));
    sign_offs.truncate(5);

    // Register classification
    let contractions_rate = lex.contractions_rate;
    let first_person_rate = lex.first_person_rate;
    let avg_sentence_length = syn.avg_sentence_length;
    let interrogative_rate = syn.interrogative_fraction;

    let register =
        if contractions_rate > 0.4 && first_person_rate > 0.02 && avg_sentence_length < 18.0 {
            "casual"
        } else if contractions_rate < 0.1 && avg_sentence_length > 22.0 {
            "formal"
        } else if interrogative_rate > 0.12 {
            "inquisitive"
        } else if first_person_rate > 0.04 {
            "personal-reflective"
        } else {
            "neutral"
        }
        .to_string();

    StylisticProfile {
        opening_gambits,
        sign_offs,
        register,
        first_person_rate,
        contractions_rate,
    }
}
