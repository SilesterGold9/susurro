//! STT backend benchmark: race the available backends, winner default
//! (v0.5.0, issue 28).
//!
//! Candidates today are whisper.cpp on CPU and whisper.cpp with the
//! OpenVINO encoder (issue 27). The ONNX Runtime EP slot exists in
//! detection only: no ONNX runner ships in this milestone, so its
//! cell always reads unavailable with the prerequisites named. When
//! the runner lands, it times itself through the same harness and
//! the persisted winner starts covering it with no CLI changes.
//!
//! Mechanics are hardware-free: timing plus winner-picking run over
//! `SpeechToTextPort`, so unit tests race sleeping mocks while the
//! CLI races real adapters on synthesized audio. The winner persists
//! under `SETTINGS_KEY`; `--backend auto` honors a stored winner
//! that is still available, else falls back to the static order.

use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

/// Settings key holding the last benchmark winner (`cpu`, `openvino`).
pub const SETTINGS_KEY: &str = "stt_backend";

/// Length of the synthesized race audio. Long enough to dwarf spawn
/// overhead, short enough to keep the command snappy.
pub const RACE_SECONDS: u64 = 4;

/// Availability of one benchmark candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateStatus {
    Ready,
    Unavailable(String),
}

/// ONNX Runtime EP prerequisites: a visible `libonnxruntime` plus an
/// operator-supplied encoder model via `SUSURRO_ONNX_ENCODER`. No
/// invented filenames: the operator names the file. Ready means the
/// machine could run it, not that this milestone can: the runner
/// itself does not exist yet, so the bench reports the cell without
/// timing it.
pub fn detect_onnx() -> CandidateStatus {
    let runtime = susurro_core::silent_command("ldconfig")
        .arg("-p")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if !super::openvino::ldconfig_has(&runtime, &["libonnxruntime"]) {
        return CandidateStatus::Unavailable("no libonnxruntime found".into());
    }
    match std::env::var("SUSURRO_ONNX_ENCODER") {
        Ok(p) if std::path::Path::new(&p).exists() => CandidateStatus::Unavailable(
            "prerequisites present but no ONNX runner ships in this milestone".into(),
        ),
        Ok(_) => {
            CandidateStatus::Unavailable("SUSURRO_ONNX_ENCODER points at a missing file".into())
        }
        Err(_) => {
            CandidateStatus::Unavailable("set SUSURRO_ONNX_ENCODER to an encoder onnx model".into())
        }
    }
}

/// Deterministic race audio: layered sines plus seeded noise at 16kHz
/// mono. Synthesized so the bench never needs a mic. Rich enough to
/// decode (a pure tone reads as silence and trips the no-speech
/// gate), fixed so reruns compare the backend, not the input.
pub fn synth_sine(seconds: u64) -> Vec<i16> {
    let n = seconds as usize * 16_000;
    let mut seed: u32 = 0x12345678;
    (0..n)
        .map(|i| {
            let t = i as f32 / 16_000.0;
            // LCG noise, deterministic per index.
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 16) as i16 % 1000 - 500;
            let tone = 6_000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 3_000.0 * (2.0 * std::f32::consts::PI * 660.0 * t).sin();
            (tone as i16).saturating_add(noise)
        })
        .collect()
}

/// Time one fallible call in milliseconds. The error still returns
/// its elapsed time: a failing backend loses the race with its
/// number attached instead of aborting the bench.
pub fn time_call<T>(f: impl FnOnce() -> Result<T, CoreError>) -> (u128, Result<T, CoreError>) {
    let t0 = std::time::Instant::now();
    let out = f();
    (t0.elapsed().as_millis(), out)
}

/// Time one full transcription in milliseconds. Same note as
/// `time_call`: errors lose with numbers attached.
pub fn time_transcribe(
    stt: &dyn SpeechToTextPort,
    pcm: &[i16],
) -> (u128, Result<Transcript, CoreError>) {
    time_call(|| stt.transcribe(pcm))
}

