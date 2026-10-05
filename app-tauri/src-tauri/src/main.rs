//! Susurro Tauri backend (v0.1.0): pill overlay, tray, settings,
//! updater wiring. The dictation pipeline reuses the workspace crates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use susurro_adapters_audio::{EndpointDecision, VadEndpoint};
use susurro_core::ports::{AudioCapturePort, TextInjectionPort, TextPostProcessorPort};
use susurro_core::{Pipeline, SessionId, Ticket, TicketRegistry};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    seconds: u64,
    auto_stop: bool,
    sound: bool,
    cleanup: String,
    ollama_model: String,
    whisper_model: String,
    device: String,
    socket_path: String,
    update_channel: String,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default)]
    onboarding_done: bool,
    #[serde(default)]
    high_contrast: bool,
    #[serde(default = "default_announce")]
    announce: bool,
}

fn default_hotkey() -> String {
    susurro_core::hotkey::DEFAULT.into()
}

fn default_announce() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            seconds: 30,
            auto_stop: true,
            sound: true,
            cleanup: "onnx".into(),
            ollama_model: "qwen3:0.6b".into(),
            whisper_model: String::new(),
            device: String::new(),
            socket_path: "/tmp/susurro.sock".into(),
            update_channel: "stable".into(),
            hotkey: default_hotkey(),
            onboarding_done: false,
            high_contrast: false,
            announce: default_announce(),
        }
    }
}

/// One manifest asset as the System page sees it. `Ready` is the
/// only state that needs no explanation; the rest name their cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum ConvergenceAssetState {
    Ready,
    /// On disk but inside a backoff window from an earlier failure.
    Waiting,
    Missing,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ConvergenceAsset {
    name: String,
    version: String,
    state: ConvergenceAssetState,
    next_retry_secs: u64,
}

/// One asset's persisted failure record, with the remedy spelled out
/// so the page never has to map a category back to a sentence.
#[derive(Debug, Clone, serde::Serialize)]
struct ConvergenceAttempt {
    category: String,
    remedy: String,
    attempts: u32,
    next_retry_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ConvergenceView {
    assets: Vec<ConvergenceAsset>,
    converged: bool,
    next_retry_secs: u64,
    attempts: std::collections::BTreeMap<String, ConvergenceAttempt>,
    models_dir: Option<String>,
}

struct AppState {
    settings: Mutex<Settings>,
    dir: PathBuf,
    /// Double-trigger guard: at most one dictation runs at a time.
    /// A second press while busy reports busy instead of stacking runs.
    inflight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Bundled day-0 tiny, resolved from the resource dir at startup.
    /// None in dev without resources or with SKIP_MODEL_FETCH builds.
    bundled_tiny: Mutex<Option<PathBuf>>,
    /// Bundled day-0 cleanup model dir (ADR-004 Phase 4). Same
    /// lifecycle as the tiny: resolved once, read by every dictation.
    bundled_punct: Mutex<Option<PathBuf>>,
    /// Model prefetch single-flight: one background base fetch at a
    /// time; the download button joins it instead of doubling it.
    prefetch: Mutex<Option<std::thread::JoinHandle<Result<String, String>>>>,
    /// Boot-time convergence loop stop flag (ADR-004 point 5). The
    /// loop polls this so app exit cuts a backoff short.
    convergence_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    fn settings_file(dir: &std::path::Path) -> PathBuf {
        dir.join("settings.json")
    }

    /// Claim the single dictation slot. False means a run is already
    /// in flight and the new trigger must stand down.
    fn try_claim(&self) -> bool {
        self.inflight
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    }

    fn release(&self) {
        self.inflight
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    fn load(dir: &std::path::Path) -> Settings {
        let f = Self::settings_file(dir);
        std::fs::read_to_string(&f)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let s = self.settings.lock().map_err(|e| e.to_string())?;
        let f = Self::settings_file(&self.dir);
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Atomic write: temp + rename.
        let tmp = f.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&*s).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &f).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Bundled day-0 tiny for this process, if the bundle ships one.
fn bundled_of(state: &Arc<AppState>) -> Option<PathBuf> {
    state.bundled_tiny.lock().ok().and_then(|b| b.clone())
}

/// Bundled cleanup model dir, resolved once at startup like the tiny.
fn bundled_punct_of(state: &Arc<AppState>) -> Option<PathBuf> {
    state.bundled_punct.lock().ok().and_then(|b| b.clone())
}

/// Put the bundle's runtime directory on the DLL search path
/// (ADR-004 Phase 4). Tauri resources land in a `resources`
/// subdirectory, which the Windows loader does not search, so the
/// sherpa-onnx DLLs shipped beside the app would never be found and
/// punctuation would silently degrade to the regex tidier. Runs once
/// at startup, well before the first model load.
fn extend_dll_search_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().resource_dir().ok()?.join("dylib");
    if !dir.is_dir() {
        return None;
    }
    susurro_adapters_windows::extend_dll_search_path(&dir).then_some(dir)
}

/// Cleanup model pair for the `onnx` tier (ADR-004 Phase 4). The
/// store copy wins so an updated model takes over, and the bundled
/// copy is the day-0 fallback so a fresh install punctuates offline.
/// A half-present pair is worth nothing, so `None` when only one side
/// is there: the tier then fails open to regex rather than loading a
/// model with no vocabulary.
fn punct_paths(
    bundled: Option<&std::path::Path>,
) -> Option<susurro_adapters_cleanup::PunctPaths> {
    use susurro_provision::{PUNCT_MODEL_NAME, PUNCT_VOCAB_NAME};
    let store = models_home();
    let pair = |dir: &std::path::Path| {
        let model = dir.join(PUNCT_MODEL_NAME);
        let vocab = dir.join(PUNCT_VOCAB_NAME);
        (model.is_file() && vocab.is_file()).then_some(susurro_adapters_cleanup::PunctPaths {
            model,
            vocab,
        })
    };
    store
        .as_deref()
        .and_then(pair)
        .or_else(|| bundled.and_then(pair))
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

fn models_home() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home).join(".local/share/susurro/models"));
    }
    // Windows first-run when HOME is unset.
    std::env::var("USERPROFILE")
        .ok()
        .map(|home| PathBuf::from(home).join(".local/share/susurro/models"))
}

fn model_file_in(dir: &std::path::Path, name: &str) -> Option<String> {
    let p = dir.join(name);
    if p.exists() {
        return Some(p.to_string_lossy().into_owned());
    }
    None
}

fn resolve_whisper(explicit: &str, bundled_tiny: Option<&std::path::Path>) -> String {
    let env_model = std::env::var("SUSURRO_MODEL").ok();
    let models_dir = models_home();
    resolve_whisper_with(
        explicit,
        env_model.as_deref(),
        tier_file().as_deref(),
        models_dir.as_deref(),
        bundled_tiny,
    )
}

/// Stored benchmark tier file, when a previous run persisted one
/// and the file is still on disk. Missing or broken reads as unset.
fn tier_file() -> Option<std::path::PathBuf> {
    let store = susurro_storage::SqliteSettings::open(&shared_db_path()).ok()?;
    let tier = susurro_adapters_stt_local::bench::load_tier(&store)?;
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = tier.model_path(&home);
    path.exists().then_some(path)
}

