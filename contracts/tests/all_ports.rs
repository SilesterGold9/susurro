//! Every adapter in tree through the port contracts, hardware-free.
//!
//! Mocks where they exist; error paths (missing model, dead
//! endpoint, closed port) where hardware would be needed. Blocking
//! calls (real hotkey waits, live mic reads) never appear here:
//! the suite must run identically on Linux CI, Windows CI, and a
//! laptop with no mic.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use susurro_contracts as c;

fn pcm() -> Vec<i16> {
    susurro_adapters_stt_local::stt_bench::synth_sine(1)
}

fn tmp_db(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "susurro-contract-{name}-{}",
        susurro_core::SessionId::generate()
    ))
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// --- speech to text ---

#[test]
fn mock_stt_honors_the_contract() {
    let stt = susurro_adapters_stt_local::MockStt::new("hello contract");
    c::check_stt_reports_or_errors(&stt, &pcm());
    c::check_stt_model_name(&stt);
    c::check_stt_partial_flag(&stt, &pcm());
}

#[test]
fn whisper_local_missing_model_errors_actionably() {
    let stt = susurro_adapters_stt_local::WhisperLocal::base_en(PathBuf::from(
        "/nonexistent-contract/base.en.bin",
    ));
    c::check_stt_reports_or_errors(&stt, &pcm());
    c::check_stt_model_name(&stt);
    c::check_stt_partial_flag(&stt, &pcm());
}

#[test]
fn windowed_partial_missing_model_stays_silent() {
    let whisper = susurro_adapters_stt_local::WhisperLocal::base_en(PathBuf::from(
        "/nonexistent-contract/base.en.bin",
    ));
    let decoder = susurro_adapters_stt_local::WindowedPartial::new(whisper);
    c::check_stt_reports_or_errors(&decoder, &pcm());
    c::check_stt_model_name(&decoder);
    c::check_stt_partial_flag(&decoder, &pcm());
}

#[test]
fn cloud_adapter_dead_endpoint_errors_without_leaking() {
    let cfg = susurro_adapters_stt_cloud::OpenAiCompatibleConfig::new(
        "http://127.0.0.1:1",
        "m",
        "secret-key-abc",
    )
    .unwrap()
    .with_timeout(5);
    let stt = susurro_adapters_stt_cloud::OpenAiCompatibleStt::new(cfg);
    c::check_stt_reports_or_errors(&stt, &pcm());
    c::check_stt_model_name(&stt);
}

#[test]
fn fallback_chain_lands_on_local_when_cloud_fails() {
    use susurro_adapters_stt_cloud as cloud;
    let cfg = cloud::OpenAiCompatibleConfig::new("http://127.0.0.1:1", "m", "k")
        .unwrap()
        .with_timeout(5);
    let chain = cloud::SttFallbackChain::new(Box::new(susurro_adapters_stt_local::MockStt::new(
        "hello local",
    )))
    .add_provider(
        "dead",
        Box::new(cloud::OpenAiCompatibleStt::new(cfg)),
        cloud::DEFAULT_FAILURE_THRESHOLD,
        cloud::DEFAULT_COOLDOWN_SECS,
    );
    c::check_stt_reports_or_errors(&chain, &pcm());
    c::check_stt_model_name(&chain);
}

// --- capture plus vad ---

#[test]
fn mock_capture_honors_protocol() {
    use susurro_adapters_audio::MockCapture;
    use susurro_core::ports::AudioChunk;
    let mut cap = MockCapture::new(vec![AudioChunk {
        samples: vec![1, 2, 3],
        is_final: true,
    }]);
    c::check_capture_protocol(&mut cap);
}

#[test]
fn energy_vad_separates_speech_from_silence() {
    c::check_vad_separates(&susurro_adapters_audio::EnergyVad::default());
}

// --- cleanup ---

#[test]
fn passthrough_and_regex_never_error() {
    c::check_cleanup_never_errors(&susurro_adapters_cleanup::PassthroughCleanup);
    c::check_cleanup_never_errors(&susurro_adapters_cleanup::RegexCleanup);
}

