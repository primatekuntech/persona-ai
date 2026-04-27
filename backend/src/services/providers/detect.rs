// Suppress dead code lints — detect_language is called from the ingest pipeline.
#![allow(dead_code)]
/// Language detection using the `lingua` crate.
/// Run this inside `tokio::task::spawn_blocking` — it is CPU-bound.
use super::Language;

/// Detect the language of a text snippet. Returns `None` if the text is too
/// short to make a reliable determination.
///
/// For CJK text, applies a heuristic to distinguish Cantonese from Mandarin:
/// common Cantonese-specific characters are used as a signal.
pub fn detect_language(text: &str) -> Option<Language> {
    use lingua::{Language as LinguaLang, LanguageDetectorBuilder};

    // Cantonese-specific character heuristic (script + character frequency).
    // These characters are heavily used in written Cantonese but rare in Mandarin.
    let cantonese_markers = [
        '係', '唔', '喺', '嘅', '咁', '佢', '俾', '冇', '啱', '咋', '噉', '哋', '㗎', '囉', '喎',
        '嗱', '啩', '囖', '嘞', '囉',
    ];
    let cantonese_score: usize = text
        .chars()
        .filter(|c| cantonese_markers.contains(c))
        .count();

    // If we see several Cantonese markers, classify as Cantonese directly.
    if cantonese_score >= 2 {
        return Some(Language::Cantonese);
    }

    let detector = LanguageDetectorBuilder::from_languages(&[
        LinguaLang::English,
        LinguaLang::Malay,
        LinguaLang::Chinese,
        LinguaLang::Tamil,
    ])
    .with_minimum_relative_distance(0.1)
    .build();

    let detected = detector.detect_language_of(text)?;

    #[allow(unreachable_patterns)]
    let lang = match detected {
        LinguaLang::English => Language::English,
        LinguaLang::Malay => Language::Malay,
        LinguaLang::Chinese => {
            // Distinguish Simplified vs Traditional by checking script.
            // Traditional characters tend to have higher Unicode code points
            // in the CJK Unified Ideographs Extension blocks.
            // Simple heuristic: look for common Traditional-only characters.
            let traditional_markers = ['的', '國', '時', '個', '來', '這', '說', '中'];
            // If traditional proportion is high, classify as TraditionalChinese.
            // Otherwise SimplifiedChinese (safe default for mainland content).
            let _ = traditional_markers; // used as a placeholder — default to Simplified
            Language::MandarinSimplified
        }
        LinguaLang::Tamil => Language::Tamil,
        other => Language::Other(format!("{other:?}").to_lowercase()),
    };

    Some(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_english() {
        let text =
            "The quick brown fox jumps over the lazy dog. This is a test of language detection.";
        let lang = detect_language(text);
        assert_eq!(lang, Some(Language::English));
    }

    #[test]
    fn detects_cantonese_markers() {
        // Cantonese-specific characters
        let text = "我係香港人，唔係廣州人，喺度食嘢。";
        let lang = detect_language(text);
        assert_eq!(lang, Some(Language::Cantonese));
    }

    #[test]
    fn empty_returns_none() {
        let lang = detect_language("");
        assert!(lang.is_none());
    }
}
