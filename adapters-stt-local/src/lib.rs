//! Local STT adapters (v0.0.1).
//!
//! - `MockStt`: deterministic transcript for tests/CI.
//! - `WhisperLocal`: shells out to a `whisper-cpp`/`whisper-cli` binary
//!   if present, else returns an actionable error telling the user
//!   to install the model (base.en). Native whisper-rs binding lands
//!   after v0.5.0 — this keeps CI light.
//! - `openvino` (v0.5.0): encoder offload to iGPU via `--ov-e-device`.
//!   The decoder always stays on CPU. Detection is best-effort and
//!   anything missing falls back to CPU with the reason named.
//! - `WindowedPartial` (v0.4.0): bounded-cost partial hypotheses for
//!   live feedback. Decodes at most the trailing window on a cadence,
//!   so extra CPU stays flat regardless of utterance length. Partials
//!   are display-only; the final full decode decides the transcript.
//! - `bench` (v0.4.0): hardware auto-benchmark to model tier
//!   selection. First run probes CPU throughput and persists the
//!   tier; later runs reuse it.

use std::path::PathBuf;
use std::process::Command;
use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

pub mod bench;
pub mod openvino;

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
    /// Compute backend. Cpu by default; OpenVino appends
    /// `--ov-e-device` so the encoder runs on the iGPU while the
    /// decoder stays on CPU.
    pub backend: openvino::SttBackend,
}

impl WhisperLocal {
    pub fn base_en(model_path: PathBuf) -> Self {
        Self {
            model_path,
            binary: "whisper-cli".into(),
            prompt: None,
            no_speech_threshold: 0.6,
            backend: openvino::SttBackend::Cpu,
        }
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        if !prompt.trim().is_empty() {
            self.prompt = Some(prompt.trim().into());
        }
        self
    }

    pub fn with_backend(mut self, backend: openvino::SttBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Concrete backend this instance decodes on.
    pub fn backend(&self) -> &openvino::SttBackend {
        &self.backend
    }

    /// Command for one transcription, extracted for shape tests.
    /// NOTE: no --output-txt, that writes a sidecar file and leaves
    /// stdout empty. --no-prints + -nt keeps stdout to transcript only.
    /// The OpenVINO flag is present only for that backend; CPU runs
    /// byte-identical to before.
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
        if let openvino::SttBackend::OpenVino { device } = &self.backend {
            cmd.arg("--ov-e-device").arg(device);
        }
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

/// Trailing decode window for partials, in samples at 16kHz.
/// Default 8s: enough context for a stable hypothesis, bounded cost
/// no matter how long the utterance runs.
pub const PARTIAL_WINDOW_SAMPLES: usize = 16_000 * 8;
/// New audio required between partials, in samples. Default 3s keeps
/// extra CPU near one window decode per 3s of speech.
pub const PARTIAL_MIN_NEW_SAMPLES: usize = 16_000 * 3;
/// Audio below this never produces a partial: whisper hallucinates
/// on tiny inputs, and the final decode covers short utterances.
pub const PARTIAL_MIN_SAMPLES: usize = 16_000 * 2;

/// Windowed partial decoder over a `WhisperLocal` config.
/// Call `partial` with the growing prefix during capture; it decodes
/// at most the trailing window on the cadence above and returns None
/// when there is nothing new worth the CPU. Blank output and binary
/// failures also yield None: partials are best-effort display, and the
/// final full decode reports real errors.
pub struct WindowedPartial {
    pub whisper: WhisperLocal,
    pub window_samples: usize,
    pub min_new_samples: usize,
    last_len: std::sync::Mutex<usize>,
}

impl WindowedPartial {
    pub fn new(whisper: WhisperLocal) -> Self {
        Self {
            whisper,
            window_samples: PARTIAL_WINDOW_SAMPLES,
            min_new_samples: PARTIAL_MIN_NEW_SAMPLES,
            last_len: std::sync::Mutex::new(0),
        }
    }

    /// Slice of `pcm` to decode, or None when cadence says wait.
    /// Pure for tests: `last_len` is the previously decoded length.
    pub fn window_bounds(
        total_len: usize,
        window_samples: usize,
        min_new_samples: usize,
        last_len: usize,
    ) -> Option<(usize, usize)> {
        if total_len < PARTIAL_MIN_SAMPLES {
            return None;
        }
        if total_len < last_len + min_new_samples {
            return None;
        }
        let start = total_len.saturating_sub(window_samples);
        Some((start, total_len))
    }

    pub fn partial(&self, pcm: &[i16]) -> Option<Result<Transcript, CoreError>> {
        let last = self.last_len.lock().map(|l| *l).unwrap_or(0);
        let (start, end) =
            Self::window_bounds(pcm.len(), self.window_samples, self.min_new_samples, last)?;
        if let Ok(mut l) = self.last_len.lock() {
            *l = end;
        }
        let text = match self.decode_window(&pcm[start..end]) {
            Some(Ok(t)) if !is_blank_transcript(&t) => t,
            _ => return None,
        };
        Some(Ok(Transcript {
            text,
            is_partial: true,
        }))
    }

    fn decode_window(&self, window: &[i16]) -> Option<Result<String, CoreError>> {
        if !self.whisper.model_path.exists() {
            return None;
        }
        let tmp = std::env::temp_dir().join(format!(
            "susurro-partial-{}.wav",
            susurro_core::SessionId::generate()
        ));
        if std::fs::write(&tmp, encode_wav_16k_mono(window)).is_err() {
            return None;
        }
        let out = self.whisper.command(&tmp).output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => {
                Some(Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()))
            }
            _ => None,
        }
    }
}

