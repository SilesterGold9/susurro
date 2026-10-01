//! Cleanup adapters.
//!
//! - `PassthroughCleanup`: v0.0.1 raw path.
//! - `RegexCleanup`: dependency-free fallback (whitespace, spacing).
//! - `OllamaCleanup`: local LLM via the Ollama HTTP API, with
//!   automatic fallback to `RegexCleanup` when Ollama is down or
//!   the model is missing (fail-open: always inject something).

use susurro_core::ports::TextPostProcessorPort;

pub struct PassthroughCleanup;

impl TextPostProcessorPort for PassthroughCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        Ok(raw.to_string())
    }
}

pub struct RegexCleanup;

impl TextPostProcessorPort for RegexCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        Ok(cleanup_whitespace(raw))
    }
}

fn cleanup_whitespace(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    // Remove spaces before common closers, ensure one space after them.
    let mut out = collapsed
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" ?", "?")
        .replace(" !", "!")
        .replace(" :", ":")
        .replace(" ;", ";");
    // Collapse accidental double punctuation from STT artifacts.
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    out
}

pub struct OllamaCleanup {
    /// e.g. "qwen3:0.6b".
    pub model: String,
    /// e.g. "http://localhost:11434".
    pub endpoint: String,
}

impl OllamaCleanup {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.into(),
            endpoint: "http://localhost:11434".into(),
        }
    }

    pub fn prompt(raw: &str) -> String {
        format!(
            "You are a transcript punctuator. Fix punctuation and capitalization only. \
            Changing, adding, removing, or replacing ANY word is forbidden, even if the \
            sentence sounds odd. If you cannot punctuate without changing words, return \
            the transcript unchanged. Never explain. Output the transcript and nothing else.\n\
            Transcript: {raw}"
        )
    }
}

impl TextPostProcessorPort for OllamaCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        match chat_once(&self.endpoint, &self.model, &Self::prompt(raw)) {
            Ok(text) => {
                let text = strip_wrapping_quotes(strip_leading_label(text.trim()));
                if text.is_empty() || !preserves_words(raw, text) {
                    // Fail-open per plan: the model rewrote instead of
                    // punctuating, so regex fallback, never block injection.
                    eprintln!("polish rewrote the transcript, using regex fallback");
                    Ok(cleanup_whitespace(raw))
                } else {
                    Ok(text.to_string())
                }
            }
            // Fail-open per plan: regex fallback, never block injection.
            _ => Ok(cleanup_whitespace(raw)),
        }
    }
}

/// Drop a single pair of wrapping quotes some models add around the reply.
/// Quotes inside the text are left alone.
fn strip_wrapping_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Drop a leading "Transcript:" label some models prepend to the reply.
/// The label would otherwise inject as a phantom word. Case-insensitive,
/// colon required; "Transcription" and bare "transcript" are left alone.
fn strip_leading_label(s: &str) -> &str {
    let trimmed = s.trim_start();
    let prefix_len = "transcript:".len();
    match trimmed.get(..prefix_len) {
        Some(head) if head.eq_ignore_ascii_case("transcript:") && trimmed.len() > prefix_len => {
            trimmed[prefix_len..].trim_start()
        }
        _ => s,
    }
}

/// Lowercase alphanumeric words, so "Hello," and "hello" compare equal.
fn norm_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Minimum word F1 for a cleanup to count as punctuation-only.
/// The old rule (intersection over the longer side at 4/5) is exactly
/// F1 at 0.8 for equal-length texts; F1 generalizes it to additions
/// and deletions instead of special-casing length first.
pub const F1_MINIMUM: f64 = 0.8;

