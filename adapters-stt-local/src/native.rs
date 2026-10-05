//! In-process whisper backend (Phase 1, ADR-004).
//!
//! `WhisperNative` links whisper.cpp through whisper-rs and decodes
//! without a child process: no `PATH` lookup, no binary install, no
//! per-decode spawn cost. The model loads once per instance behind a
//! mutex; every decode reuses it. CPU only by choice: GPU backends
//! stay future work behind the shell-out escape hatch.
//!
//! Parameter parity with the `whisper-cli` default path: English,
//! beam search (5, patience -1), no-speech threshold 0.6, dictionary
//! prompt as the initial prompt, all progress printing silenced.
//! Transcripts join segments exactly like the binary stdout path, so
//! the blank-output gate in `transcribe` behaves identically.

use std::path::PathBuf;
use std::sync::Mutex;
use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;
use whisper_rs::{
    convert_integer_to_float_audio, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

/// Linked whisper.cpp version, for doctor lines and diagnostics.
pub fn linked_version() -> &'static str {
    whisper_rs::get_whisper_version()
}

/// Threads for decode: explicit count, or the machine default.
/// Zero or negative readings fall back to 4, never to 1.
fn decode_threads(want: usize) -> i32 {
    if want > 0 {
        return want.min(i32::MAX as usize) as i32;
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(i32::MAX as usize) as i32
}

struct LoadedCtx {
    path: PathBuf,
    ctx: WhisperContext,
}

pub struct WhisperNative {
    pub model_path: PathBuf,
    /// Initial prompt for vocabulary boosting (dictionary phrases).
    pub prompt: Option<String>,
    /// Decode language. English by default; auto-detect (issue 61)
    /// plugs in here when the flag lands.
    pub language: String,
    /// Worker threads, 0 means the machine default.
    pub threads: usize,
    /// No-speech threshold, matching the binary default 0.6.
    pub no_speech_threshold: f32,
    ctx: Mutex<Option<LoadedCtx>>,
}

impl WhisperNative {
    pub fn base_en(model_path: PathBuf) -> Self {
        Self {
            model_path,
            prompt: None,
            language: "en".into(),
            threads: 0,
            no_speech_threshold: 0.6,
            ctx: Mutex::new(None),
        }
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        if !prompt.trim().is_empty() {
            self.prompt = Some(prompt.trim().into());
        }
        self
    }

    /// Load the model on first use, reload when the path changed
    /// under the instance. Holds the lock only while borrowing the
    /// context for the decode that follows.
    fn with_ctx<T>(&self, f: impl FnOnce(&mut WhisperContext) -> T) -> Result<T, CoreError> {
        if !self.model_path.exists() {
            return Err(CoreError::Transcription(format!(
                "model not found at {}. Fetch it with `susurro model-fetch`, or run `susurro doctor`.",
                self.model_path.display()
            )));
        }
        let mut guard = self
            .ctx
            .lock()
            .map_err(|e| CoreError::Transcription(format!("decoder lock poisoned: {e}")))?;
        let reload = guard
            .as_ref()
            .is_none_or(|loaded| loaded.path != self.model_path);
        if reload {
            let ctx = WhisperContext::new_with_params(
                &self.model_path,
                WhisperContextParameters::default(),
            )
            .map_err(|e| CoreError::Transcription(format!("model failed to load: {e:?}")))?;
            *guard = Some(LoadedCtx {
                path: self.model_path.clone(),
                ctx,
            });
        }
        Ok(f(&mut guard.as_mut().expect("context just loaded").ctx))
    }

    /// Raw decode without the no-speech gate. Mirrors
    /// `WhisperLocal::decode_text`: empty audio errors, model
    /// absence names the fetch, blank output is the caller's check.
    pub fn decode_text(&self, pcm: &[i16]) -> Result<String, CoreError> {
        if pcm.is_empty() {
            return Err(CoreError::Transcription("empty audio".into()));
        }
        self.with_ctx(|ctx| {
            let mut state = ctx
                .create_state()
                .map_err(|e| CoreError::Transcription(format!("decoder state failed: {e:?}")))?;
            let mut params = FullParams::new(SamplingStrategy::BeamSearch {
                beam_size: 5,
                patience: -1.0,
            });
            params.set_language(Some(&self.language));
            params.set_n_threads(decode_threads(self.threads));
            params.set_no_speech_thold(self.no_speech_threshold);
            if let Some(prompt) = self.prompt.as_deref() {
                params.set_initial_prompt(prompt);
            }
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            let mut audio = vec![0.0f32; pcm.len()];
            convert_integer_to_float_audio(pcm, &mut audio)
                .map_err(|e| CoreError::Transcription(format!("audio conversion failed: {e:?}")))?;
            state
                .full(params, &audio)
                .map_err(|e| CoreError::Transcription(format!("decode failed: {e:?}")))?;
            let mut text = String::new();
            for segment in state.as_iter() {
                text.push_str(&segment.to_string());
            }
            Ok(text.trim().to_string())
        })?
    }
}

impl SpeechToTextPort for WhisperNative {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        let text = self.decode_text(pcm)?;
        if crate::is_blank_transcript(&text) {
            return Err(CoreError::Transcription(
                "heard only silence, nothing injected. Speak during the recording window.".into(),
            ));
        }
        Ok(Transcript {
            text,
            is_partial: false,
        })
    }

    fn model_name(&self) -> &str {
        self.model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("native")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_names_the_fetch() {
        let w = WhisperNative::base_en(PathBuf::from("/nonexistent/base.en.bin"));
        let err = w.transcribe(&[1, 2, 3]).unwrap_err().to_string();
        assert!(err.contains("model not found"), "{err}");
        assert!(err.contains("model-fetch"), "{err}");
    }

    #[test]
    fn empty_audio_errors_before_touching_the_model() {
        let w = WhisperNative::base_en(PathBuf::from("/nonexistent/base.en.bin"));
        let err = w.transcribe(&[]).unwrap_err().to_string();
        assert!(err.contains("empty audio"), "{err}");
    }

    #[test]
    fn model_name_follows_the_file() {
        let w = WhisperNative::base_en(PathBuf::from("/models/tiny.en.bin"));
        assert_eq!(w.model_name(), "tiny.en");
    }

    #[test]
    fn thread_count_never_collapses_to_serial() {
        assert!(decode_threads(0) >= 1);
        assert_eq!(decode_threads(8), 8);
    }

    /// Real decode over the linked engine, gated on purpose: it
    /// needs the tiny model plus a 16kHz mono sample on disk, and
    /// must never run (or download) in CI. Set both env vars to run:
    /// SUSURRO_ACCURACY_MODEL=/path/tiny.en.bin
    /// SUSURRO_ACCURACY_WAV=/path/jfk.wav (16kHz mono s16)
    #[test]
    fn jfk_decodes_to_english() {
        let (model, wav) = match (
            std::env::var("SUSURRO_ACCURACY_MODEL").ok(),
            std::env::var("SUSURRO_ACCURACY_WAV").ok(),
        ) {
            (Some(m), Some(w)) => (m, w),
            _ => {
                eprintln!(
                    "accuracy proof skipped: set SUSURRO_ACCURACY_MODEL and SUSURRO_ACCURACY_WAV"
                );
                return;
            }
        };
        let bytes = std::fs::read(&wav).expect("sample wav readable");
        assert!(bytes.len() > 44, "sample has a wav header");
        let pcm: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        let w = WhisperNative::base_en(PathBuf::from(model));
        let text = w.decode_text(&pcm).expect("native decode runs");
        let lower = text.to_lowercase();
        assert!(
            lower.contains("fellow americans"),
            "jfk sample recognized, got: {text}"
        );
    }
}
