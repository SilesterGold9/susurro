//! Cloud STT adapters — stub until v0.3.0.
//!
//! The plan calls for one generic `OpenAiCompatibleAdapter`
//! (Groq, NVIDIA NIM, future providers) with circuit breaker +
//! rate-limit-aware fallback. That lives here in v0.3.0.
//! v0.0.1 compiles the crate so CI guards the shape early.

use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

pub struct UnconfiguredCloud {
    pub provider: &'static str,
}

impl SpeechToTextPort for UnconfiguredCloud {
    fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
        Err(CoreError::Transcription(format!(
            "{} not configured until v0.3.0. Using local instead.",
            self.provider
        )))
    }
    fn model_name(&self) -> &str {
        "unconfigured-cloud"
    }
}