/// Full resolution order, pure except existence checks (mirrors the
/// CLI resolver): explicit setting, SUSURRO_MODEL, benchmark tier
/// file, best quality on disk (small, base, tiny), bundled day-0
/// tiny, then the missing base path the error names.
fn resolve_whisper_with(
    explicit: &str,
    env_model: Option<&str>,
    tier_file: Option<&std::path::Path>,
    models_dir: Option<&std::path::Path>,
    bundled_tiny: Option<&std::path::Path>,
) -> String {
    if !explicit.is_empty() {
        return shellexpand(explicit);
    }
    if let Some(m) = env_model {
        let p = shellexpand(m);
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    if let Some(t) = tier_file {
        if t.exists() {
            return t.to_string_lossy().into_owned();
        }
    }
    if let Some(dir) = models_dir {
        for name in ["small.en.bin", "base.en.bin", "tiny.en.bin"] {
            if let Some(p) = model_file_in(dir, name) {
                return p;
            }
        }
    }
    if let Some(bundled) = bundled_tiny {
        if bundled.exists() {
            return bundled.to_string_lossy().into_owned();
        }
    }
    shellexpand("~/.local/share/susurro/models/base.en.bin")
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(settings: Settings, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // Mirror the hotkey into the shared kv store so the CLI daemon
    // honors a GUI remap. Best-effort, never fails the save.
    if let Ok(mut store) = susurro_storage::SqliteSettings::open(&shared_db_path()) {
        use susurro_core::ports::SettingsStorePort;
        let _ = store.set("hotkey", &settings.hotkey);
    }
    *state.settings.lock().map_err(|e| e.to_string())? = settings;
    state.save()
}

#[tauri::command]
fn run_doctor() -> String {
    let mut lines = vec!["Susurro doctor (tauri)".to_string()];
    match susurro_adapters_audio::default_input_name() {
        Some(n) => lines.push(format!("mic: found ({n})")),
        None => lines.push("mic: missing".into()),
    }
    for d in susurro_adapters_audio::list_input_devices() {
        lines.push(format!("  device: {d}"));
    }
    for tool in [
        "pw-record",
        "wl-copy",
        "wtype",
        "ydotool",
        "whisper-cli",
        "socat",
        "ollama",
        "curl",
    ] {
        // `where` is the Windows equivalent; failure means missing,
        // never an error, so doctor degrades to install hints.
        #[cfg(target_os = "windows")]
        let probe = "where";
        #[cfg(not(target_os = "windows"))]
        let probe = "which";
        let found = susurro_core::silent_command(probe)
            .arg(tool)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        lines.push(format!(
            "{tool}: {}",
            if found { "found" } else { "missing" }
        ));
    }
    lines.join("\n")
}

#[derive(Clone, Serialize)]
struct UtteranceResult {
    raw: String,
    cleaned: String,
    latency_ms: u64,
}

/// Shared history db with the CLI: same path, same rows, so undo in
/// either surface sees the same sessions.
fn shared_db_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/susurro/susurro.db");
    }
    #[cfg(target_os = "windows")]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("susurro").join("susurro.db");
    }
    std::env::temp_dir().join("susurro.db")
}

#[derive(Clone, Serialize)]
struct FormatProfileRow {
    app: String,
    style: String,
    cleanup: String,
}

/// List per-app formatting profiles (v0.8.0, issue 40).
#[tauri::command]
fn list_format_profiles() -> Result<Vec<FormatProfileRow>, String> {
    let store = susurro_storage::SqliteFormatProfiles::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    let profiles = store.list().map_err(|e| e.to_string())?;
    Ok(profiles
        .into_iter()
        .map(|p| FormatProfileRow {
            app: p.app.clone(),
            cleanup: p.style.cleanup().to_string(),
            style: p.style.as_str().to_string(),
        })
        .collect())
}

/// Set the formatting style for an app (formal, casual, verbatim).
#[tauri::command]
fn save_format_profile(app: String, style: String) -> Result<(), String> {
    let style = susurro_core::Style::parse(&style)?;
    let store = susurro_storage::SqliteFormatProfiles::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.set(&app, style).map_err(|e| e.to_string())
}

/// Usage plus latency stats for the settings view (v0.9.0, issue 43).
#[tauri::command]
fn get_stats() -> Result<serde_json::Value, String> {
    let history = susurro_storage::SqliteHistory::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    let rows = history.stat_rows(100_000).map_err(|e| e.to_string())?;
    let dict = susurro_storage::SqliteDictionary::open(&shared_db_path())
        .map(|d| d.list().unwrap_or_default())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let s = susurro_core::summarize(&rows, &dict, susurro_core::day_index(now));
    Ok(serde_json::json!({
        "entries": s.entries,
        "words": s.words,
        "polished": s.polished,
        "dict_hits": s.dict_hits,
        "dict_phrases": dict.len(),
        "streak_days": s.streak_days,
        "top_apps": s.top_apps.iter().map(|(a, n)| serde_json::json!({"app": a, "sessions": n})).collect::<Vec<_>>(),
        "p50_ms": s.p50_ms,
        "p95_ms": s.p95_ms,
        "p99_ms": s.p99_ms,
        "days": s.days.iter().map(|d| serde_json::json!({"label": d.label, "words": d.words})).collect::<Vec<_>>(),
    }))
}

/// Remove an app formatting profile (falls back to the cleanup setting).
#[tauri::command]
fn remove_format_profile(app: String) -> Result<(), String> {
    let store = susurro_storage::SqliteFormatProfiles::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.remove(&app).map_err(|e| e.to_string())
}

/// First-run state for the onboarding flow (v0.8.0, issue 41).
#[derive(Clone, Serialize)]
struct OnboardingStatus {
    model_found: bool,
    model_path: String,
    model_checksum: String,
    tier: Option<String>,
    hotkey: String,
    onboarding_done: bool,
}

/// Where onboarding stands: model on disk, persisted bench tier,
/// chosen hotkey, and whether the flow already finished.
#[tauri::command]
fn onboarding_status(state: State<'_, Arc<AppState>>) -> Result<OnboardingStatus, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let path = resolve_whisper(&settings.whisper_model, bundled_of(&state).as_deref());
    let model_found = std::path::Path::new(&path).exists();
    let model_checksum = checksum_line(&path);
    let tier = susurro_storage::SqliteSettings::open(&shared_db_path())
        .ok()
        .and_then(|s| susurro_adapters_stt_local::bench::load_tier(&s))
        .map(|t| t.as_str().to_string());
    Ok(OnboardingStatus {
        model_found,
        model_path: path,
        model_checksum,
        tier,
        hotkey: settings.hotkey,
        onboarding_done: settings.onboarding_done,
    })
}

/// One-line checksum state shared by status and download
/// (v0.9.0, issue 45). Trust on first use, compare after.
fn checksum_line(path: &str) -> String {
    use susurro_adapters_stt_local::checksum::{verify_model, VerifyOutcome};
    let p = std::path::Path::new(path);
    match susurro_storage::SqliteSettings::open(&shared_db_path()) {
        Ok(mut store) => match verify_model(p, &mut store) {
            Ok(VerifyOutcome::Matched(h)) => format!("verified {}", &h[..16.min(h.len())]),
            Ok(VerifyOutcome::Recorded(h)) => {
                format!("recorded {}", &h[..16.min(h.len())])
            }
            Ok(VerifyOutcome::Mismatch { .. }) => "MISMATCH re-download".into(),
            Err(e) => format!("unverified ({e})"),
        },
        Err(e) => format!("unverified ({e})"),
    }
}

/// System requirements for onboarding screen one (Windows audit):
/// linked engine, model, and paste tools with per-OS install hints.
/// The engine is linked in, so whisper is always ready: only the
/// model can still be missing. Nothing here blocks: each item names
/// its own fix.
#[tauri::command]
fn requirements_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    requirements_data(&state)
}