impl SpeechToTextPort for WindowedPartial {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        self.whisper.transcribe(pcm)
    }

    fn model_name(&self) -> &str {
        self.whisper.model_name()
    }

    fn transcribe_partial(&self, pcm: &[i16]) -> Option<Result<Transcript, CoreError>> {
        self.partial(pcm)
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

    #[test]
    fn cpu_command_carries_no_offload_flag() {
        let w = WhisperLocal::base_en(PathBuf::from("/models/base.en.bin"));
        let dbg = format!("{:?}", w.command(std::path::Path::new("/tmp/x.wav")));
        assert!(!dbg.contains("--ov-e-device"), "{dbg}");
    }

    #[test]
    fn openvino_command_offloads_encoder_only() {
        use crate::openvino::SttBackend;
        let w = WhisperLocal::base_en(PathBuf::from("/models/base.en.bin")).with_backend(
            SttBackend::OpenVino {
                device: "GPU".into(),
            },
        );
        let dbg = format!("{:?}", w.command(std::path::Path::new("/tmp/x.wav")));
        assert!(dbg.contains("--ov-e-device"), "{dbg}");
        assert!(dbg.contains("\"GPU\""), "{dbg}");
        assert_eq!(
            w.backend().describe(),
            "openvino (encoder on GPU, decoder on CPU)"
        );
    }

    #[test]
    fn window_bounds_gate_short_and_stale_audio() {
        use super::{WindowedPartial, PARTIAL_MIN_NEW_SAMPLES, PARTIAL_WINDOW_SAMPLES};
        // Under 2s never decodes.
        assert_eq!(
            WindowedPartial::window_bounds(
                16_000,
                PARTIAL_WINDOW_SAMPLES,
                PARTIAL_MIN_NEW_SAMPLES,
                0
            ),
            None
        );
        // 5s decodes from the start (window longer than audio).
        assert_eq!(
            WindowedPartial::window_bounds(
                80_000,
                PARTIAL_WINDOW_SAMPLES,
                PARTIAL_MIN_NEW_SAMPLES,
                0
            ),
            Some((0, 80_000))
        );
        // 10s decodes the trailing 8s window.
        assert_eq!(
            WindowedPartial::window_bounds(
                160_000,
                PARTIAL_WINDOW_SAMPLES,
                PARTIAL_MIN_NEW_SAMPLES,
                0
            ),
            Some((32_000, 160_000))
        );
        // Same length twice decodes once; 1 sample short of cadence waits.
        assert_eq!(
            WindowedPartial::window_bounds(
                80_000,
                PARTIAL_WINDOW_SAMPLES,
                PARTIAL_MIN_NEW_SAMPLES,
                80_000
            ),
            None
        );
        assert_eq!(
            WindowedPartial::window_bounds(
                80_000 + PARTIAL_MIN_NEW_SAMPLES - 1,
                PARTIAL_WINDOW_SAMPLES,
                PARTIAL_MIN_NEW_SAMPLES,
                80_000
            ),
            None
        );
        assert!(WindowedPartial::window_bounds(
            80_000 + PARTIAL_MIN_NEW_SAMPLES,
            PARTIAL_WINDOW_SAMPLES,
            PARTIAL_MIN_NEW_SAMPLES,
            80_000
        )
        .is_some());
    }

    #[test]
    fn partial_needs_no_binary_for_cadence() {
        use super::WindowedPartial;
        let w = WhisperLocal::base_en(PathBuf::from("/nonexistent/base.en.bin"));
        let decoder = WindowedPartial::new(w);
        // Short audio: None before any binary contact.
        assert!(decoder.partial(&vec![0; 16_000]).is_none());
        // Long audio with missing model: None, and the cadence holds
        // the same audio back on the immediate retry.
        assert!(decoder.partial(&vec![0; 80_000]).is_none());
        assert!(decoder.partial(&vec![0; 80_000]).is_none());
    }
}
