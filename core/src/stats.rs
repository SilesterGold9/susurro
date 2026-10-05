//! Usage plus latency stats (v0.9.0, issue 43) and the voice
//! fingerprint (issue 64).
//!
//! The user-facing half of observability: per-day words, polished
//! entries, dictionary hits, top apps, and a streak. The engineering
//! half is latency percentiles over end-to-end session times. All
//! pure: storage fetches rows, this module does the math, callers
//! print it. Rows with `created_at` zero predate day tracking and
//! count toward totals only.
//!
//! The fingerprint is the cheap half of a voice profile: counting over
//! history we already store. No model, no topic clustering. Every
//! result is deterministic, because a card that reshuffles between two
//! identical reads is a card nobody trusts.

use crate::ports::HistoryEntry;

/// One day of dictation, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCount {
    pub label: String,
    pub words: u64,
}

/// The whole usage picture in one struct.
#[derive(Debug, Clone)]
pub struct Summary {
    pub entries: usize,
    pub words: u64,
    pub polished: usize,
    pub dict_hits: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
    pub days: Vec<DayCount>,
    pub streak_days: u64,
    pub top_apps: Vec<(String, usize)>,
    /// Issue 64. `None` where history is too thin to say anything
    /// honest rather than a confident-looking zero.
    pub fingerprint: VoiceFingerprint,
}

/// What counting can honestly claim about how someone dictates
/// (issue 64). Three cards: the words they lean on, the phrase they
/// repeat, and when they talk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceFingerprint {
    /// Most-used words after stopword removal, most frequent first.
    pub top_words: Vec<(String, usize)>,
    /// Most frequent word trigram, the "catchphrase".
    pub catchphrase: Option<(String, usize)>,
    /// Busiest hour of the day, 0-23, with the session count.
    pub peak_hour: Option<(u32, usize)>,
}

/// Words carrying no signal in a frequency ranking. Short and common
/// enough that counting them tells you about English, not about the
/// speaker. Deliberately small: an aggressive list would strip real
/// signal like "not" or "because" out of a working user's vocabulary.
const STOPWORDS: &[&str] = &[
    "a", "about", "all", "am", "an", "and", "any", "are", "as", "at", "be", "been", "but", "by",
    "can", "did", "do", "does", "for", "from", "get", "had", "has", "have", "he", "her", "him",
    "his", "how", "i", "if", "in", "into", "is", "it", "its", "just", "like", "me", "my", "no",
    "not", "of", "on", "one", "or", "our", "out", "she", "so", "some", "than", "that", "the",
    "their", "them", "then", "there", "these", "they", "this", "to", "up", "us", "was", "we",
    "were", "what", "when", "which", "who", "will", "with", "would", "you", "your",
];