/// Shared probe body: onboarding and System read the same object,
/// so the two screens can never disagree about the machine.
fn requirements_data(
    state: &State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let whisper = Some(format!(
        "native whisper.cpp {} (linked, no install needed)",
        susurro_adapters_stt_local::native::linked_version()
    ));
    let model_path = resolve_whisper(&settings.whisper_model, bundled_of(state).as_deref());
    let model_found = std::path::Path::new(&model_path).exists();
    #[cfg(target_os = "windows")]
    let os = "windows";
    #[cfg(target_os = "linux")]
    let os = "linux";
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let os = "other";
    #[cfg(target_os = "windows")]
    let (paste_ok, paste_detail) = (true, "SendInput direct-type, no tools needed".to_string());
    #[cfg(not(target_os = "windows"))]
    let (paste_ok, paste_detail) = {
        let mut missing = Vec::new();
        for tool in ["wl-copy", "ydotool", "socat"] {
            let found = susurro_core::silent_command(if cfg!(target_os = "windows") {
                "where"
            } else {
                "which"
            })
            .arg(tool)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
            if !found {
                missing.push(tool);
            }
        }
        if missing.is_empty() {
            (true, "wl-copy plus ydotool paste ready".to_string())
        } else {
            (
                false,
                format!("missing {} — see README", missing.join(", ")),
            )
        }
    };
    // The engine ships linked: no binary to install, no hint needed.
    let whisper_hint = String::new();
    // Cleanup chain (ADR-004 Phase 4): the bundled ONNX punctuation
    // model is the default and needs no server, so this reports that
    // tier first. The Ollama API is identical on every OS, so one
    // probe covers all, and the model must be pulled, not just the
    // server up.
    let punct_ready = punct_paths(bundled_punct_of(state).as_deref()).is_some();
    let cleanup_tier = settings.cleanup.clone();
    let ollama_models = susurro_core::silent_command("curl")
        .args(["-sS", "-m", "5", "http://localhost:11434/api/tags"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let ollama_up = !ollama_models.is_empty();
    let ollama_model_present =
        ollama_models.contains(&settings.ollama_model);
    // Only nag about Ollama when the user actually chose that tier.
    let ollama_hint = if cleanup_tier != "ollama" || (ollama_up && ollama_model_present) {
        String::new()
    } else if os == "windows" {
        format!(
            "Install Ollama for Windows, then run ollama pull {} so cleanup has its model.",
            settings.ollama_model
        )
    } else {
        format!(
            "Start Ollama and run ollama pull {} so cleanup has its model.",
            settings.ollama_model
        )
    };
    Ok(serde_json::json!({
        "os": os,
        "whisper": whisper,
        "model_found": model_found,
        "model_path": model_path,
        "paste_ok": paste_ok,
        "paste_detail": paste_detail,
        "whisper_hint": whisper_hint,
        "ollama_up": ollama_up,
        "ollama_model_present": ollama_model_present,
        "ollama_hint": ollama_hint,
        "cleanup_tier": cleanup_tier,
        "punct_ready": punct_ready,
        "punct_hint": if punct_ready {
            String::new()
        } else {
            "punctuation model not on disk yet: run susurro converge, or let the next launch fetch it."
                .to_string()
        },
    }))
}

/// Live capability matrix for the System page (Phase 3): the same
/// requirements object onboarding sees, merged with the provision
/// asset report, the resolved model plus tier, and the prefetch
/// state. One endpoint, no second opinion about the machine.
#[tauri::command]
fn system_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let mut status = requirements_data(&state)?;
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let bundled = bundled_of(&state);
    let manifest = susurro_provision::default_manifest();
    let models = match models_home() {
        Some(dir) => susurro_provision::health(&manifest, &dir, bundled.as_deref()),
        None => Vec::new(),
    };
    let resolved = resolve_whisper(&settings.whisper_model, bundled.as_deref());
    let tier = susurro_storage::SqliteSettings::open(&shared_db_path())
        .ok()
        .and_then(|s| susurro_adapters_stt_local::bench::load_tier(&s))
        .map(|t| t.as_str().to_string());
    let prefetch_running = state
        .prefetch
        .lock()
        .map(|p| p.is_some())
        .unwrap_or(false);
    let map = status
        .as_object_mut()
        .ok_or_else(|| "requirements malformed".to_string())?;
    map.insert(
        "engine".to_string(),
        serde_json::json!({
            "kind": "native",
            "version": susurro_adapters_stt_local::native::linked_version(),
        }),
    );
    map.insert(
        "models".to_string(),
        serde_json::to_value(&models).map_err(|e| e.to_string())?,
    );
    map.insert("resolved_path".to_string(), resolved.into());
    map.insert("tier".to_string(), tier.into());
    map.insert("prefetch_running".to_string(), prefetch_running.into());
    map.insert(
        "convergence".to_string(),
        serde_json::to_value(convergence_snapshot(bundled.as_deref())).unwrap_or_default(),
    );
    Ok(status)
}

/// Convergence status for the System page (ADR-004 point 5). The
/// same `susurro-provision` report the CLI prints, so the two
/// surfaces can never disagree about what is missing or when the
/// retry loop next acts.
#[tauri::command]
fn converge_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    serde_json::to_value(convergence_snapshot(bundled_of(&state).as_deref()))
        .map_err(|e| e.to_string())
}

/// One read-only pass over the plane, plus the persisted retry
/// state. No downloads happen here: the boot loop owns fetching, so
/// a page refresh cannot start a 150 MB transfer.
fn convergence_snapshot(bundled: Option<&std::path::Path>) -> ConvergenceView {
    let Some(dir) = models_home() else {
        return ConvergenceView {
            assets: Vec::new(),
            converged: false,
            next_retry_secs: 0,
            attempts: std::collections::BTreeMap::new(),
            models_dir: None,
        };
    };
    let manifest = susurro_provision::default_manifest();
    let report = susurro_provision::health(&manifest, &dir, bundled);
    let retry_state = susurro_provision::ConvergeState::load(&dir);
    let mut attempts = std::collections::BTreeMap::new();
    let mut next_retry_secs = 0;
    for (name, failure) in &retry_state.assets {
        attempts.insert(
            name.clone(),
            ConvergenceAttempt {
                category: format!("{:?}", failure.category),
                remedy: failure.category.remedy().to_string(),
                attempts: failure.attempts,
                next_retry_secs: susurro_provision::backoff_secs(failure.attempts),
            },
        );
        next_retry_secs = next_retry_secs.max(susurro_provision::backoff_secs(failure.attempts));
    }
    let mut assets = Vec::new();
    for asset in &report {
        // A verified copy needs no row beyond "ready"; anything else
        // carries the reason so the page can name the fix.
        let next_retry_secs = retry_state.delay_for(&asset.name);
        let asset_state = match asset.copies.iter().find(|c| c.bytes_ok) {
            Some(_) if retry_state.attempts_for(&asset.name) == 0 => ConvergenceAssetState::Ready,
            Some(_) => ConvergenceAssetState::Waiting,
            None => ConvergenceAssetState::Missing,
        };
        assets.push(ConvergenceAsset {
            name: asset.name.clone(),
            version: asset.version.clone(),
            state: asset_state,
            next_retry_secs,
        });
    }
    next_retry_secs = next_retry_secs.max(
        assets
            .iter()
            .map(|a| a.next_retry_secs)
            .max()
            .unwrap_or(0),
    );
    ConvergenceView {
        converged: assets
            .iter()
            .all(|a| a.state == ConvergenceAssetState::Ready),
        assets,
        next_retry_secs,
        attempts,
        models_dir: Some(dir.to_string_lossy().into_owned()),
    }
}

/// Start the convergence loop on a background thread (ADR-004 point
/// 5: runs on boot). The loop stops when every asset is on disk or a
/// failure needs a human, and the shared stop flag lets app exit cut
/// a backoff short instead of parking the thread for 15 minutes.
fn spawn_convergence(app: AppHandle, state: Arc<AppState>) {
    let Some(dir) = models_home() else {
        return;
    };
    let bundled = bundled_of(&state);
    std::thread::spawn(move || {
        let manifest = susurro_provision::default_manifest();
        let mut converger =
            susurro_provision::Converger::new(manifest, dir, bundled);
        let stop = state.convergence_stop.clone();
        converger.run_until_converged(
            6,
            &|| stop.load(std::sync::atomic::Ordering::SeqCst),
            &|d| {
                // Chunked sleep so exit is noticed within a second,
                // not at the end of a 15-minute backoff.
                let mut left = d;
                while !left.is_zero()
                    && !stop.load(std::sync::atomic::Ordering::SeqCst)
                {
                    let step = left.min(std::time::Duration::from_secs(1));
                    std::thread::sleep(step);
                    left -= step;
                }
                stop.load(std::sync::atomic::Ordering::SeqCst)
            },
            &|report| {
                let _ = app.emit(
                    "susurro://convergence",
                    serde_json::json!({
                        "converged": report.converged,
                        "next_retry_secs": report.next_delay_secs(),
                        "assets": report.assets,
                    }),
                );
            },
        );
    });
}

