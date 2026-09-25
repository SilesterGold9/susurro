//! Local STT adapters (v0.0.1).
//!
//! - `MockStt`: deterministic transcript for tests/CI.
//! - `WhisperLocal`: shells out to a `whisper-cpp`/`whisper-cli` binary
//!   if present, else returns an actionable error telling the user
//!   to install the model (base.en). Native whisper-rs binding +
//!   OpenVINO offload land in v0.5.0 — this keeps v0.0.1 CI light.

use std::path::PathBuf;
use std::process::Command;
use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

pub struct MockStt {
    pub text: String,
}

impl MockStt {
    pub fn new(text: &str) -> Self {
        Self { text: text.into() }
    }
}

impl SpeechToTextPort for MockStt {
    fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
        Ok(Transcript {
            text: self.text.clone(),
            is_partial: false,
        })
    }
    fn model_name(&self) -> &str {
        "mock"
    }
}

pub struct WhisperLocal {
    pub model_path: PathBuf,
    pub binary: String,
}

impl WhisperLocal {
    pub fn base_en(model_path: PathBuf) -> Self {
        Self {
            model_path,
            binary: "whisper-cli".into(),
        }
    }
}

impl SpeechToTextPort for WhisperLocal {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        if pcm.is_empty() {
            return Err(CoreError::Transcription("empty audio".into()));
        }
        if !self.model_path.exists() {
            return Err(CoreError::Transcription(format!(
                "model not found at {}. Download base.en and set SUSURRO_MODEL, or run `susurro doctor`.",
                self.model_path.display()
            )));
        }
        // v0.0.1: stream raw PCM via temp file to external binary.
        // Native binding replaces this in later milestones.
        let tmp = std::env::temp_dir().join(format!(
            "susurro-{}.raw",
            susurro_core::SessionId::generate()
        ));
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(&tmp, &bytes)
            .map_err(|e| CoreError::Transcription(format!("temp write failed: {e}")))?;
        let out = Command::new(&self.binary)
            .arg("-m")
            .arg(&self.model_path)
            .arg("-f")
            .arg(&tmp)
            .arg("--output-txt")
            .output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => Ok(Transcript {
                text: String::from_utf8_lossy(&o.stdout).trim().to_string(),
                is_partial: false,
            }),
            Ok(o) => Err(CoreError::Transcription(format!(
                "whisper binary failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ))),
            Err(e) => Err(CoreError::Transcription(format!(
                "Couldn't run {}. Install whisper-cpp (base.en, CPU) or use mock in tests: {e}",
                self.binary
            ))),
        }
    }

    fn model_name(&self) -> &str {
        "base.en"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_fixed_text() {
        let m = MockStt::new("hello");
        let t = m.transcribe(&[0, 1, 2]).unwrap();
        assert_eq!(t.text, "hello");
    }

    #[test]
    fn missing_model_errors_actionably() {
        let w = WhisperLocal::base_en(PathBuf::from("/nonexistent/base.en.bin"));
        let err = w.transcribe(&[1, 2, 3]).unwrap_err().to_string();
        assert!(err.contains("model not found"), "{err}");
    }
}
