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
    /// Initial prompt for vocabulary boosting (dictionary phrases).
    pub prompt: Option<String>,
    /// whisper.cpp no-speech threshold: segments scoring above this as
    /// non-speech are suppressed by the binary. Default 0.6 matches
    /// whisper.cpp. Blank output is rejected below regardless.
    pub no_speech_threshold: f32,
}

impl WhisperLocal {
    pub fn base_en(model_path: PathBuf) -> Self {
        Self {
            model_path,
            binary: "whisper-cli".into(),
            prompt: None,
            no_speech_threshold: 0.6,
        }
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        if !prompt.trim().is_empty() {
            self.prompt = Some(prompt.trim().into());
        }
        self
    }

    /// Command for one transcription, extracted for shape tests.
    /// NOTE: no --output-txt, that writes a sidecar file and leaves
    /// stdout empty. --no-prints + -nt keeps stdout to transcript only.
    fn command(&self, wav_path: &std::path::Path) -> Command {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-m")
            .arg(&self.model_path)
            .arg("-f")
            .arg(wav_path)
            .arg("-l")
            .arg("en")
            .arg("--no-prints")
            .arg("-nt")
            .arg("--no-speech-thold")
            .arg(self.no_speech_threshold.to_string());
        if let Some(p) = self.prompt.as_deref() {
            cmd.arg("--prompt").arg(p);
        }
        cmd
    }
}

/// True when the transcript holds nothing worth injecting: empty,
/// whitespace, or a bracketed non-speech tag like [BLANK_AUDIO].
pub fn is_blank_transcript(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    t.starts_with('[') && t.ends_with(']') || t.starts_with('(') && t.ends_with(')')
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
        // v0.0.1: write 16kHz mono S16 WAV to a temp file for the
        // external binary. Native binding replaces this in later milestones.
        let tmp = std::env::temp_dir().join(format!(
            "susurro-{}.wav",
            susurro_core::SessionId::generate()
        ));
        let wav = encode_wav_16k_mono(pcm);
        std::fs::write(&tmp, &wav)
            .map_err(|e| CoreError::Transcription(format!("temp write failed: {e}")))?;
        // NOTE: no --output-txt — that writes a sidecar file and leaves
        // stdout empty. --no-prints + -nt keeps stdout to transcript only.
        let out = self.command(&tmp).output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                // No-speech gate: silence must never inject an empty string
                // or a hallucinated tag into the app and history.
                if is_blank_transcript(&text) {
                    return Err(CoreError::Transcription(
                        "heard only silence, nothing injected. Speak during the recording window."
                            .into(),
                    ));
                }
                Ok(Transcript {
                    text,
                    is_partial: false,
                })
            }
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

/// Minimal 16kHz mono S16 WAV encoder (44-byte header, no deps).
fn encode_wav_16k_mono(pcm: &[i16]) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
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

    #[test]
    fn wav_header_is_valid() {
        let wav = encode_wav_16k_mono(&[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 6);
        // Sample rate field.
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    }

    #[test]
    fn blank_transcripts_never_inject() {
        assert!(is_blank_transcript(""));
        assert!(is_blank_transcript("   "));
        assert!(is_blank_transcript("[BLANK_AUDIO]"));
        assert!(is_blank_transcript("(silence)"));
        assert!(!is_blank_transcript("hello world"));
        assert!(!is_blank_transcript("well (known) fact"));
    }

    #[test]
    fn command_carries_no_speech_gate() {
        let w = WhisperLocal::base_en(PathBuf::from("/models/base.en.bin"));
        let dbg = format!("{:?}", w.command(std::path::Path::new("/tmp/x.wav")));
        assert!(dbg.contains("--no-speech-thold"), "{dbg}");
        assert!(dbg.contains("0.6"), "{dbg}");
        assert!(!dbg.contains("--output-txt"), "{dbg}");
    }
}