/// Lowercase alphanumeric words with stopwords removed. Shared by the
/// word counts and the catchphrase so both see the same tokens.
fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(str::to_lowercase)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// The voice fingerprint over history rows (issue 64).
///
/// Deterministic by construction: counts come from sorted maps and
/// ties break alphabetically, so the same rows always produce the same
/// three cards. Words need a trigram to make a catchphrase, and the
/// peak hour needs dated rows, so both stay `None` until the history
/// is thick enough to mean something.
pub fn fingerprint(rows: &[HistoryEntry], top_n: usize) -> VoiceFingerprint {
    let mut words: std::collections::BTreeMap<String, usize> = Default::default();
    let mut trigrams: std::collections::BTreeMap<String, usize> = Default::default();
    let mut per_hour: [usize; 24] = [0; 24];
    let mut dated_hours = 0usize;

    for row in rows {
        let tokens = content_words(&row.raw_text);
        for w in &tokens {
            *words.entry(w.clone()).or_default() += 1;
        }
        for w in tokens.windows(3) {
            *trigrams.entry(w.join(" ")).or_default() += 1;
        }
        if row.created_at > 0 {
            // Hours are UTC because that is what the timestamp is.
            // Naming the zone in the caller keeps this honest: local
            // hour needs a timezone the store does not carry.
            let hour = ((row.created_at.rem_euclid(86_400)) / 3600) as usize;
            if hour < 24 {
                per_hour[hour] += 1;
                dated_hours += 1;
            }
        }
    }

    // Most frequent first, ties alphabetical so the order never wobbles.
    let mut ranked: Vec<(String, usize)> = words.into_iter().filter(|(_, n)| *n > 0).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(top_n);

    // A phrase used once is noise, so a catchphrase needs a repeat.
    // Ties break alphabetically ascending, spelled out here rather
    // than left to `max_by`, whose reversal is easy to misread.
    let mut catchphrase: Option<(String, usize)> = None;
    for (phrase, count) in trigrams {
        if count <= 1 {
            continue;
        }
        let better = match &catchphrase {
            None => true,
            Some((best_phrase, best_count)) => {
                count > *best_count || (count == *best_count && phrase < *best_phrase)
            }
        };
        if better {
            catchphrase = Some((phrase, count));
        }
    }

    let peak_hour = (dated_hours >= 2)
        .then(|| {
            per_hour
                .iter()
                .enumerate()
                .max_by_key(|(h, n)| (**n, std::cmp::Reverse(*h)))
                .map(|(h, n)| (h as u32, *n))
        })
        .flatten();

    VoiceFingerprint {
        top_words: ranked,
        catchphrase,
        peak_hour,
    }
}

