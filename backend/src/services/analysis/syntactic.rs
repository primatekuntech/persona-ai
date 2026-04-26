use super::Chunk;

pub struct SyntacticProfile {
    pub avg_sentence_length: f64,
    pub short_fraction: f64,
    pub medium_fraction: f64,
    pub long_fraction: f64,
    pub declarative_fraction: f64,
    pub interrogative_fraction: f64,
    pub exclamatory_fraction: f64,
    pub fragment_fraction: f64,
    pub comma_per_sentence: f64,
    pub em_dash_per_1000: f64,
    pub ellipsis_per_1000: f64,
    pub semicolon_per_1000: f64,
    pub paragraph_length_avg_sentences: f64,
}

impl SyntacticProfile {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "avg_sentence_length": self.avg_sentence_length,
            "sentence_length_distribution": {
                "short (<10)": self.short_fraction,
                "medium (10-20)": self.medium_fraction,
                "long (>20)": self.long_fraction,
            },
            "sentence_type_mix": {
                "declarative": self.declarative_fraction,
                "interrogative": self.interrogative_fraction,
                "exclamatory": self.exclamatory_fraction,
                "fragment": self.fragment_fraction,
            },
            "punctuation_rhythm": {
                "comma_per_sentence": self.comma_per_sentence,
                "em_dash_per_1000_words": self.em_dash_per_1000,
                "ellipsis_per_1000_words": self.ellipsis_per_1000,
                "semicolon_per_1000_words": self.semicolon_per_1000,
            },
            "paragraph_length_avg_sentences": self.paragraph_length_avg_sentences,
        })
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        current.push(c);

        if matches!(c, '.' | '!' | '?') {
            // Consume any trailing punctuation
            while i + 1 < len && matches!(chars[i + 1], '.' | '!' | '?') {
                i += 1;
                current.push(chars[i]);
            }
            // Check if followed by whitespace or end
            if i + 1 >= len || chars[i + 1].is_whitespace() || chars[i + 1] == '\n' {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                current.clear();
                // Skip whitespace
                while i + 1 < len && chars[i + 1].is_whitespace() {
                    i += 1;
                }
            }
        } else if c == '\n' {
            // Paragraph break — treat accumulated text as a sentence if non-empty
            if let Some(last) = current.trim_end_matches('\n').trim().chars().last() {
                if last != '.' && last != '!' && last != '?' {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        sentences.push(trimmed);
                    }
                    current.clear();
                }
            }
        }
        i += 1;
    }

    // Remaining text
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    sentences
}

fn word_count_of(s: &str) -> usize {
    s.split_whitespace().count()
}

fn classify(sentence: &str) -> (&'static str, usize) {
    let trimmed = sentence.trim();
    let wc = word_count_of(trimmed);
    if trimmed.ends_with('?') {
        ("interrogative", wc)
    } else if trimmed.ends_with('!') {
        ("exclamatory", wc)
    } else if wc < 5 {
        ("fragment", wc)
    } else {
        ("declarative", wc)
    }
}

