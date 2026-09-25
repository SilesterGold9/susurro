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
            sentence sounds odd. Never explain. Output the transcript and nothing else.\n\
            Transcript: {raw}"
        )
    }
}

impl TextPostProcessorPort for OllamaCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        match chat_once(&self.endpoint, &self.model, &Self::prompt(raw)) {
            Ok(text) if !text.trim().is_empty() => Ok(text.trim().to_string()),
            // Fail-open per plan: regex fallback, never block injection.
            _ => Ok(cleanup_whitespace(raw)),
        }
    }
}

/// Minimal Ollama `/api/chat` call via curl (no HTTP deps in v0.1.0).
/// Returns the assistant message content.
fn chat_once(endpoint: &str, model: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "options": {"temperature": 0},
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
}