/// Fetch one asset with onboarding progress events, then record
/// trust-on-first-use plus the manifest version. Shared by the
/// button and the background prefetch: same bytes, same records.
/// Resume, hash-while-write, and atomic swap live in
/// `susurro-provision`.
fn fetch_asset_with_progress(
    app: &AppHandle,
    asset_name: &str,
    force: bool,
) -> Result<String, String> {
    let dir = models_home().ok_or_else(|| "no models dir on this machine.".to_string())?;
    let manifest = susurro_provision::default_manifest();
    // The manifest outlives this borrow: clone the entry we need.
    let asset = susurro_provision::select_asset(&manifest, asset_name)
        .ok_or_else(|| format!("{asset_name} vanished from the asset manifest."))?
        .clone();
    let _ = app.emit(
        "susurro://onboarding",
        serde_json::json!({ "step": "model", "state": "downloading", "pct": 0 }),
    );
    // Whole points upward only, like the old bar parser.
    let last = std::cell::Cell::new(0u64);
    let progress_app = app.clone();
    let path = susurro_provision::ensure_asset(&dir, &asset, force, &|done, total| {
        if let Some(total) = total.filter(|t| *t > 0) {
            let pct = done.saturating_mul(100) / total;
            if pct > last.get() {
                last.set(pct);
                let _ = progress_app.emit(
                    "susurro://onboarding",
                    serde_json::json!({
                        "step": "model",
                        "state": "downloading",
                        "pct": pct,
                    }),
                );
            }
        }
    })
    .map_err(|e| {
        let _ = app.emit(
            "susurro://onboarding",
            serde_json::json!({ "step": "model", "state": "failed" }),
        );
        e.to_string()
    })?;
    // Trust on first use starts at download: the fresh bytes are the
    // reference every later run compares against. The manifest
    // version rides beside the hash so upgrades can promote.
    if let Ok(mut store) = susurro_storage::SqliteSettings::open(&shared_db_path()) {
        use susurro_adapters_stt_local::checksum::verify_model;
        use susurro_core::ports::SettingsStorePort;
        let _ = verify_model(&path, &mut store);
        let _ = store.set(&format!("asset_version:{asset_name}"), &asset.version);
    }
    let _ = app.emit(
        "susurro://onboarding",
        serde_json::json!({ "step": "model", "state": "done" }),
    );
    Ok(path.to_string_lossy().into_owned())
}

/// Explicit model download: joins the background prefetch when one
/// is running instead of doubling the transfer.
#[tauri::command]
fn download_model(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let handle = state.prefetch.lock().map_err(|e| e.to_string())?.take();
    match handle {
        Some(join) => join.join().map_err(|_| "background fetch panicked".to_string())?,
        None => fetch_asset_with_progress(&app, "base.en.bin", false),
    }
}

/// Background base fetch plus auto-bench, started on onboarding
/// first paint: the user picks intent, tone, and hotkey while the
/// bytes stream. Single-flight: a second call reports instead of
/// spawning. The done event refreshes every screen listening.
#[tauri::command]
fn start_model_prefetch(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    {
        let guard = state.prefetch.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok("base fetch already running.".into());
        }
    }
    let worker_app = app.clone();
    let join = std::thread::spawn(move || {
        // Bench first (sub-second): the tier is recorded before the
        // slow download finishes, so resolution promotes correctly.
        {
            use susurro_adapters_stt_local::bench;
            let (tier, _) = bench::benchmark();
            if let Ok(mut store) = susurro_storage::SqliteSettings::open(&shared_db_path()) {
                let _ = bench::store_tier(&mut store, tier);
            }
        }
        fetch_asset_with_progress(&worker_app, "base.en.bin", false)
    });
    *state.prefetch.lock().map_err(|e| e.to_string())? = Some(join);
    Ok("base fetch started in the background.".into())
}

/// Run the CPU benchmark and persist the winning tier, same as the
/// CLI bench. The tier pick rides inside onboarding screen one.
#[tauri::command]
fn run_bench() -> Result<serde_json::Value, String> {
    use susurro_adapters_stt_local::bench;
    let (tier, probe) = bench::benchmark();
    let persisted = match susurro_storage::SqliteSettings::open(&shared_db_path()) {
        Ok(mut store) => bench::store_tier(&mut store, tier)
            .map(|_| true)
            .unwrap_or(false),
        Err(_) => false,
    };
    Ok(serde_json::json!({
        "tier": tier.as_str(),
        "iters_per_sec": probe.iters_per_sec,
        "elapsed_ms": probe.elapsed_ms,
        "cores": probe.cores,
        "persisted": persisted,
    }))
}

/// Short test dictation for onboarding screen three: six seconds,
/// then the transcript shows in the window.
#[tauri::command]
fn test_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UtteranceResult, String> {
    if !state.try_claim() {
        return Err("already dictating. Wait for this run to finish.".into());
    }
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    settings.seconds = 6;
    let tickets = TicketRegistry::new();
    let out = run_dictation(
        &app,
        &settings,
        &tickets,
        bundled_of(&state).as_deref(),
        bundled_punct_of(&state).as_deref(),
    );
    state.release();
    out
}

/// Hyprland bind line for a hotkey choice, same names as the CLI
/// hyprland-bind --hotkey flag.
#[tauri::command]
fn hotkey_snippet(hotkey: String) -> String {
    let combo = match hotkey.trim().to_lowercase().as_str() {
        "ctrl_shift_r" => "CTRL_SHIFT, R",
        "shift_d" => "SHIFT, D",
        _ => "SUPER_SHIFT, R",
    };
    format!("bind = {combo}, exec, echo toggle | socat - UNIX-CONNECT:/tmp/susurro.sock")
}

/// Finish onboarding: store the hotkey choice and close the flow.
/// First run never returns after this; settings opens instead.
#[tauri::command]
fn finish_onboarding(
    hotkey: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let hotkey =
        susurro_core::hotkey::normalize(&hotkey).unwrap_or_else(|_| default_hotkey());
    if let Ok(mut store) = susurro_storage::SqliteSettings::open(&shared_db_path()) {
        use susurro_core::ports::SettingsStorePort;
        let _ = store.set("hotkey", &hotkey);
    }
    {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.hotkey = hotkey;
        settings.onboarding_done = true;
    }
    state.save()
}

#[derive(Clone, Serialize)]
struct SnippetRow {
    trigger: String,
    expansion: String,
}

/// List spoken snippets for the Snippets page.
#[tauri::command]
fn list_snippets() -> Result<Vec<SnippetRow>, String> {
    let store = susurro_storage::SqliteSnippets::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    let snippets = store.list().map_err(|e| e.to_string())?;
    Ok(snippets
        .into_iter()
        .map(|s| SnippetRow {
            trigger: s.trigger,
            expansion: s.expansion,
        })
        .collect())
}

/// Add or overwrite a spoken snippet.
#[tauri::command]
fn save_snippet(trigger: String, expansion: String) -> Result<(), String> {
    let store = susurro_storage::SqliteSnippets::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.set(&trigger, &expansion).map_err(|e| e.to_string())
}

/// Remove a spoken snippet by trigger.
#[tauri::command]
fn remove_snippet(trigger: String) -> Result<(), String> {
    let store = susurro_storage::SqliteSnippets::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.remove(&trigger).map_err(|e| e.to_string())
}

/// List dictionary phrases for the Dictionary page.
#[tauri::command]
fn list_dictionary() -> Result<Vec<String>, String> {
    let store = susurro_storage::SqliteDictionary::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.list().map_err(|e| e.to_string())
}

