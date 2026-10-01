//! Dictionary suggestions from history corrections (issue 63).
//!
//! Whisper mangles names, the polish chain sometimes repairs them, and
//! the user keeps the repair. Diffing raw against cleaned across history
//! surfaces those repairs as dictionary candidates, ranked by how many
//! sessions evidence them. Nothing is auto-added: the CLI prints the
//! ranking, one flag writes the chosen set.
//!
//! The word-overlap guard stays the judge: entries that trip it are
//! rewrites, not corrections, and never propose.

use crate::ports::HistoryEntry;
use crate::SessionId;

/// One candidate phrase plus the sessions that evidence it.
/// Sessions ride newest-first, capped, so every proposal explains itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhraseSuggestion {
    pub phrase: String,
    pub sessions: Vec<SessionId>,
}

impl PhraseSuggestion {
    pub fn count(&self) -> usize {
        self.sessions.len()
    }
}

/// Sessions named per proposal. Enough to audit, never the whole log.
pub const SESSIONS_PER_SUGGESTION: usize = 5;

/// History rows scanned per suggest run. Bounded so a huge history
/// cannot stall the command; newest rows carry the freshest speech.
pub const SCAN_LIMIT: usize = 500;

/// Longest cleaned span that counts as a phrase. Wider spans are
/// rewrites the guard should already have rejected.
pub const MAX_PHRASE_WORDS: usize = 4;

/// One whitespace token with its normalized and surface forms.
/// Stripping is outer punctuation only, so `Hyprland,` compares as
/// `hyprland` but prints with its case kept.
fn tokens(s: &str) -> Vec<(String, String)> {
    s.split_whitespace()
        .filter_map(|w| {
            let surface = w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            if surface.is_empty() {
                return None;
            }
            Some((surface.to_lowercase(), surface))
        })
        .collect()
}

