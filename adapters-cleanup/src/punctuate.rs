//! ONNX punctuation restoration (ADR-004 Phase 4).
//!
//! Whisper emits lowercase, unpunctuated text. This adapter restores
//! sentence punctuation and capitalisation on device with the int8
//! CNN-BiLSTM English model, which replaces the "install Ollama and
//! pull a 400 MB LLM" first-run story with 7.6 MB of files that ship
//! with the installer.
//!
//! It fails open, exactly like [`super::OllamaCleanup`]: a missing
//! model, a model that will not load, a native error, or output that
//! changed the speaker's words all end at the regex tidier. Dictation
//! never waits on punctuation.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sherpa_onnx::{OnlinePunctuation, OnlinePunctuationConfig, OnlinePunctuationModelConfig};
use susurro_core::ports::TextPostProcessorPort;
use susurro_core::preserves_words;

use crate::regex_cleanup_text;

/// One loaded native session plus the files it came from, so new
/// weights on disk reload instead of being silently ignored.
struct Loaded {
    model: PathBuf,
    vocab: PathBuf,
    engine: OnlinePunctuation,
}

/// Process-wide session cache. Loading weights plus creating a session
/// costs real time and every utterance would pay it again. The lock is
/// held across the call: sherpa-onnx documents one object as safe for
/// single-object use, and dictation is already serialised by the
/// in-flight guard, so a second concurrent cleaner is a bug elsewhere.
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// Punctuation restoration backed by the native ONNX engine.
pub struct PunctuateCleanup {
    model: PathBuf,
    vocab: PathBuf,
}

impl PunctuateCleanup {
    /// Point at the model pair. File names come from the manifest, so
    /// this adapter only takes paths.
    pub fn new(model: impl Into<PathBuf>, vocab: impl Into<PathBuf>) -> Self {
        Self {
            model: model.into(),
            vocab: vocab.into(),
        }
    }

    /// Load once, reuse thereafter, reload when the paths change.
    /// `None` means the caller gets the regex fallback.
    fn punctuate(&self, text: &str) -> Option<String> {
        let mut guard = LOADED.lock().ok()?;
        let stale = match guard.as_ref() {
            Some(loaded) => loaded.model != self.model || loaded.vocab != self.vocab,
            None => true,
        };
        if stale {
            *guard = Some(Loaded {
                model: self.model.clone(),
                vocab: self.vocab.clone(),
                engine: load_engine(&self.model, &self.vocab)?,
            });
        }
        guard.as_ref()?.engine.add_punctuation(text)
    }
}

impl TextPostProcessorPort for PunctuateCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        Ok(accept_or_fallback(raw, self.punctuate(raw)))
    }
}

/// The one rule the adapter exists to enforce: punctuation is allowed,
/// words are not. Kept separate from the engine so it is testable
/// without a model on disk, because it is the safety property.
fn accept_or_fallback(raw: &str, punctuated: Option<String>) -> String {
    match punctuated {
        // The model touched words. That is a rewrite, not
        // punctuation, so the speaker's own text wins.
        Some(text) if !preserves_words(raw, &text) => {
            eprintln!("punctuation changed words, using regex fallback");
            regex_cleanup_text(raw)
        }
        Some(text) => text,
        // Fail-open: no model, no native answer, no block.
        None => regex_cleanup_text(raw),
    }
}

/// Build a native session from a model pair. Missing files mean no
/// engine, never an error: the caller falls back.
fn load_engine(model: &Path, vocab: &Path) -> Option<OnlinePunctuation> {
    if !model.is_file() || !vocab.is_file() {
        return None;
    }
    let config = OnlinePunctuationConfig {
        model: OnlinePunctuationModelConfig {
            cnn_bilstm: Some(model.to_string_lossy().into_owned()),
            bpe_vocab: Some(vocab.to_string_lossy().into_owned()),
            num_threads: 1,
            debug: false,
            provider: Some("cpu".into()),
        },
    };
    OnlinePunctuation::create(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_and_case_pass_through() {
        assert_eq!(
            accept_or_fallback("hello world", Some("Hello, world.".into())),
            "Hello, world."
        );
        assert_eq!(
            accept_or_fallback("i try to test susura", Some("I try to test susura.".into())),
            "I try to test susura."
        );
    }

    #[test]
    fn changed_words_fall_back_to_the_regex_tidier() {
        // Paraphrase: most words replaced.
        assert_eq!(
            accept_or_fallback(
                "the quick brown fox jumps",
                Some("a fast dark fox leaps high".into())
            ),
            "the quick brown fox jumps"
        );
        // Added sentence.
        assert_eq!(
            accept_or_fallback("buy milk", Some("Buy milk. Also call.".into())),
            "buy milk"
        );
        // Emptied output is a silent failure, not a deletion.
        assert_eq!(
            accept_or_fallback("buy milk", Some("  ".into())),
            "buy milk"
        );
    }

    #[test]
    fn a_missing_model_fails_open() {
        let missing = std::env::temp_dir().join("susurro-no-such-punct-model.onnx");
        let cleaner = PunctuateCleanup::new(&missing, &missing);
        assert_eq!(cleaner.cleanup("  hello   world ").unwrap(), "hello world");
        assert!(load_engine(&missing, &missing).is_none());
    }

    #[test]
    fn a_half_present_pair_fails_open() {
        // The model without its vocabulary is unusable, and a partial
        // pair must not be treated as ready.
        let dir = std::env::temp_dir();
        let model = dir.join("susurro-punct-half-present.onnx");
        std::fs::write(&model, b"not a model").unwrap();
        let vocab = dir.join("susurro-punct-half-present.vocab");
        let cleaner = PunctuateCleanup::new(&model, &vocab);
        assert_eq!(cleaner.cleanup("hello world").unwrap(), "hello world");
        let _ = std::fs::remove_file(&model);
    }
}
