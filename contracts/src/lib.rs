//! Port contract suite (v0.7.0, issue 33).
//!
//! One `check_*` function per port trait, generic over the trait so
//! every adapter proves the same behavior. `contracts/tests/`
//! instantiates every adapter in tree hardware-free: mocks where
//! they exist, error paths (missing model, dead endpoint) where
//! hardware would be needed. A check that needs hardware does not
//! exist: anything unprovable on CI is documented at the check, not
//! skipped silently.
//!
//! Test doubles for ports with no scriptable real adapter live here
//! too (`MockHotkey`, `NullOverlay`); issue 34 promotes them into
//! first-class mock adapters.

use susurro_core::ports::{
    AudioCapturePort, GlobalHotkeyPort, HistoryStorePort, HotkeyEvent, NetworkState,
    NetworkStatusPort, OverlayRendererPort, OverlayState, SettingsStorePort, SpeechToTextPort,
    TextInjectionPort, TextPostProcessorPort,
};
use susurro_core::{CoreError, Ticket};

/// Immediate hotkey double: every wait is one press. Proves the
/// consumer side; real adapters block on hardware by design and are
/// covered only through their error paths.
pub struct MockHotkey;

impl GlobalHotkeyPort for MockHotkey {
    fn wait_for_hotkey(&self) -> Result<HotkeyEvent, CoreError> {
        Ok(HotkeyEvent::ToggleDictation)
    }
}

/// Overlay double that accepts everything without panicking,
/// including out-of-range amplitudes.
pub struct NullOverlay;

impl OverlayRendererPort for NullOverlay {
    fn render(&self, _state: OverlayState, _amplitude: f32) {}
}

/// Recording overlay for tests and CI: stores every render call as
/// a state plus amplitude pair behind a mutex. `takes` drains the
/// log so assertions read what the pipeline showed.
pub struct RecordingOverlay {
    seen: std::sync::Mutex<Vec<(OverlayState, f32)>>,
}