pub fn compute(chunks: &[Chunk]) -> SyntacticProfile {
    let full_text: String = chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    if full_text.trim().is_empty() {
        return SyntacticProfile {
            avg_sentence_length: 0.0,
            short_fraction: 0.0,
            medium_fraction: 0.0,
            long_fraction: 0.0,
            declarative_fraction: 0.0,
            interrogative_fraction: 0.0,
            exclamatory_fraction: 0.0,
            fragment_fraction: 0.0,
            comma_per_sentence: 0.0,
            em_dash_per_1000: 0.0,
            ellipsis_per_1000: 0.0,
            semicolon_per_1000: 0.0,
            paragraph_length_avg_sentences: 0.0,
        };
    }

    let sentences = split_sentences(&full_text);
    let sentence_count = sentences.len();

    if sentence_count == 0 {
        return SyntacticProfile {
            avg_sentence_length: 0.0,
            short_fraction: 0.0,
            medium_fraction: 0.0,
            long_fraction: 0.0,
            declarative_fraction: 0.0,
            interrogative_fraction: 0.0,
            exclamatory_fraction: 0.0,
            fragment_fraction: 0.0,
            comma_per_sentence: 0.0,
            em_dash_per_1000: 0.0,
            ellipsis_per_1000: 0.0,
            semicolon_per_1000: 0.0,
            paragraph_length_avg_sentences: 0.0,
        };
    }

    let mut total_words = 0usize;
    let mut short_count = 0usize;
    let mut medium_count = 0usize;
    let mut long_count = 0usize;
    let mut declarative_count = 0usize;
    let mut interrogative_count = 0usize;
    let mut exclamatory_count = 0usize;
    let mut fragment_count = 0usize;

    for s in &sentences {
        let (kind, wc) = classify(s);
        total_words += wc;
        match kind {
            "interrogative" => interrogative_count += 1,
            "exclamatory" => exclamatory_count += 1,
            "fragment" => fragment_count += 1,
            _ => declarative_count += 1,
        }
        if wc < 10 {
            short_count += 1;
        } else if wc <= 20 {
            medium_count += 1;
        } else {
            long_count += 1;
        }
    }

    let sc = sentence_count as f64;
    let avg_sentence_length = total_words as f64 / sc;
    let short_fraction = short_count as f64 / sc;
    let medium_fraction = medium_count as f64 / sc;
    let long_fraction = long_count as f64 / sc;
    let declarative_fraction = declarative_count as f64 / sc;
    let interrogative_fraction = interrogative_count as f64 / sc;
    let exclamatory_fraction = exclamatory_count as f64 / sc;
    let fragment_fraction = fragment_count as f64 / sc;

    // Punctuation counts
    let comma_count = full_text.chars().filter(|&c| c == ',').count();
    let em_dash_count = full_text.matches('—').count() + full_text.matches(" -- ").count();
    let ellipsis_count = full_text.matches("...").count() + full_text.matches('…').count();
    let semicolon_count = full_text.chars().filter(|&c| c == ';').count();

    let total_words_f = total_words as f64;
    let comma_per_sentence = comma_count as f64 / sc;
    let em_dash_per_1000 = if total_words > 0 {
        em_dash_count as f64 / total_words_f * 1000.0
    } else {
        0.0
    };
    let ellipsis_per_1000 = if total_words > 0 {
        ellipsis_count as f64 / total_words_f * 1000.0
    } else {
        0.0
    };
    let semicolon_per_1000 = if total_words > 0 {
        semicolon_count as f64 / total_words_f * 1000.0
    } else {
        0.0
    };

    // Paragraph sentence averages: split full text on double newlines
    let paragraphs: Vec<&str> = full_text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .collect();
    let paragraph_length_avg_sentences = if paragraphs.is_empty() {
        sc
    } else {
        let para_sentence_counts: Vec<usize> = paragraphs
            .iter()
            .map(|p| split_sentences(p).len())
            .collect();
        let total_para_sentences: usize = para_sentence_counts.iter().sum();
        total_para_sentences as f64 / paragraphs.len() as f64
    };

    SyntacticProfile {
        avg_sentence_length,
        short_fraction,
        medium_fraction,
        long_fraction,
        declarative_fraction,
        interrogative_fraction,
        exclamatory_fraction,
        fragment_fraction,
        comma_per_sentence,
        em_dash_per_1000,
        ellipsis_per_1000,
        semicolon_per_1000,
        paragraph_length_avg_sentences,
    }
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
    fn sentence_split_basic() {
        let sentences = split_sentences("Hello world. How are you? Fine thanks!");
        assert_eq!(sentences.len(), 3);
    }

    #[test]
    fn interrogative_detected() {
        let chunks = vec![chunk("How are you? I am fine. What is this?")];
        let profile = compute(&chunks);
        assert!(profile.interrogative_fraction > 0.0);
    }

    #[test]
    fn punctuation_counts() {
        let text = "He said this, and that, and more. She left — quietly. Really...";
        let chunks = vec![chunk(text)];
        let profile = compute(&chunks);
        assert!(profile.comma_per_sentence > 0.0);
    }
}
