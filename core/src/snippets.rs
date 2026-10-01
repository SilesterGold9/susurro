//! Spoken snippet expansion (v1.1.0, issue 55).
//!
//! Saying a stored trigger injects its expansion instead of the raw
//! words. Matching runs after cleanup and before injection, on the
//! whole utterance only. Exact normalized equality or nothing: partial
//! or fuzzy input stays dictation, never a guess.

/// One trigger plus its expansion. Trigger stores lowercased, like
/// the sqlite table; expansion keeps its case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub trigger: String,
    pub expansion: String,
}

impl Snippet {
    /// Validate at the boundary. Trigger lowercases and trims,
    /// expansion trims but keeps case.
    pub fn new(trigger: &str, expansion: &str) -> Result<Self, String> {
        let t = trigger.trim().to_lowercase();
        let e = expansion.trim().to_string();
        if t.is_empty() {
            return Err("empty trigger. Name the words that expand.".into());
        }
        if e.is_empty() {
            return Err("empty expansion. Give the trigger something to expand to.".into());
        }
        Ok(Self {
            trigger: t,
            expansion: e,
        })
    }
}

/// Normalize for comparison: lowercase, strip surrounding punctuation
/// the polisher adds (periods, commas, bangs), collapse inner space.
/// Both sides normalize, so `My  Email.` matches trigger `my email`.
pub fn normalize_trigger(s: &str) -> String {
    let lower = s.trim().to_lowercase();
    let stripped = lower.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
            )
    });
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Exact normalized match against the list, list order wins ties.
/// Storage lists ORDER BY trigger, so results stay deterministic.
/// Returns trigger plus expansion of the hit, or None.
pub fn find_expansion<'a>(cleaned: &str, snippets: &'a [Snippet]) -> Option<(&'a str, &'a str)> {
    let want = normalize_trigger(cleaned);
    if want.is_empty() {
        return None;
    }
    snippets
        .iter()
        .find(|s| normalize_trigger(&s.trigger) == want)
        .map(|s| (s.trigger.as_str(), s.expansion.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippets() -> Vec<Snippet> {
        vec![
            Snippet::new("my email", "me@example.com").unwrap(),
            Snippet::new("standup link", "https://meet.example.com/daily").unwrap(),
        ]
    }

    #[test]
    fn exact_match_is_case_insensitive() {
        let s = snippets();
        assert_eq!(
            find_expansion("MY EMAIL", &s),
            Some(("my email", "me@example.com"))
        );
    }

    #[test]
    fn trailing_punctuation_is_tolerated() {
        let s = snippets();
        for text in [
            "my email.",
            "My email!",
            "my email?",
            "my email,",
            "  my   email.  ",
        ] {
            assert_eq!(
                find_expansion(text, &s),
                Some(("my email", "me@example.com")),
                "{text}"
            );
        }
    }

    #[test]
    fn partial_or_longer_input_never_expands() {
        let s = snippets();
        for text in [
            "send my email please",
            "my email is",
            "my",
            "email",
            "my emails",
            "standup",
            "",
            "   ",
            "...",
        ] {
            assert_eq!(find_expansion(text, &s), None, "{text}");
        }
    }

    #[test]
    fn empty_list_never_expands() {
        assert_eq!(find_expansion("my email", &[]), None);
    }

    #[test]
    fn constructor_rejects_blanks() {
        assert!(Snippet::new("  ", "x").is_err());
        assert!(Snippet::new("y", "  ").is_err());
    }
}