impl RecordingOverlay {
    /// Empty recording log.
    pub fn new() -> Self {
        Self {
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Drain recorded pairs, oldest first, leaving the log empty.
    pub fn takes(&self) -> Vec<(OverlayState, f32)> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

impl Default for RecordingOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRendererPort for RecordingOverlay {
    fn render(&self, state: OverlayState, amplitude: f32) {
        self.seen.lock().unwrap().push((state, amplitude));
    }
}

/// STT reports a transcript or an actionable error, never empty Ok
/// and never a partial flagged final. `pcm` is input, not fixture:
/// callers pass speech-like audio; mocks ignore it, real adapters
/// decode or fail on the missing model.
pub fn check_stt_reports_or_errors(stt: &dyn SpeechToTextPort, pcm: &[i16]) {
    match stt.transcribe(pcm) {
        Ok(t) => {
            assert!(!t.text.is_empty(), "empty transcript on Ok");
            assert!(!t.is_partial, "final transcribe flagged partial");
        }
        Err(e) => assert!(!e.to_string().is_empty(), "empty error text"),
    }
}

/// Model names are never empty: doctor, history, and logs print them.
pub fn check_stt_model_name(stt: &dyn SpeechToTextPort) {
    assert!(!stt.model_name().is_empty(), "empty model name");
}

/// A partial hypothesis, when offered, is flagged partial. None is a
/// legal answer for batch adapters; a final-flagged partial is not.
pub fn check_stt_partial_flag(stt: &dyn SpeechToTextPort, pcm: &[i16]) {
    if let Some(r) = stt.transcribe_partial(pcm) {
        let t = r.expect("partial errored");
        assert!(t.is_partial, "partial hypothesis flagged final");
    }
}

/// Capture protocol: start, one chunk, stop, all Ok. Fixture owns
/// the chunk content; the check owns the call order.
pub fn check_capture_protocol(cap: &mut dyn AudioCapturePort) {
    cap.start().expect("start failed");
    cap.next_chunk().expect("first chunk failed");
    cap.stop().expect("stop failed");
}

/// VAD separates loud speech-like frames from silence. Only
/// `is_speech` is contractual: `end_of_speech` depends on detector
/// history and stays implementation-tested.
pub fn check_vad_separates(vad: &dyn susurro_core::ports::VoiceActivityDetectorPort) {
    let loud: Vec<i16> = (0..480)
        .map(|i| {
            let t = i as f32 / 16_000.0;
            (8_000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as i16
        })
        .collect();
    assert!(vad.is_speech(&loud), "loud frame not speech");
    assert!(!vad.is_speech(&vec![0; 480]), "silence is speech");
}

/// Cleanup never errors, not even on empty input or a dead backend:
/// the fail-open ladder (model, regex, passthrough) always yields text.
pub fn check_cleanup_never_errors(cleanup: &dyn TextPostProcessorPort) {
    assert_eq!(cleanup.cleanup("").unwrap(), "");
    let out = cleanup.cleanup("  hello   world ").unwrap();
    assert!(!out.is_empty(), "cleanup emptied input");
}

/// Empty injection is a no-op on every platform, never a spawn and
/// never an error. Real text needs hardware and stays out of the suite.
pub fn check_inject_empty_ok(injector: &dyn TextInjectionPort) {
    let ticket = Ticket::new(susurro_core::SessionId::new(1), "inject");
    injector.inject("", &ticket).expect("empty inject failed");
}

/// Empty removal is a no-op on every platform, same contract as
/// empty injection. Real spans need the platform injector.
pub fn check_remove_last_empty_ok(injector: &dyn TextInjectionPort) {
    let ticket = Ticket::new(susurro_core::SessionId::new(2), "remove");
    injector
        .remove_last("", &ticket)
        .expect("empty remove failed");
}

/// Settings converge: set, overwrite, missing reads None.
pub fn check_settings_roundtrip(store: &mut dyn SettingsStorePort) {
    assert_eq!(store.get("contract-key").unwrap(), None);
    store.set("contract-key", "a").unwrap();
    assert_eq!(store.get("contract-key").unwrap().as_deref(), Some("a"));
    store.set("contract-key", "b").unwrap();
    assert_eq!(store.get("contract-key").unwrap().as_deref(), Some("b"));
}

/// Repeated upserts for one session converge without error.
/// Read-back is port-specific (Memory entries, Sqlite recent) and
/// stays in per-adapter tests; the contract pins convergence.
pub fn check_history_upsert_converges(store: &mut dyn HistoryStorePort) {
    use susurro_core::ports::HistoryEntry;
    let entry = |raw: &str| HistoryEntry {
        session: susurro_core::SessionId::new(9),
        raw_text: raw.into(),
        cleaned_text: None,
        provider: "contract".into(),
        latency_ms: 1,
        app: None,
        created_at: 0,
    };
    store.upsert(entry("one")).expect("first upsert failed");
    store.upsert(entry("two")).expect("second upsert failed");
}

/// Network status never panics and always resolves to a variant.
/// Callers force the answer with env overrides where supported.
pub fn check_network_resolves(net: &dyn NetworkStatusPort) {
    assert!(matches!(
        net.status(),
        NetworkState::Online | NetworkState::Offline
    ));
}

/// The mock hotkey answers immediately with a toggle press.
pub fn check_hotkey_mock_returns_toggle(hotkey: &dyn GlobalHotkeyPort) {
    assert!(matches!(
        hotkey.wait_for_hotkey().unwrap(),
        HotkeyEvent::ToggleDictation
    ));
}

/// The overlay accepts every state and any amplitude, including
/// out-of-range values, without panicking.
pub fn check_overlay_accepts_all(renderer: &dyn OverlayRendererPort) {
    for state in [
        OverlayState::Idle,
        OverlayState::Listening,
        OverlayState::Processing,
    ] {
        for amplitude in [0.0, 0.5, 1.0, 2.0, f32::NAN] {
            renderer.render(state, amplitude);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_overlay_records_renders() {
        let overlay = RecordingOverlay::new();
        overlay.render(OverlayState::Listening, 0.5);
        overlay.render(OverlayState::Processing, 1.0);
        let seen = overlay.takes();
        assert_eq!(seen.len(), 2);
        assert!(matches!(seen[0].0, OverlayState::Listening));
        assert_eq!(seen[0].1, 0.5);
        assert!(matches!(seen[1].0, OverlayState::Processing));
    }

    #[test]
    fn recording_overlay_takes_drains_log() {
        let overlay = RecordingOverlay::new();
        overlay.render(OverlayState::Idle, 0.0);
        assert_eq!(overlay.takes().len(), 1);
        assert!(overlay.takes().is_empty());
        overlay.render(OverlayState::Idle, 0.25);
        assert_eq!(overlay.takes().len(), 1);
    }
}