/// Add a dictionary phrase.
#[tauri::command]
fn save_word(phrase: String) -> Result<(), String> {
    let store = susurro_storage::SqliteDictionary::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.add(&phrase).map_err(|e| e.to_string())
}

/// Apply an onboarding intent plus tone (revamp plan): writes real
/// format profiles for the category app patterns. Intent answers
/// where, the style quiz answers how, so both change behavior.
#[tauri::command]
fn apply_intent(intent: String, style: String) -> Result<String, String> {
    let intent = intent.trim().to_lowercase();
    let style = susurro_core::Style::parse(&style)?;
    let patterns: &[&str] = match intent.as_str() {
        "docs" => &["docs", "libreoffice", "word", "notion", "obsidian"],
        "messages" => &[
            "chat", "telegram", "discord", "slack", "whatsapp", "message",
        ],
        "both" => &[
            "docs",
            "libreoffice",
            "word",
            "notion",
            "obsidian",
            "chat",
            "telegram",
            "discord",
            "slack",
            "whatsapp",
            "message",
        ],
        _ => return Err("unknown intent. Use docs, messages, or both.".into()),
    };
    let store = susurro_storage::SqliteFormatProfiles::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    for app in patterns {
        store.set(app, style).map_err(|e| e.to_string())?;
    }
    Ok(format!(
        "{} profiles sound {} for {intent}.",
        patterns.len(),
        style.as_str(),
    ))
}

/// Remove a dictionary phrase.
#[tauri::command]
fn remove_word(phrase: String) -> Result<(), String> {
    let store = susurro_storage::SqliteDictionary::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.remove(&phrase).map_err(|e| e.to_string())
}

/// Erase user data for a fresh start (onboarding plan): history,
/// events, tickets, dictionary, privacy additions, profiles, and
/// snippets. Settings and models survive.
#[tauri::command]
fn wipe_data() -> Result<String, String> {
    let counts = susurro_storage::wipe_user_data(&shared_db_path()).map_err(|e| e.to_string())?;
    let total =
        counts.history + counts.events + counts.tickets + counts.dictionary + counts.privacy + counts.profiles + counts.snippets;
    Ok(format!(
        "erased {total} rows ({} sessions, {} events, {} phrases, {} snippets).",
        counts.history, counts.events, counts.dictionary, counts.snippets
    ))
}

/// Reopen the onboarding window (Help page replay entry).
#[tauri::command]
fn show_onboarding(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("onboarding") {
        w.show().map_err(|e| e.to_string())?;
        let _ = w.set_focus();
        Ok(())
    } else {
        Err("no onboarding window in this build.".into())
    }
}

#[derive(Clone, Serialize)]
struct HistoryRow {
    session: String,
    raw_text: String,
    cleaned_text: Option<String>,
    provider: String,
    latency_ms: u64,
    created_at: i64,
}

#[tauri::command]
fn list_history(limit: u64) -> Result<Vec<HistoryRow>, String> {
    let h = susurro_storage::SqliteHistory::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    let entries = h
        .recent(limit.clamp(1, 100) as usize)
        .map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .map(|e| HistoryRow {
            session: e.session.to_string(),
            raw_text: e.raw_text,
            cleaned_text: e.cleaned_text,
            provider: e.provider,
            latency_ms: e.latency_ms,
            created_at: e.created_at,
        })
        .collect())
}

/// Re-inject one session's raw transcript (v0.8.0, issue 39
/// addendum): the undo-AI-edit toggle. History keeps the entry;
/// restoring twice pastes twice, which is the operator asking twice.
#[tauri::command]
fn restore_session(session: String) -> Result<String, String> {
    let h = susurro_storage::SqliteHistory::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    let entries = h.recent(50).map_err(|e| e.to_string())?;
    let entry = susurro_core::ports::find_history_entry(&entries, &session)?;
    if entry.raw_text.trim().is_empty() {
        return Err("nothing to restore. Dictate something first.".into());
    }
    GuiInjector
        .inject(
            &entry.raw_text,
            &Ticket::new(SessionId::generate(), "restore"),
        )
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "restored raw transcript ({} chars).",
        entry.raw_text.chars().count()
    ))
}

struct GuiCapture {
    pcm: Vec<i16>,
    done: bool,
}

impl AudioCapturePort for GuiCapture {
    fn start(&mut self) -> Result<(), susurro_core::CoreError> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), susurro_core::CoreError> {
        Ok(())
    }
    fn next_chunk(&mut self) -> Result<susurro_core::ports::AudioChunk, susurro_core::CoreError> {
        if self.done {
            return Ok(susurro_core::ports::AudioChunk {
                samples: vec![],
                is_final: true,
            });
        }
        self.done = true;
        Ok(susurro_core::ports::AudioChunk {
            samples: std::mem::take(&mut self.pcm),
            is_final: true,
        })
    }
}

struct GuiInjector;
impl TextInjectionPort for GuiInjector {
    fn inject(
        &self,
        text: &str,
        ticket: &susurro_core::Ticket,
    ) -> Result<(), susurro_core::CoreError> {
        // Injection is platform-owned: clipboard-free Unicode typing
        // on Windows, wl-copy/ydotool paste on Linux.
        #[cfg(target_os = "windows")]
        return susurro_adapters_windows::WindowsSendInput.inject(text, ticket);
        #[cfg(not(target_os = "windows"))]
        return susurro_adapters_linux::LinuxPasteInjector::new().inject(text, ticket);
    }
    fn remove_last(
        &self,
        text: &str,
        ticket: &susurro_core::Ticket,
    ) -> Result<(), susurro_core::CoreError> {
        #[cfg(target_os = "windows")]
        return susurro_adapters_windows::WindowsSendInput.remove_last(text, ticket);
        #[cfg(not(target_os = "windows"))]
        return susurro_adapters_linux::LinuxPasteInjector::new().remove_last(text, ticket);
    }
}

fn emit_state(app: &AppHandle, s: &str) {
    let _ = app.emit("susurro://state", s);
}

fn emit_level(app: &AppHandle, v: f32) {
    let _ = app.emit("susurro://level", v);
}

fn emit_error(app: &AppHandle, msg: &str) {
    emit_state(app, "error");
    let _ = app.emit("susurro://error", msg);
}

/// Show the pill bottom-center on the current monitor.
///
/// The pill never takes focus: stealing focus would yank the caret out of
/// the app being dictated into so paste lands in the wrong window, and on
/// Hyprland a focused window also gains an active border. Visibility comes
/// from always-on-top, not focus. The start cue plays here, at the moment
/// the pill answers the hotkey.
fn show_pill(app: &AppHandle, sound: bool) {
    if let Some(w) = app.get_webview_window("pill") {
        // A never-shown window has no monitor yet, so fall back to primary.
        // Positioning must always run: skipping it leaves placement to the
        // window manager, which centers new windows.
        let monitor = w
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| w.primary_monitor().ok().flatten());
        if let Some(m) = monitor {
            let scale = m.scale_factor();
            let size = m.size().to_logical::<f64>(scale);
            // Window is 360x64 logical; place center-x above the bottom
            // edge with a margin. On Windows the edge is the work area
            // (taskbar excluded): docking against full height hides the
            // pill behind the bar.
            #[cfg(target_os = "windows")]
            let (x, y) = match susurro_adapters_windows::work_area_px() {
                Some((wx, wy, ww, wh)) => (
                    (wx as f64 + (ww as f64 - 360.0) / 2.0) / scale,
                    (wy as f64 + wh as f64 - 64.0 - 12.0) / scale,
                ),
                None => (size.width / 2.0 - 360.0 / 2.0, size.height * 0.92),
            };
            #[cfg(not(target_os = "windows"))]
            let (x, y) = (size.width / 2.0 - 360.0 / 2.0, size.height * 0.92);
            // Below 768p screens this clips a few pixels; dragging
            // overrides the dock wherever the compositor honors moves.
            let _ = w.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        } else {
            eprintln!("pill: no monitor found, showing at default position");
        }
        let _ = w.show();
        susurro_adapters_audio::CuePlayer::new(sound).play(susurro_adapters_audio::Cue::Start);
    }
}