#[test]
fn ollama_dead_endpoint_falls_back() {
    let ollama = susurro_adapters_cleanup::OllamaCleanup {
        model: "contract".into(),
        endpoint: "http://127.0.0.1:1".into(),
    };
    c::check_cleanup_never_errors(&ollama);
}

// --- injection ---

#[test]
fn all_injectors_accept_empty() {
    c::check_inject_empty_ok(&susurro_adapters_linux::MockInjector::new());
    c::check_inject_empty_ok(&susurro_adapters_linux::LinuxPasteInjector::new());
    c::check_inject_empty_ok(&susurro_adapters_windows::WindowsSendInput);
    c::check_remove_last_empty_ok(&susurro_adapters_linux::MockInjector::new());
    c::check_remove_last_empty_ok(&susurro_adapters_linux::LinuxPasteInjector::new());
    c::check_remove_last_empty_ok(&susurro_adapters_windows::WindowsSendInput);
}

// --- settings plus history ---

#[test]
fn memory_settings_converge() {
    c::check_settings_roundtrip(&mut susurro_storage::MemorySettings::default());
}

#[test]
fn sqlite_settings_converge() {
    let p = tmp_db("settings");
    let mut store = susurro_storage::SqliteSettings::open(&p).unwrap();
    c::check_settings_roundtrip(&mut store);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn histories_converge_on_replay() {
    c::check_history_upsert_converges(&mut susurro_storage::MemoryHistory::default());
    let p = tmp_db("history");
    let mut store = susurro_storage::SqliteHistory::open(&p).unwrap();
    c::check_history_upsert_converges(&mut store);
    let _ = std::fs::remove_file(&p);
}

// --- network plus hotkey plus overlay ---

#[test]
fn network_offline_override_resolves() {
    let _guard = env_lock().lock().unwrap();
    std::env::set_var("SUSURRO_OFFLINE", "1");
    std::env::remove_var("SUSURRO_ONLINE");
    c::check_network_resolves(&susurro_adapters_stt_cloud::NetworkStatus::new());
    std::env::remove_var("SUSURRO_OFFLINE");
}

#[test]
fn network_closed_port_resolves_offline() {
    let _guard = env_lock().lock().unwrap();
    std::env::remove_var("SUSURRO_OFFLINE");
    std::env::remove_var("SUSURRO_ONLINE");
    let probe = susurro_adapters_stt_cloud::NetworkStatus::with_probe("127.0.0.1", 1, 2);
    c::check_network_resolves(&probe);
}

#[test]
fn mock_hotkey_answers_toggle() {
    c::check_hotkey_mock_returns_toggle(&c::MockHotkey);
}

#[test]
fn scripted_hotkey_replays_press_queue() {
    use susurro_core::ports::HotkeyEvent;
    let hotkey = susurro_adapters_linux::ScriptedHotkey::new(vec![HotkeyEvent::ToggleDictation]);
    c::check_hotkey_mock_returns_toggle(&hotkey);
}

#[test]
fn recording_overlay_records_pipeline() {
    use susurro_core::ports::OverlayState;
    let overlay = c::RecordingOverlay::new();
    c::check_overlay_accepts_all(&overlay);
    let seen = overlay.takes();
    assert_eq!(seen.len(), 15);
    assert!(seen
        .iter()
        .any(|(s, _)| matches!(s, OverlayState::Listening)));
    assert!(overlay.takes().is_empty());
}

#[test]
fn mock_network_fixed_states_resolve() {
    use susurro_adapters_stt_cloud::MockNetwork;
    use susurro_core::ports::NetworkState;
    c::check_network_resolves(&MockNetwork::new(NetworkState::Online));
    c::check_network_resolves(&MockNetwork::new(NetworkState::Offline));
}

#[test]
fn mock_vad_scripted_answers_separate() {
    c::check_vad_separates(&susurro_adapters_audio::MockVad::new(vec![true, false]));
}

#[test]
fn null_overlay_accepts_everything() {
    c::check_overlay_accepts_all(&c::NullOverlay);
}