/// True when `cleaned` keeps the words of `raw`: punctuation-only edits
/// score 1.0, paraphrases collapse toward 0. Punctuation and case never
/// change the score, so a faithful model always passes and a rewriting
/// model trips the regex fallback above.
fn preserves_words(raw: &str, cleaned: &str) -> bool {
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

/// Request body for one Ollama `/api/chat` call, extracted for tests.
///
/// Speed notes: `keep_alive` holds the model resident for a dictation
/// session so repeat polishes skip the ~1s CPU reload; `num_predict`
/// caps rambling generations; `num_ctx` stays small because utterances
/// are short. Temperature stays 0: determinism is also a fidelity fix.
/// `think` stays false: thinking models would burn the token budget on
/// chain-of-thought and return an empty reply; the flag is ignored by
/// models without thinking support.
fn request_body(model: &str, prompt: &str) -> String {
    serde_json::json!({
        "model": model,
        "stream": false,
        "think": false,
        "keep_alive": "30m",
        "options": {"temperature": 0, "num_predict": 256, "num_ctx": 1024},
        "messages": [{"role": "user", "content": prompt}],
    })
    .to_string()
}

/// Minimal Ollama `/api/chat` call via curl (no HTTP deps in v0.1.0).
/// Returns the assistant message content. Shape and options live in
/// `request_body`.
fn chat_once(endpoint: &str, model: &str, prompt: &str) -> Result<String, String> {
    let body = request_body(model, prompt);
    let out = susurro_core::silent_command("curl")
        .args([
            "-sS",
            "-m",
            "60",
            &format!("{endpoint}/api/chat"),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
        ])
        .output()
        .map_err(|e| format!("Couldn't run curl (is curl installed?): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ollama HTTP failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("ollama returned non-JSON (is the server up?): {e}"))?;
    // Shape: {"message": {"content": "..."}, ...} or {"error": "..."}.
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("ollama error (model pulled?): {err}"));
    }
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "ollama response had no message.content".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_collapses_and_tidies() {
        assert_eq!(
            RegexCleanup.cleanup("  hello   world ").unwrap(),
            "hello world"
        );
        assert_eq!(
            RegexCleanup.cleanup("hello ,  world .").unwrap(),
            "hello, world."
        );
        assert_eq!(RegexCleanup.cleanup("wait.. what").unwrap(), "wait. what");
    }

    #[test]
    fn ollama_falls_back_when_server_down() {
        // Nothing listens on port 1: must fail open to regex output.
        let bad = OllamaCleanup {
            model: "qwen3:0.6b".into(),
            endpoint: "http://127.0.0.1:1".into(),
        };
        assert_eq!(bad.cleanup("  hello   world ").unwrap(), "hello world");
    }

    #[test]
    fn prompt_preserves_transcript() {
        let p = OllamaCleanup::prompt("i try to test susura");
        assert!(p.contains("i try to test susura"));
        assert!(p.contains("forbidden"));
    }

    #[test]
    fn request_disables_thinking() {
        // Thinking models burn the token budget on chain-of-thought
        // and return empty; the flag is ignored elsewhere.
        let body = request_body("qwen3:0.6b", "hi");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v.get("think").and_then(|t| t.as_bool()), Some(false));
        assert_eq!(v.get("model").and_then(|m| m.as_str()), Some("qwen3:0.6b"));
    }

    #[test]
    fn leading_label_strips_but_lookalikes_stay() {
        assert_eq!(
            strip_leading_label("Transcript: Hey can you send it?"),
            "Hey can you send it?"
        );
        assert_eq!(strip_leading_label("transcript:hey"), "hey");
        assert_eq!(strip_leading_label("  Transcript:  spaced"), "spaced");
        // Bare word, longer words, and empty input are content, not labels.
        assert_eq!(strip_leading_label("transcript"), "transcript");
        assert_eq!(strip_leading_label("Transcript:"), "Transcript:");
        assert_eq!(
            strip_leading_label("Transcription: notes"),
            "Transcription: notes"
        );
        assert_eq!(strip_leading_label(""), "");
        assert_eq!(
            strip_leading_label("héllo transcript: x"),
            "héllo transcript: x"
        );
    }

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
        // Paraphrase: most words replaced.
        assert!(!preserves_words(
            "the quick brown fox jumps",
            "a fast dark fox leaps high over everything today"
        ));
        // Added sentence.
        assert!(!preserves_words(
            "buy milk",
            "Buy milk. Also, call your mother about dinner."
        ));
        // Emptied reply.
        assert!(!preserves_words("buy milk", "  "));
    }

    #[test]
    fn quotes_do_not_trip_the_guard() {
        let out = strip_wrapping_quotes("\"Hello world.\"");
        assert!(preserves_words("hello world", out));
    }

    #[test]
    fn f1_scores_precision_and_recall() {
        assert_eq!(word_f1(2, 2, 2), 1.0);
        assert_eq!(word_f1(0, 2, 2), 0.0);
        // Added words hurt precision: 5 kept of 5 with 2 added.
        assert!(word_f1(5, 7, 5) > F1_MINIMUM);
        assert!(word_f1(4, 7, 4) < F1_MINIMUM);
        // Dropped words hurt recall: 3 kept of 6 drops below the bar.
        assert!(word_f1(3, 3, 6) < F1_MINIMUM);
    }

    #[test]
    fn added_sentence_trips_f1() {
        // Same length-budget edge the old rule also caught, now by F1.
        assert!(!preserves_words(
            "buy milk",
            "Buy milk. Also, call your mother about dinner."
        ));
    }
}
