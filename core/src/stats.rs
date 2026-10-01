//! Usage plus latency stats (v0.9.0, issue 43).
//!
//! The user-facing half of observability: per-day words, polished
//! entries, dictionary hits, top apps, and a streak. The engineering
//! half is latency percentiles over end-to-end session times. All
//! pure: storage fetches rows, this module does the math, callers
//! print it. Rows with `created_at` zero predate day tracking and
//! count toward totals only.

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