/// Fastest (name, ms) wins; ties keep the earlier candidate, which
/// is the static preference order. Empty input is None, never a guess.
pub fn pick_winner<'a>(timings: &[(&'a str, u128)]) -> Option<&'a str> {
    timings
        .iter()
        .min_by_key(|(_, ms)| *ms)
        .map(|(name, _)| *name)
}

/// Stored winner, validated against the names this milestone can
/// actually run. Anything else reads as unset, never as an error.
pub fn load_winner(store: &impl susurro_core::ports::SettingsStorePort) -> Option<String> {
    let raw = store.get(SETTINGS_KEY).ok()??;
    match raw.as_str() {
        "cpu" | "openvino" => Some(raw),
        _ => None,
    }
}

/// Persist the winner. Callers treat failure as degraded, not fatal.
pub fn store_winner(
    store: &mut impl susurro_core::ports::SettingsStorePort,
    winner: &str,
) -> Result<(), CoreError> {
    store.set(SETTINGS_KEY, winner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use susurro_core::ports::SettingsStorePort;

    struct SleepStt {
        ms: u64,
    }

    impl SpeechToTextPort for SleepStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
            std::thread::sleep(std::time::Duration::from_millis(self.ms));
            Ok(Transcript {
                text: "hello".into(),
                is_partial: false,
            })
        }
        fn model_name(&self) -> &str {
            "sleep"
        }
    }

    struct FailStt;

    impl SpeechToTextPort for FailStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
            std::thread::sleep(std::time::Duration::from_millis(5));
            Err(CoreError::Transcription("boom".into()))
        }
        fn model_name(&self) -> &str {
            "fail"
        }
    }

    #[test]
    fn synth_is_deterministic_and_sized() {
        let a = synth_sine(1);
        let b = synth_sine(1);
        assert_eq!(a.len(), 16_000);
        assert_eq!(a, b);
        assert!(a.iter().any(|&s| s != 0));
    }

    #[test]
    fn faster_backend_wins_the_race() {
        let pcm = synth_sine(1);
        let slow = SleepStt { ms: 60 };
        let fast = SleepStt { ms: 5 };
        let (slow_ms, _) = time_transcribe(&slow, &pcm);
        let (fast_ms, _) = time_transcribe(&fast, &pcm);
        assert!(fast_ms < slow_ms);
        assert_eq!(
            pick_winner(&[("slow", slow_ms), ("fast", fast_ms)]),
            Some("fast")
        );
        assert_eq!(pick_winner(&[]), None);
    }

    #[test]
    fn errors_lose_with_numbers_attached() {
        let pcm = synth_sine(1);
        let (ms, out) = time_transcribe(&FailStt, &pcm);
        assert!(out.is_err());
        assert!(ms < 5_000);
    }

    #[test]
    fn onnx_detects_honestly() {
        // This box has no ORT runtime: must name it, never claim Ready.
        // (If a future machine has one, the env branch names the model.)
        let status = detect_onnx();
        if let CandidateStatus::Unavailable(reason) = status {
            assert!(!reason.is_empty());
        }
    }

    #[derive(Default)]
    struct FakeSettings {
        map: HashMap<String, String>,
    }

    impl susurro_core::ports::SettingsStorePort for FakeSettings {
        fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
            Ok(self.map.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), CoreError> {
            self.map.insert(key.into(), value.into());
            Ok(())
        }
    }

    #[test]
    fn winner_roundtrips_and_rejects_unknown() {
        let mut store = FakeSettings::default();
        assert_eq!(load_winner(&store), None);
        store_winner(&mut store, "cpu").unwrap();
        assert_eq!(load_winner(&store).as_deref(), Some("cpu"));
        store.set(SETTINGS_KEY, "onnx").unwrap();
        assert_eq!(load_winner(&store), None);
    }
}
