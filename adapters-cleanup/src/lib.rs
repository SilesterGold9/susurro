//! Cleanup adapters.
//!
//! - `PassthroughCleanup`: v0.0.1 raw path.
//! - `RegexCleanup`: dependency-free fallback (whitespace, spacing).
//! - `PunctuateCleanup`: the default (ADR-004 Phase 4). Bundled ONNX
//!   punctuation, on device, no server to install.
//! - `OllamaCleanup`: opt-in rewrite tier for users who already run
//!   Ollama and want a real model behind the polish step.
//!
//! Every tier fails open to the regex tidier, so dictation never waits
//! on cleanup.

mod punctuate;

pub use punctuate::PunctuateCleanup;

use susurro_core::ports::TextPostProcessorPort;
use susurro_core::preserves_words;

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

/// Whitespace and spacing tidier. Public because it is the last link
/// in every fail-open chain, not only the `RegexCleanup` tier.
pub fn regex_cleanup_text(raw: &str) -> String {
    cleanup_whitespace(raw)
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

/// Word F1 from intersection size over cleaned and raw word counts.
/// Precision punishes added words, recall punishes dropped words.
/// Re-exported from core so every caller shares one definition.
pub use susurro_core::word_f1;

/// Model files for the `onnx` tier, resolved by the caller because the
/// provisioning plane owns asset names.
#[derive(Debug, Clone)]
pub struct PunctPaths {
    pub model: std::path::PathBuf,
    pub vocab: std::path::PathBuf,
}

/// Build the cleaner for a tier name. One place decides the chain, so
/// the CLI, the app, and the tests cannot disagree about what `onnx`
/// means or which names exist.
///
/// Tiers: `onnx` (default, bundled punctuation), `ollama` (opt-in
/// rewrite tier), `regex` (tidier only), `none` (raw transcript).
pub fn by_name(
    tier: &str,
    punct: Option<PunctPaths>,
    ollama_model: &str,
) -> Result<Box<dyn TextPostProcessorPort>, String> {
    Ok(match tier {
        "onnx" => {
            let paths = punct.ok_or_else(|| {
                "no models dir on this machine, so the bundled punctuation model cannot load. \
                 Use --cleanup regex or none."
                    .to_string()
            })?;
            Box::new(PunctuateCleanup::new(paths.model, paths.vocab))
        }
        "ollama" => Box::new(OllamaCleanup::new(ollama_model)),
        "regex" => Box::new(RegexCleanup),
        "none" => Box::new(PassthroughCleanup),
        other => {
            return Err(format!(
                "unknown cleanup tier '{other}'. Use none, regex, onnx, or ollama."
            ))
        }
    })
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
    use susurro_core::F1_MINIMUM;

    #[test]
    fn tiers_resolve_and_bad_names_are_rejected() {
        let dir = std::env::temp_dir();
        let punct = Some(PunctPaths {
            model: dir.join("punct-model.onnx"),
            vocab: dir.join("punct-model.vocab"),
        });
        // Construction never touches the disk, so every tier resolves
        // even with no model present: the engine fails open at call
        // time, not at build time.
        for tier in ["onnx", "ollama", "regex", "none"] {
            assert!(by_name(tier, punct.clone(), "qwen3:0.6b").is_ok(), "{tier}");
        }
        let err = by_name("nope", punct.clone(), "qwen3:0.6b")
            .err()
            .expect("unknown tier must not resolve");
        assert!(err.contains("unknown cleanup tier"), "{err}");
        assert!(err.contains("onnx"), "{err}");
    }

    #[test]
    fn onnx_without_a_models_dir_says_what_to_do() {
        let err = by_name("onnx", None, "qwen3:0.6b")
            .err()
            .expect("no models dir must not resolve");
        assert!(err.contains("--cleanup regex"), "{err}");
    }

    #[test]
    fn the_onnx_tier_fails_open_with_no_model() {
        let dir = std::env::temp_dir();
        let punct = Some(PunctPaths {
            model: dir.join("absent-punct.onnx"),
            vocab: dir.join("absent-punct.vocab"),
        });
        let cleaner = by_name("onnx", punct, "qwen3:0.6b").unwrap();
        assert_eq!(cleaner.cleanup("  hello   world ").unwrap(), "hello world");
    }

    #[test]
    fn none_is_the_raw_transcript() {
        let cleaner = by_name("none", None, "qwen3:0.6b").unwrap();
        assert_eq!(
            cleaner.cleanup("  hello   world ").unwrap(),
            "  hello   world "
        );
    }

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
