//! Word-overlap guard (moved from adapters-cleanup, issue 63).
//!
//! The cleanup chain judges punctuation-only edits here, and the
//! dictionary suggester trusts only entries that pass. One definition,
//! every caller shares it.

/// Minimum word F1 for a cleanup to count as punctuation-only.
/// Punctuation and case never change the score, so a faithful edit
/// always passes and a rewrite trips the fallback.
pub const F1_MINIMUM: f64 = 0.8;

/// Lowercase alphanumeric words, so "Hello," and "hello" compare equal.
fn norm_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// True when `cleaned` keeps the words of `raw`: punctuation-only edits
/// score 1.0, paraphrases collapse toward 0.
pub fn preserves_words(raw: &str, cleaned: &str) -> bool {
    let r = norm_words(raw);
    let mut c = norm_words(cleaned);
    if r.is_empty() {
        return c.is_empty();
    }
    if c.is_empty() || c.len() > r.len() + r.len() / 4 + 1 {
        return false;
    }
    // Multiset intersection over the longer side.
    c.sort();
    let mut r_sorted = r.clone();
    r_sorted.sort();
    let (mut i, mut j, mut hit) = (0, 0, 0);
    while i < r_sorted.len() && j < c.len() {
        if r_sorted[i] == c[j] {
            hit += 1;
            i += 1;
            j += 1;
        } else if r_sorted[i] < c[j] {
            i += 1;
        } else {
            j += 1;
        }
    }
    word_f1(hit, c.len(), r_sorted.len()) >= F1_MINIMUM
}

/// Word F1 from intersection size over cleaned and raw word counts.
/// Precision punishes added words, recall punishes dropped words.
pub fn word_f1(hit: usize, cleaned_len: usize, raw_len: usize) -> f64 {
    if hit == 0 || cleaned_len == 0 || raw_len == 0 {
        return 0.0;
    }
    let precision = hit as f64 / cleaned_len as f64;
    let recall = hit as f64 / raw_len as f64;
    2.0 * precision * recall / (precision + recall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faithful_passes_punctuation_only() {
        assert!(preserves_words("hello world", "Hello, world."));
        assert!(preserves_words(
            "i try to test susura",
            "I try to test susura."
        ));
    }

    #[test]
    fn rewrite_trips_the_guard() {
        assert!(!preserves_words(
            "the quick brown fox jumps",
            "a fast dark fox leaps high over everything today"
        ));
        assert!(!preserves_words(
            "buy milk",
            "Buy milk. Also, call your mother about dinner."
        ));
        assert!(!preserves_words("buy milk", "  "));
    }

    #[test]
    fn f1_scores_precision_and_recall() {
        assert_eq!(word_f1(2, 2, 2), 1.0);
        assert_eq!(word_f1(0, 2, 2), 0.0);
        assert!(word_f1(5, 7, 5) > F1_MINIMUM);
        assert!(word_f1(4, 7, 4) < F1_MINIMUM);
        assert!(word_f1(3, 3, 6) < F1_MINIMUM);
    }
}