/// Cleaned-side spans the raw transcript lacks, via word LCS.
/// Returns norm-key plus surface phrase per span. Deletions-only and
/// punctuation-only pairs yield nothing.
fn corrected_spans(raw: &str, cleaned: &str) -> Vec<(String, String)> {
    let r = tokens(raw);
    let c = tokens(cleaned);
    let (rn, cn): (Vec<&str>, Vec<&str>) = (
        r.iter().map(|t| t.0.as_str()).collect(),
        c.iter().map(|t| t.0.as_str()).collect(),
    );
    // LCS lengths over the norm sequences.
    let (n, m) = (rn.len(), cn.len());
    let mut len = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            len[i][j] = if rn[i] == cn[j] {
                len[i + 1][j + 1] + 1
            } else {
                len[i + 1][j].max(len[i][j + 1])
            };
        }
    }
    // Walk the table, grouping consecutive cleaned-only indices.
    let (mut i, mut j) = (0, 0);
    let mut spans: Vec<Vec<usize>> = Vec::new();
    let mut open: Option<Vec<usize>> = None;
    let flush = |open: &mut Option<Vec<usize>>, spans: &mut Vec<Vec<usize>>| {
        if let Some(span) = open.take() {
            spans.push(span);
        }
    };
    while i < n && j < m {
        if rn[i] == cn[j] {
            flush(&mut open, &mut spans);
            i += 1;
            j += 1;
        } else if len[i + 1][j] >= len[i][j + 1] {
            flush(&mut open, &mut spans);
            i += 1;
        } else {
            open.get_or_insert_with(Vec::new).push(j);
            j += 1;
        }
    }
    while j < m {
        open.get_or_insert_with(Vec::new).push(j);
        j += 1;
    }
    flush(&mut open, &mut spans);
    spans
        .into_iter()
        .filter(|span| !span.is_empty() && span.len() <= MAX_PHRASE_WORDS)
        .map(|span| {
            let key = span
                .iter()
                .map(|&k| c[k].0.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let surface = span
                .iter()
                .map(|&k| c[k].1.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (key, surface)
        })
        .collect()
}

/// True when the phrase belongs in a whisper prompt boost: alphabetic,
/// not an address or link, not a lone letter or bare number.
fn keep_phrase(key: &str) -> bool {
    if key.len() < 2 || !key.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    if key.contains('@') || key.contains("://") {
        return false;
    }
    if key
        .split_whitespace()
        .all(|w| w.chars().all(|c| c.is_numeric()))
    {
        return false;
    }
    true
}

/// Ranked dictionary candidates over history. `entries` is newest-first,
/// as `SqliteHistory::recent` returns. `existing` holds current phrases;
/// matches are case-insensitive, so nothing proposes twice. `limit` caps
/// the ranking; zero means no cap.
pub fn suggest_phrases(
    entries: &[HistoryEntry],
    existing: &[String],
    limit: usize,
) -> Vec<PhraseSuggestion> {
    let known: std::collections::HashSet<String> =
        existing.iter().map(|p| p.trim().to_lowercase()).collect();
    // Norm key to first-seen surface plus evidencing sessions.
    let mut order: Vec<String> = Vec::new();
    let mut table: std::collections::HashMap<String, (String, Vec<SessionId>)> =
        std::collections::HashMap::new();
    for entry in entries.iter().take(SCAN_LIMIT) {
        let Some(cleaned) = entry.cleaned_text.as_deref() else {
            continue;
        };
        if entry.raw_text.trim().is_empty() || cleaned.trim().is_empty() {
            continue;
        }
        // The guard judges: rewrites never propose.
        if !crate::words::preserves_words(&entry.raw_text, cleaned) {
            continue;
        }
        for (key, surface) in corrected_spans(&entry.raw_text, cleaned) {
            if !keep_phrase(&key) || known.contains(&key) {
                continue;
            }
            match table.get_mut(&key) {
                Some((_, sessions)) => {
                    if sessions.len() < SESSIONS_PER_SUGGESTION
                        && !sessions.contains(&entry.session)
                    {
                        sessions.push(entry.session);
                    }
                }
                None => {
                    order.push(key.clone());
                    table.insert(key, (surface, vec![entry.session]));
                }
            }
        }
    }
    let mut out: Vec<PhraseSuggestion> = order
        .into_iter()
        .filter_map(|key| {
            table
                .remove(&key)
                .map(|(phrase, sessions)| PhraseSuggestion { phrase, sessions })
        })
        .collect();
    // Frequency first, phrase second: deterministic on ties.
    out.sort_by(|a, b| {
        b.count()
            .cmp(&a.count())
            .then_with(|| a.phrase.to_lowercase().cmp(&b.phrase.to_lowercase()))
    });
    if limit > 0 {
        out.truncate(limit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session: u128, raw: &str, cleaned: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            session: SessionId::new(session),
            raw_text: raw.into(),
            cleaned_text: cleaned.map(|c| c.into()),
            provider: "local".into(),
            latency_ms: 1,
            app: None,
            created_at: 0,
        }
    }

    #[test]
    fn single_correction_proposes_with_session() {
        // Long enough that one merge stays above the guard: the judge
        // trusts small fractions, never short-utterance rewrites.
        let entries = vec![entry(
            1,
            "i manage my windows with hyper land every day",
            Some("I manage my windows with Hyprland every day."),
        )];
        let out = suggest_phrases(&entries, &[], 20);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].phrase, "Hyprland");
        assert_eq!(out[0].sessions, vec![SessionId::new(1)]);
    }

    #[test]
    fn punctuation_only_proposes_nothing() {
        let entries = vec![entry(1, "hello world", Some("Hello, world."))];
        assert!(suggest_phrases(&entries, &[], 20).is_empty());
    }

    #[test]
    fn rewrite_tripping_guard_never_proposes() {
        let entries = vec![entry(
            1,
            "buy milk",
            Some("Buy milk. Also, call your mother about dinner."),
        )];
        assert!(suggest_phrases(&entries, &[], 20).is_empty());
    }

    #[test]
    fn frequency_ranks_first_and_ties_break_alphabetically() {
        let entries = vec![
            entry(
                1,
                "please send the notes to sussuro before noon today",
                Some("Please send the notes to Susurro before noon today."),
            ),
            entry(
                2,
                "sussuro helps me write faster every single day",
                Some("Susurro helps me write faster every single day."),
            ),
            entry(
                3,
                "i manage my windows with hyper land every day",
                Some("I manage my windows with Hyprland every day."),
            ),
        ];
        let out = suggest_phrases(&entries, &[], 20);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].phrase, "Susurro");
        assert_eq!(out[0].count(), 2);
        assert_eq!(out[1].phrase, "Hyprland");
    }

    #[test]
    fn known_phrases_never_repropose() {
        let entries = vec![entry(
            1,
            "i manage my windows with hyper land every day",
            Some("I manage my windows with Hyprland every day."),
        )];
        let existing = vec!["HYPRland ".to_string()];
        assert!(suggest_phrases(&entries, &existing, 20).is_empty());
    }

    #[test]
    fn sessions_cap_and_stay_explainable() {
        let entries: Vec<HistoryEntry> = (1..=8)
            .map(|s| {
                entry(
                    s,
                    "i want to thank sussuro for the help today please",
                    Some("I want to thank Susurro for the help today, please."),
                )
            })
            .collect();
        let out = suggest_phrases(&entries, &[], 20);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sessions.len(), SESSIONS_PER_SUGGESTION);
        assert_eq!(out[0].sessions[0], SessionId::new(1));
    }

    #[test]
    fn junk_spans_stay_out() {
        // Addresses and bare numbers never reach the prompt. The email
        // and number pairs pass the guard, so only the phrase filter
        // can stop them; the link explodes into words and trips the
        // guard first. Either judge bars prompt junk.
        for (raw, cleaned, guarded) in [
            (
                "please contact me tomorrow morning about the new contract terms",
                "Please contact me@example.com tomorrow morning about the new contract terms.",
                true,
            ),
            (
                "please open the link tomorrow morning for the team review",
                "Please open https://meet.example.com/daily tomorrow morning for the team review.",
                false,
            ),
            (
                "please call forty two people tomorrow morning for lunch",
                "Please call 42 people tomorrow morning for lunch.",
                true,
            ),
        ] {
            assert_eq!(
                crate::words::preserves_words(raw, cleaned),
                guarded,
                "{cleaned}"
            );
            let entries = vec![entry(1, raw, Some(cleaned))];
            let found = suggest_phrases(&entries, &[], 20);
            let phrases: Vec<&str> = found.iter().map(|s| s.phrase.as_str()).collect();
            assert!(
                !phrases.iter().any(|p| p.contains('@') || p.contains("://")),
                "{cleaned} -> {phrases:?}"
            );
        }
        let bare = vec![entry(
            1,
            "please call forty two people tomorrow morning for lunch",
            Some("Please call 42 people tomorrow morning for lunch."),
        )];
        assert!(suggest_phrases(&bare, &[], 20).is_empty());
    }

    #[test]
    fn missing_or_empty_sides_skip() {
        let entries = vec![
            entry(1, "hello", None),
            entry(2, "  ", Some("Hello.")),
            entry(3, "hello", Some("  ")),
        ];
        assert!(suggest_phrases(&entries, &[], 20).is_empty());
    }
}