/// Nearest-rank percentile over sorted values. Empty reads zero.
pub fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (pct / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Days since the Unix epoch for a timestamp.
pub fn day_index(created_at: i64) -> i64 {
    created_at.div_euclid(86_400)
}

/// Gregorian label for a day index (Howard Hinnant's algorithm).
/// No date dependency for one label format.
pub fn day_label(day_idx: i64) -> String {
    let z = day_idx + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", y + i64::from(m <= 2), m, d)
}

fn words(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}

/// Summarize history rows. `dict` holds custom phrases; an entry
/// counts as a dictionary hit when its text mentions one of them.
/// `today_idx` is `day_index(now)`, passed in so tests pin the day.
pub fn summarize(rows: &[HistoryEntry], dict: &[String], today_idx: i64) -> Summary {
    let mut latencies: Vec<u64> = Vec::with_capacity(rows.len());
    let mut words_total = 0u64;
    let mut polished = 0usize;
    let mut dict_hits = 0usize;
    let mut per_day: std::collections::BTreeMap<i64, u64> = std::collections::BTreeMap::new();
    let mut per_app: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let lowered_dict: Vec<String> = dict.iter().map(|p| p.to_lowercase()).collect();

    for row in rows {
        latencies.push(row.latency_ms);
        words_total += words(&row.raw_text);
        let cleaned = row.cleaned_text.as_deref().unwrap_or("");
        if !cleaned.is_empty() && cleaned != row.raw_text {
            polished += 1;
        }
        if !lowered_dict.is_empty() {
            let hay = format!("{} {cleaned}", row.raw_text).to_lowercase();
            if lowered_dict.iter().any(|p| hay.contains(p.as_str())) {
                dict_hits += 1;
            }
        }
        if row.created_at > 0 {
            *per_day.entry(day_index(row.created_at)).or_default() += words(&row.raw_text);
        }
        if let Some(app) = row.app.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            *per_app.entry(app.to_lowercase()).or_default() += 1;
        }
    }
    latencies.sort_unstable();

    // Last seven days with entries, oldest first.
    let days: Vec<DayCount> = per_day
        .iter()
        .rev()
        .take(7)
        .rev()
        .map(|(idx, w)| DayCount {
            label: day_label(*idx),
            words: *w,
        })
        .collect();

    // Streak: consecutive days with entries ending today. Today may
    // still be empty while dictation is pending, so one grace day.
    let mut streak_days = 0u64;
    let mut cursor = if per_day.contains_key(&today_idx) {
        today_idx
    } else {
        today_idx - 1
    };
    while per_day.contains_key(&cursor) {
        streak_days += 1;
        cursor -= 1;
    }

    let mut top_apps: Vec<(String, usize)> = per_app.into_iter().collect();
    top_apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_apps.truncate(5);

    Summary {
        entries: rows.len(),
        words: words_total,
        polished,
        dict_hits,
        p50_ms: percentile(&latencies, 50.0),
        p95_ms: percentile(&latencies, 95.0),
        p99_ms: percentile(&latencies, 99.0),
        days,
        streak_days,
        top_apps,
        fingerprint: fingerprint(rows, 8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;

    fn row(
        session: u128,
        raw: &str,
        cleaned: Option<&str>,
        latency: u64,
        app: Option<&str>,
        day: i64,
    ) -> HistoryEntry {
        HistoryEntry {
            session: SessionId::new(session),
            raw_text: raw.into(),
            cleaned_text: cleaned.map(|c| c.into()),
            provider: "local".into(),
            latency_ms: latency,
            app: app.map(|a| a.into()),
            created_at: day * 86_400,
        }
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let v = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&v, 50.0), 50);
        assert_eq!(percentile(&v, 95.0), 100);
        assert_eq!(percentile(&v, 99.0), 100);
        assert_eq!(percentile(&[], 50.0), 0);
        assert_eq!(percentile(&[7], 50.0), 7);
    }

    #[test]
    fn labels_format_gregorian_days() {
        // 2026-10-01.
        assert_eq!(day_label(day_index(1_790_812_800)), "2026-10-01");
        assert_eq!(day_label(day_index(0)), "1970-01-01");
    }

    #[test]
    fn summary_counts_words_polish_and_hits() {
        let rows = vec![
            row(
                1,
                "hello world",
                Some("Hello world."),
                100,
                Some("Docs"),
                20_000,
            ),
            row(2, "buy milk", None, 200, Some("chat"), 20_000),
            row(
                3,
                "ship susurro fast",
                Some("Ship susurro fast!"),
                300,
                None,
                19_999,
            ),
        ];
        let s = summarize(&rows, &["susurro".into()], 20_000);
        assert_eq!(s.entries, 3);
        assert_eq!(s.words, 7);
        assert_eq!(s.polished, 2);
        assert_eq!(s.dict_hits, 1);
        assert_eq!(s.p50_ms, 200);
        assert_eq!(s.streak_days, 2);
        assert_eq!(s.top_apps, vec![("chat".into(), 1), ("docs".into(), 1)]);
        assert_eq!(s.days.len(), 2);
        assert_eq!(s.days[1].words, 4);
    }

    #[test]
    fn fingerprint_ranks_content_words_and_finds_the_repeated_phrase() {
        let rows = vec![
            row(
                1,
                "the deploy pipeline broke again because the cache was stale",
                None,
                10,
                None,
                20_000,
            ),
            row(
                2,
                "the deploy pipeline broke again because the cache was stale",
                None,
                10,
                None,
                20_000,
            ),
            row(3, "ship it", None, 10, None, 20_000),
        ];
        let f = fingerprint(&rows, 8);
        // Stopwords ("the", "it", "was") never appear.
        let words: Vec<&str> = f.top_words.iter().map(|(w, _)| w.as_str()).collect();
        assert!(!words.contains(&"the"), "{words:?}");
        assert!(!words.contains(&"it"), "{words:?}");
        assert!(!words.contains(&"was"), "{words:?}");
        // The repeated trigram wins and repeats matter.
        // Four trigrams repeat equally; the alphabetical tie-break makes
        // "again because cache" the winner every time.
        assert_eq!(f.catchphrase, Some(("again because cache".into(), 2)));
        // Ties break alphabetically, so the order never wobbles.
        assert!(words.contains(&"cache"), "{words:?}");
    }

    #[test]
    fn fingerprint_is_deterministic_across_identical_reads() {
        let rows = vec![
            row(1, "alpha beta gamma delta", None, 10, None, 20_000),
            row(2, "alpha beta gamma delta", None, 10, None, 20_001),
            row(3, "epsilon zeta", None, 10, None, 20_001),
        ];
        assert_eq!(fingerprint(&rows, 5), fingerprint(&rows, 5));
        // Reordering rows must not change the answer either.
        let mut shuffled = rows.clone();
        shuffled.reverse();
        assert_eq!(fingerprint(&rows, 5), fingerprint(&shuffled, 5));
    }

    #[test]
    fn fingerprint_withholds_answers_it_cannot_support() {
        // One session: no repeated phrase, no peak hour worth naming.
        let rows = vec![row(1, "hello world", None, 10, None, 20_000)];
        let f = fingerprint(&rows, 5);
        assert_eq!(f.catchphrase, None, "a phrase used once is noise");
        assert_eq!(f.peak_hour, None, "one timestamp is not a pattern");
        // A single row can still name words; it cannot name patterns.
        assert_eq!(f.top_words, vec![("hello".into(), 1), ("world".into(), 1)]);
        // All-stopword text yields nothing rather than filler.
        let filler = vec![row(1, "the and of it is", None, 10, None, 20_000)];
        assert!(fingerprint(&filler, 5).top_words.is_empty());
        // Empty history says nothing at all rather than guessing.
        assert_eq!(fingerprint(&[], 5).top_words, Vec::new());
    }

    #[test]
    fn peak_hour_counts_utc_and_breaks_ties_early() {
        // 09:00 UTC twice, 14:00 UTC once: the busiest hour wins.
        let nine = 20_000 * 86_400 + 9 * 3600;
        let nine2 = 20_000 * 86_400 + 9 * 3600 + 60;
        let fourteen = 20_000 * 86_400 + 14 * 3600;
        let rows = vec![
            row(1, "a b c", None, 10, None, 20_000),
            HistoryEntry {
                session: SessionId::new(2),
                raw_text: "d e f".into(),
                cleaned_text: None,
                provider: "local".into(),
                latency_ms: 10,
                app: None,
                created_at: nine,
            },
            HistoryEntry {
                session: SessionId::new(3),
                raw_text: "g h i".into(),
                cleaned_text: None,
                provider: "local".into(),
                latency_ms: 10,
                app: None,
                created_at: nine2,
            },
            HistoryEntry {
                session: SessionId::new(4),
                raw_text: "j k l".into(),
                cleaned_text: None,
                provider: "local".into(),
                latency_ms: 10,
                app: None,
                created_at: fourteen,
            },
        ];
        assert_eq!(fingerprint(&rows, 5).peak_hour, Some((9, 2)));
    }

    #[test]
    fn summary_carries_the_fingerprint() {
        let rows = vec![row(
            1,
            "susurro dictates fast and susurro ships fast",
            None,
            10,
            None,
            20_000,
        )];
        let s = summarize(&rows, &[], 20_000);
        // "susurro" and "fast" both appear twice; the tie breaks
        // alphabetically, so "fast" leads.
        assert_eq!(s.fingerprint.top_words[0], ("fast".into(), 2));
        assert_eq!(s.fingerprint.top_words[1], ("susurro".into(), 2));
        // "and" is a stopword and never reaches the card.
        assert!(!s.fingerprint.top_words.iter().any(|(w, _)| w == "and"));
    }

    #[test]
    fn streak_breaks_on_gap_and_zero_days_are_totals_only() {
        let rows = vec![
            row(1, "a b", None, 10, None, 20_000),
            row(2, "c", None, 10, None, 19_998),
            // Undated rows count in totals, never in days.
            HistoryEntry {
                session: SessionId::new(3),
                raw_text: "d e f".into(),
                cleaned_text: None,
                provider: "local".into(),
                latency_ms: 10,
                app: None,
                created_at: 0,
            },
        ];
        let s = summarize(&rows, &[], 20_000);
        assert_eq!(s.entries, 3);
        assert_eq!(s.words, 6);
        assert_eq!(s.streak_days, 1);
        assert_eq!(s.days.len(), 2);
    }
}
