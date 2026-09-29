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
    /// e.g. "qwen2.5:0.5b".
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
                let text = strip_wrapping_quotes(text.trim());
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

/// Lowercase alphanumeric words, so "Hello," and "hello" compare equal.
fn norm_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

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
    hit * 5 >= c.len().max(r.len()) * 4
}

/// Minimal Ollama `/api/chat` call via curl (no HTTP deps in v0.1.0).
/// Returns the assistant message content.
///
/// Speed notes: `keep_alive` holds the model resident for a dictation
/// session so repeat polishes skip the ~1s CPU reload; `num_predict`
/// caps rambling generations; `num_ctx` stays small because utterances
/// are short. Temperature stays 0: determinism is also a fidelity fix.
fn chat_once(endpoint: &str, model: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "keep_alive": "30m",
        "options": {"temperature": 0, "num_predict": 256, "num_ctx": 1024},
        "messages": [{"role": "user", "content": prompt}],
    })
    .to_string();
    let out = std::process::Command::new("curl")
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
            model: "qwen2.5:0.5b".into(),
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
}