/// Replay slice peaks over ~1s so the waveform visibly moves between
/// chunk recordings (true streaming lands in v0.4.0). Four filler ticks
/// at the noise floor follow the slices so the wave breathes through
/// inter-chunk gaps instead of freezing mid-peak.
fn animate_levels(app: AppHandle, chunk: Vec<i16>) {
    std::thread::spawn(move || {
        let n = 10;
        let len = chunk.len();
        for i in 0..n {
            let s = len * i / n;
            let e = len * (i + 1) / n;
            let peak = susurro_adapters_audio::peak_amplitude(&chunk[s..e]);
            emit_level(&app, susurro_adapters_audio::level_from_peak(peak));
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
        for f in [0.05, 0.03, 0.06, 0.04] {
            emit_level(&app, f);
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
    });
}

#[derive(Clone, Serialize)]
struct ProgressTick {
    stage: susurro_core::Stage,
    value: f32,
}

fn emit_progress(app: &AppHandle, stage: susurro_core::Stage, value: f32) {
    let _ = app.emit("susurro://progress", ProgressTick { stage, value });
}

/// Capture is platform-owned: PipeWire with VAD auto-stop on Linux,
/// cpal fixed-window on Windows (chunked VAD parity is future work),
/// actionable error elsewhere. Waveform animation and cues run in
/// every path so the pill behaves the same while recording.
#[cfg(target_os = "linux")]
fn capture_pcm(
    app: &AppHandle,
    settings: &Settings,
    cues: &susurro_adapters_audio::CuePlayer,
) -> Result<Vec<i16>, String> {
    // Only a VAD end plays the stop cue. Cap-timeout stops stay silent
    // so the two endings feel different.
    let mut endpoint = VadEndpoint::default();
    let mut all: Vec<i16> = Vec::new();
    let max = if settings.auto_stop {
        settings.seconds.clamp(2, 30)
    } else {
        1
    };
    if settings.auto_stop {
        for _ in 0..max {
            let target = (!settings.device.is_empty()).then_some(settings.device.as_str());
            let chunk = susurro_adapters_audio::record_pipewire(1, target)
                .map_err(|e| format!("Couldn't capture audio. Check mic: {e}"))?;
            let tail = susurro_adapters_audio::trim_transient(&chunk);
            animate_levels(app.clone(), tail.to_vec());
            let d = endpoint.push(tail, 1.0);
            all.extend_from_slice(&chunk);
            if d == EndpointDecision::EndOfSpeech {
                cues.play(susurro_adapters_audio::Cue::Stop);
                break;
            }
        }
    } else {
        let target = (!settings.device.is_empty()).then_some(settings.device.as_str());
        all = susurro_adapters_audio::record_pipewire(settings.seconds.clamp(1, 30), target)
            .map_err(|e| format!("Couldn't capture audio. Check mic: {e}"))?;
        animate_levels(app.clone(), all.clone());
    }
    if all.is_empty() {
        let msg = "Captured zero samples. Is the mic muted?";
        cues.play(susurro_adapters_audio::Cue::Error);
        emit_error(app, msg);
        return Err(msg.into());
    }
    Ok(all)
}

#[cfg(target_os = "windows")]
fn capture_pcm(
    app: &AppHandle,
    settings: &Settings,
    cues: &susurro_adapters_audio::CuePlayer,
) -> Result<Vec<i16>, String> {
    use susurro_adapters_audio::CpalCapture;
    let seconds = settings.seconds.clamp(1, 30);
    let target = (!settings.device.is_empty()).then_some(settings.device.as_str());
    let mut cap = match target {
        Some(dev) => CpalCapture::with_device(seconds, dev),
        None => CpalCapture::new(seconds),
    };
    cap.start()
        .map_err(|e| format!("Couldn't start capture. Check mic permissions: {e}"))?;
    let chunk = cap
        .next_chunk()
        .map_err(|e| format!("Couldn't capture audio. Check mic permissions: {e}"))?;
    let _ = cap.stop();
    animate_levels(app.clone(), chunk.samples.clone());
    if chunk.samples.is_empty() {
        let msg = "Captured zero samples. Is the mic muted?";
        cues.play(susurro_adapters_audio::Cue::Error);
        emit_error(app, msg);
        return Err(msg.into());
    }
    Ok(chunk.samples)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn capture_pcm(
    _app: &AppHandle,
    _settings: &Settings,
    _cues: &susurro_adapters_audio::CuePlayer,
) -> Result<Vec<i16>, String> {
    Err("Capture needs Linux or Windows.".into())
}

/// Shared dictation run used by the command, tray, and hotkey thread.
fn run_dictation(
    app: &AppHandle,
    settings: &Settings,
    tickets: &TicketRegistry,
    bundled_tiny: Option<&std::path::Path>,
    bundled_punct: Option<&std::path::Path>,
) -> Result<UtteranceResult, String> {
    let t0 = std::time::Instant::now();
    emit_state(app, "listening");
    // Dictation context for the pill avatar: which app receives the
    // text. Linux auto-detects; elsewhere the avatar stays generic.
    // Best-effort display data, never blocks the run.
    #[cfg(target_os = "linux")]
    let focused = susurro_adapters_linux::focused_app();
    #[cfg(not(target_os = "linux"))]
    let focused: Option<String> = None;
    // Format profile (v0.8.0, issue 40): tone follows the app, so a
    // matching profile overrides the settings cleanup for this run.
    // A broken store degrades to settings, never blocks dictation.
    let profile_style: Option<String> = susurro_storage::SqliteFormatProfiles::open(
        &shared_db_path(),
    )
    .ok()
    .and_then(|store| store.list().ok())
    .as_deref()
    .and_then(|profiles| susurro_core::matched_profile(profiles, focused.as_deref()))
    .map(|p| p.style.as_str().to_string());
    let _ = app.emit(
        "susurro://context",
        serde_json::json!({ "app": focused, "profile": profile_style }),
    );
    let cues = susurro_adapters_audio::CuePlayer::new(settings.sound);

    let pcm = capture_pcm(app, settings, &cues)?;

    emit_state(app, "processing");
    let stt = susurro_adapters_stt_local::native::WhisperNative::base_en(
        resolve_whisper(&settings.whisper_model, bundled_tiny).into(),
    );
    // Cleanup chain (ADR-004 Phase 4). `onnx` is the default and needs no
    // server; `ollama` is the opt-in rewrite tier for the formal
    // profile; `regex` tidies; `none` injects raw. Every tier fails
    // open to regex, so a missing model never blocks injection.
    let cleanup_name: &str = match profile_style.as_deref() {
        Some("formal") => "ollama",
        Some("casual") => "regex",
        Some("verbatim") => "none",
        _ => settings.cleanup.as_str(),
    };
    let cleanup = susurro_adapters_cleanup::by_name(
        cleanup_name,
        punct_paths(bundled_punct),
        &settings.ollama_model,
    )
    .map_err(|e| e.to_string())?;
    let cleanup: &dyn TextPostProcessorPort = cleanup.as_ref();
    // Staged progress ticker: the current stage plus its start time are
    // shared with a thread that emits susurro://progress every 100ms.
    // The math lives in core (progress_for): asymptotic per stage, so
    // the fill stalls but never regresses or finishes early.
    let stage_now = std::sync::Arc::new(std::sync::Mutex::new((
        susurro_core::Stage::Transcribing,
        std::time::Instant::now(),
    )));
    let progress_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let progress_thread = {
        let app = app.clone();
        let stage_now = stage_now.clone();
        let progress_stop = progress_stop.clone();
        std::thread::spawn(move || {
            while !progress_stop.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(guard) = stage_now.lock() {
                    let (stage, since) = (guard.0, guard.1);
                    emit_progress(
                        &app,
                        stage,
                        susurro_core::progress_for(stage, since.elapsed().as_millis() as u64),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
    };
    let mut capture = GuiCapture { pcm, done: false };
    let session = SessionId::generate();
    // Snippets (issue 55): same whole-utterance match as the CLI.
    // Best-effort load so a broken db degrades to plain dictation.
    let snippets: Vec<susurro_core::Snippet> =
        susurro_storage::SqliteSnippets::open(&shared_db_path())
            .ok()
            .and_then(|store| store.list().ok())
            .unwrap_or_default();
    let out = Pipeline::run_staged(
        &mut capture,
        &stt,
        cleanup,
        &GuiInjector,
        tickets,
        session,
        &snippets,
        &|stage| {
            *stage_now.lock().unwrap() = (stage, std::time::Instant::now());
            emit_progress(app, stage, susurro_core::progress_for(stage, 0));
        },
    )
    .map_err(|e| {
        progress_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        cues.play(susurro_adapters_audio::Cue::Error);
        let msg = match e {
            susurro_core::CoreError::Transcription(m) => {
                format!("Couldn't transcribe. Using local instead? {m}")
            }
            susurro_core::CoreError::Injection(m) => {
                format!("Couldn't paste. Is ydotoold running? {m}")
            }
            other => format!("{other}"),
        };
        emit_error(app, &msg);
        msg
    })?;
    progress_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = progress_thread.join();
    cues.play(susurro_adapters_audio::Cue::Done);

    let result = UtteranceResult {
        raw: out.raw_text.clone(),
        cleaned: out.cleaned_text.clone(),
        latency_ms: t0.elapsed().as_millis() as u64,
    };
    // Shared history with the CLI (v0.9.0, issue 43): GUI dictations
    // feed the same stats and restore views. Best-effort, never blocks.
    if let Ok(mut history) = susurro_storage::SqliteHistory::open(&shared_db_path()) {
        use susurro_core::ports::HistoryStorePort;
        let _ = history.upsert(susurro_core::ports::HistoryEntry {
            session,
            raw_text: out.raw_text,
            cleaned_text: Some(out.cleaned_text),
            provider: "local".into(),
            latency_ms: result.latency_ms,
            app: focused.clone(),
            created_at: 0,
        });
    }
    let _ = app.emit("susurro://result", &result);
    emit_state(app, "done");
    Ok(result)
}

#[derive(Clone, Serialize)]
struct DragAnchor {
    address: String,
    x: i32,
    y: i32,
}

/// hyprctl client list as JSON. Linux-only: the drag path below is
/// the sole caller.
#[cfg(target_os = "linux")]
fn hyprland_clients() -> Result<serde_json::Value, String> {
    let out = susurro_core::silent_command("hyprctl")
        .args(["clients", "-j"])
        .output()
        .map_err(|e| format!("Couldn't run hyprctl (Hyprland only): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "hyprctl clients failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("hyprctl returned non-JSON: {e}"))
}

/// Find our pill window: Wayland app id first, title fallback.
/// Linux-only with its callers.
#[cfg(target_os = "linux")]
fn pill_address(clients: &serde_json::Value) -> Option<(String, i32, i32)> {
    clients.as_array()?.iter().find_map(|c| {
        let class = c.get("class")?.as_str()?;
        let title = c.get("title").and_then(|t| t.as_str()).unwrap_or("");
        if class != "susurro-app" && title != "Susurro" {
            return None;
        }
        let address = c.get("address")?.as_str()?.to_string();
        let at = c.get("at")?.as_array()?;
        let x = at.first()?.as_i64()? as i32;
        let y = at.get(1)?.as_i64()? as i32;
        Some((address, x, y))
    })
}

#[cfg(target_os = "linux")]
fn hyprland_move(address: &str, x: i32, y: i32) -> Result<(), String> {
    // Argv shape proven live: address rides the last param after a comma.
    let out = susurro_core::silent_command("hyprctl")
        .args([
            "dispatch".to_string(),
            "movewindowpixel".to_string(),
            "exact".to_string(),
            x.to_string(),
            format!("{y},address:{address}"),
        ])
        .output()
        .map_err(|e| format!("Couldn't run hyprctl (Hyprland only): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "hyprctl move failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Drag trace for real-session diagnosis: appends one line per drag
/// event to /tmp/susurro-drag.log so a failed drag leaves evidence
/// instead of silence. Always on, drag-only volume. Best-effort.
#[cfg(target_os = "linux")]
fn trace_drag(line: &str) {
    use std::io::Write;
    let path = std::path::PathBuf::from("/tmp/susurro-drag.log");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    if let Ok(f) = std::fs::metadata(&path) {
        if f.len() > 50_000 {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut f) = opts.open(&path) {
        let _ = writeln!(
            f,
            "{} {line}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
    }
}

/// Drag session anchor: resolves the pill address plus its current
/// top-left, then validates the move path with a no-op dispatch to
/// the same spot. Any failure falls back to the platform drag on the
/// frontend, so this command failing is routine, never fatal.
#[tauri::command]
fn pill_drag_start() -> Result<DragAnchor, String> {
    #[cfg(not(target_os = "linux"))]
    {
        Err("Hyprland drag needs Linux.".into())
    }
    #[cfg(target_os = "linux")]
    {
        let clients = hyprland_clients()?;
        let (address, x, y) =
            pill_address(&clients).ok_or_else(|| "pill window not found in hyprctl clients.".to_string())?;
        match hyprland_move(&address, x, y) {
            Ok(()) => {
                trace_drag(&format!("start ok {address} {x},{y}"));
                Ok(DragAnchor { address, x, y })
            }
            Err(e) => {
                trace_drag(&format!("start err {e}"));
                Err(e)
            }
        }
    }
}

/// One drag step: absolute top-left for the pill window.
#[tauri::command]
fn pill_drag_move(address: String, x: i32, y: i32) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (address, x, y);
        Err("Hyprland drag needs Linux.".into())
    }
    #[cfg(target_os = "linux")]
    {
        match hyprland_move(&address, x, y) {
            Ok(()) => {
                trace_drag(&format!("move ok {x},{y}"));
                Ok(())
            }
            Err(e) => {
                trace_drag(&format!("move err {e}"));
                Err(e)
            }
        }
    }
}

/// Frontend-invoked dictation (pill button / tray). Blocks; progress via events.
/// A second trigger while busy reports busy instead of stacking runs.
#[tauri::command]
fn start_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UtteranceResult, String> {
    if !state.try_claim() {
        return Err("already dictating. Wait for this run to finish.".into());
    }
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let tickets = TicketRegistry::new();
    show_pill(&app, settings.sound);
    let out = run_dictation(
        &app,
        &settings,
        &tickets,
        bundled_of(&state).as_deref(),
        bundled_punct_of(&state).as_deref(),
    );
    state.release();
    out
}

/// Background hotkey listener: each press dictates. The hotkey is
/// rebuilt every press from settings, so a remap applies without a
/// restart. Anything else sleeps instead of spinning.
fn spawn_hotkey_listener(app: AppHandle, state: Arc<AppState>) {
    use susurro_core::ports::GlobalHotkeyPort;
    std::thread::spawn(move || {
        let tickets = TicketRegistry::new();
        loop {
            #[cfg(target_os = "linux")]
            let hotkey: Box<dyn GlobalHotkeyPort> = {
                let socket_path = state
                    .settings
                    .lock()
                    .map(|s| s.socket_path.clone())
                    .unwrap_or_else(|_| "/tmp/susurro.sock".into());
                Box::new(susurro_adapters_linux::HyprlandSocket::new(&socket_path))
            };
            #[cfg(target_os = "windows")]
            let hotkey: Box<dyn GlobalHotkeyPort> = {
                let name = state
                    .settings
                    .lock()
                    .map(|s| s.hotkey.clone())
                    .unwrap_or_else(|_| default_hotkey());
                Box::new(susurro_adapters_windows::hotkey_from_name(&name))
            };
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            let hotkey: Box<dyn GlobalHotkeyPort> = {
                // No listener here: sleep forever instead of hot-spinning.
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            };
            if hotkey.wait_for_hotkey().is_err() {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            // A press mid-run is a bounce, not a queue: the in-flight
            // run owns the mic until it finishes.
            if !state.try_claim() {
                continue;
            }
            show_pill(&app, state.settings.lock().map(|s| s.sound).unwrap_or(true));
            let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
            let _ = run_dictation(
        &app,
        &settings,
        &tickets,
        bundled_of(&state).as_deref(),
        bundled_punct_of(&state).as_deref(),
    );
            state.release();
        }
    });
}

fn build_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let dictate = MenuItem::with_id(app, "dictate", "Dictate now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&dictate, &settings, &quit])?;
    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Susurro — talk-to-text")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "dictate" => {
                let handle = app.clone();
                let sound = {
                    let state: State<'_, Arc<AppState>> = handle.state();
                    state.settings.lock().map(|s| s.sound).unwrap_or(true)
                };
                show_pill(app, sound);
                std::thread::spawn(move || {
                    let state: State<'_, Arc<AppState>> = handle.state();
                    if !state.try_claim() {
                        return;
                    }
                    let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
                    let tickets = TicketRegistry::new();
                    let _ = run_dictation(
            &handle,
            &settings,
            &tickets,
            bundled_of(&state).as_deref(),
            bundled_punct_of(&state).as_deref(),
        );
                    state.release();
                });
            }
            "settings" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                // Stop the convergence loop before exit: a thread
                // parked on a 15-minute backoff would outlive the
                // window the user just closed.
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    state.convergence_stop.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                app.exit(0)
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn dirs_fallback() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("com.susurro.app");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/com.susurro.app");
    }
    PathBuf::from("/tmp/com.susurro.app")
}

fn main() {
    let ctx_dir = dirs_fallback();
    let app_state = Arc::new(AppState {
        settings: Mutex::new(AppState::load(&ctx_dir)),
        dir: ctx_dir,
        inflight: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        bundled_tiny: Mutex::new(None),
        bundled_punct: Mutex::new(None),
        prefetch: Mutex::new(None),
        convergence_stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let hotkey_state = app_state.clone();
    let resource_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            start_dictation,
            get_settings,
            save_settings,
            run_doctor,
            list_history,
            restore_session,
            list_format_profiles,
            save_format_profile,
            remove_format_profile,
            list_snippets,
            save_snippet,
            remove_snippet,
            list_dictionary,
            save_word,
            remove_word,
            apply_intent,
            wipe_data,
            show_onboarding,
            onboarding_status,
            requirements_status,
            system_status,
            converge_status,
            download_model,
            start_model_prefetch,
            run_bench,
            test_dictation,
            hotkey_snippet,
            finish_onboarding,
            get_stats,
            pill_drag_start,
            pill_drag_move
        ])
        .setup(move |app| {
            spawn_hotkey_listener(app.handle().clone(), hotkey_state.clone());
            // Day-0 model: resolve the bundled tiny once, so every
            // later resolution finds it without an AppHandle.
if let Ok(resource_dir) = app.path().resource_dir() {
                    let bundled = resource_dir.join("models/tiny.en.bin");
                    if bundled.exists() {
                        *resource_state.bundled_tiny.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(bundled);
                    }
                    // Cleanup model pair, same deal (ADR-004 Phase 4).
                    let punct = resource_dir.join("models");
                    if punct.join(susurro_provision::PUNCT_MODEL_NAME).exists()
                        && punct.join(susurro_provision::PUNCT_VOCAB_NAME).exists()
                    {
                        *resource_state.bundled_punct.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(punct);
                    }
                }
            build_tray(app.handle())?;
            // Runtime DLL search path first (ADR-004 Phase 4): the
            // punctuation model cannot load without it, and dictation
            // must not be the thing that discovers the problem.
            if let Some(dir) = extend_dll_search_path(app.handle()) {
                eprintln!("runtime dll path: {}", dir.display());
            }
            // Convergence loop on boot (ADR-004 point 5). It runs
            // beside onboarding: the bundled tiny already makes
            // dictation work offline, so the loop only fills in what
            // the bundle cannot carry.
            let converge_handle = app.handle().clone();
            let converge_state = app.state::<Arc<AppState>>().inner().clone();
            spawn_convergence(converge_handle, converge_state);
            // Startup inventory: every window with its label, visibility,
            // and URL. Diagnosing wrong-window reports starts here.
            for (label, w) in app.webview_windows() {
                eprintln!(
                    "window: label={label} visible={:?} url={:?}",
                    w.is_visible().unwrap_or(false),
                    w.url().map(|u| u.to_string()).unwrap_or_default(),
                );
            }
            let done = app
                .state::<Arc<AppState>>()
                .settings
                .lock()
                .map(|s| s.onboarding_done)
                .unwrap_or(true);
            if !done {
                if let Some(w) = app.get_webview_window("onboarding") {
                    let _ = w.show();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Tray app behavior: closing a window hides it instead of
            // destroying it, so tray entries always have a window to
            // show. Quit stays on the tray menu.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("susurro failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_defaults_are_sane() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "super_shift_r");
        assert!(!s.high_contrast);
        assert!(s.announce);
        assert!(!s.onboarding_done);
    }

    #[test]
    fn old_settings_files_migrate_forward() {
        // Pre-accessibility files lack the new keys: they parse with
        // defaults instead of failing the load.
        let s: Settings = serde_json::from_str(r#"{"seconds":30}"#).unwrap();
        assert_eq!(s.hotkey, "super_shift_r");
        assert!(!s.high_contrast);
        assert!(s.announce);
        // Full roundtrip keeps explicit choices.
        let full = Settings {
            high_contrast: true,
            announce: false,
            hotkey: "shift_d".into(),
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert!(back.high_contrast);
        assert!(!back.announce);
        assert_eq!(back.hotkey, "shift_d");
    }

    fn resolve_tmp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-resolve-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolution_prefers_tier_then_quality_then_bundled() {
        let models = resolve_tmp("models");
        let tiny = models.join("tiny.en.bin");
        std::fs::write(&tiny, b"tiny").unwrap();
        let bundled_dir = resolve_tmp("bundled");
        let bundled = bundled_dir.join("tiny.en.bin");
        std::fs::write(&bundled, b"bundled").unwrap();

        // Explicit wins even when missing: the caller named it.
        assert_eq!(
            resolve_whisper_with("/explicit/m.bin", None, None, Some(&models), Some(&bundled)),
            "/explicit/m.bin"
        );
        // Tier file wins over everything on disk.
        let tier_dir = resolve_tmp("tier");
        let tier = tier_dir.join("small.en.bin");
        std::fs::write(&tier, b"small").unwrap();
        assert_eq!(
            resolve_whisper_with("", None, Some(&tier), Some(&models), Some(&bundled)),
            tier.to_string_lossy().into_owned()
        );
        // Missing tier falls through to best quality on disk.
        let base = models.join("base.en.bin");
        std::fs::write(&base, b"base").unwrap();
        assert_eq!(
            resolve_whisper_with(
                "",
                None,
                Some(&models.join("absent.bin")),
                Some(&models),
                Some(&bundled)
            ),
            base.to_string_lossy().into_owned()
        );
        // Empty models dir falls through to the bundled day-0 tiny.
        let empty = resolve_tmp("empty");
        assert_eq!(
            resolve_whisper_with("", None, None, Some(&empty), Some(&bundled)),
            bundled.to_string_lossy().into_owned()
        );
        // Nothing anywhere names the missing base path.
        let missing = resolve_whisper_with("", None, None, Some(&empty), None);
        assert!(missing.ends_with("base.en.bin"), "{missing}");
    }
}
