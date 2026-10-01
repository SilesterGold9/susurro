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
            cleanup: "ollama".into(),
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

struct AppState {
    settings: Mutex<Settings>,
    dir: PathBuf,
    /// Double-trigger guard: at most one dictation runs at a time.
    /// A second press while busy reports busy instead of stacking runs.
    inflight: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

fn model_file(name: &str) -> Option<String> {
    let p = models_home()?.join(name);
    if p.exists() {
        return Some(p.to_string_lossy().into_owned());
    }
    None
}

fn resolve_whisper(explicit: &str) -> String {
    if !explicit.is_empty() {
        return shellexpand(explicit);
    }
    if let Ok(m) = std::env::var("SUSURRO_MODEL") {
        let p = shellexpand(&m);
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    // First model actually on disk wins; the error names base.en.
    for name in ["small.en.bin", "tiny.en.bin", "base.en.bin"] {
        if let Some(p) = model_file(name) {
            return p;
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
    let path = resolve_whisper(&settings.whisper_model);
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

/// Last whole percentage in a curl progress-bar chunk, if any.
/// The bar rewrites one carriage-return line ending in `NN.N%`;
/// scanning for the last `%` keeps partial reads convergent.
fn curl_progress_pct(chunk: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(chunk);
    let idx = text.rfind('%')?;
    let digits: String = text[..idx]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let whole = digits.split('.').next().unwrap_or("");
    if whole.is_empty() {
        return None;
    }
    whole.parse::<u64>().ok().filter(|p| *p <= 100)
}

/// System requirements for onboarding screen one (Windows audit):
/// whisper binary, model, and paste tools with per-OS install hints.
/// Nothing here blocks: each item names its own fix.
#[tauri::command]
fn requirements_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let whisper = susurro_core::silent_command("whisper-cli")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("whisper-cli")
                .trim()
                .chars()
                .take(40)
                .collect::<String>()
        });
    let model_path = resolve_whisper(&settings.whisper_model);
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
    let whisper_hint = if whisper.is_some() {
        String::new()
    } else if os == "windows" {
        "Install whisper.cpp for Windows: unzip a whisper-cli.exe build plus its DLLs into one folder, add that folder to PATH (MSVC redist may be required), then recheck.".into()
    } else {
        "Install whisper.cpp (distro package or build from source) so whisper-cli is on PATH, then recheck.".into()
    };
    // Cleanup chain (Windows audit): the Ollama API is identical on
    // every OS, so one probe covers all. The model must be pulled,
    // not just the server up.
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
    let ollama_hint = if ollama_up && ollama_model_present {
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
    }))
}

/// Download base.en from the whisper.cpp release mirror into the
/// models dir. Atomic temp plus rename, so a retry converges instead
/// of leaving a half file behind. Progress rides the onboarding
/// event as percentages parsed from the transfer bar.
#[tauri::command]
fn download_model(app: AppHandle) -> Result<String, String> {
    const MODEL_URL: &str =
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
    let dir = models_home().ok_or_else(|| "no models dir on this machine.".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join("base.en.bin");
    if dest.exists() {
        return Ok(dest.to_string_lossy().into_owned());
    }
    let tmp = dir.join("base.en.bin.tmp");
    let _ = app.emit(
        "susurro://onboarding",
        serde_json::json!({ "step": "model", "state": "downloading", "pct": 0 }),
    );
    // Progress bar on stderr: parse trailing percentages off the
    // carriage-return updates and emit whole points upward only.
    let mut child = susurro_core::silent_command("curl")
        .args([
            "-sSL",
            "--fail",
            "--progress-bar",
            MODEL_URL,
            "-o",
            &tmp.to_string_lossy(),
        ])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run curl (is curl installed?): {e}"))?;
    if let Some(stderr) = child.stderr.take() {
        use std::io::Read;
        let progress_app = app.clone();
        std::thread::spawn(move || {
            let mut last = 0u64;
            let mut buf = [0u8; 1024];
            let mut tail = Vec::new();
            let mut reader: Box<dyn Read> = Box::new(stderr);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        tail.extend_from_slice(&buf[..n]);
                        if let Some(pct) = curl_progress_pct(&tail) {
                            if pct > last {
                                last = pct;
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
                        if tail.len() > 4096 {
                            tail.drain(..tail.len() - 1024);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
    let status = child
        .wait()
        .map_err(|e| format!("model download failed: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        let _ = app.emit(
            "susurro://onboarding",
            serde_json::json!({ "step": "model", "state": "failed" }),
        );
        return Err("model download failed. Check the network and retry.".into());
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    // Trust on first use starts at download: the fresh bytes are the
    // reference every later run compares against.
    if let Ok(mut store) = susurro_storage::SqliteSettings::open(&shared_db_path()) {
        use susurro_adapters_stt_local::checksum::verify_model;
        let _ = verify_model(&dest, &mut store);
    }
    let _ = app.emit(
        "susurro://onboarding",
        serde_json::json!({ "step": "model", "state": "done" }),
    );
    Ok(dest.to_string_lossy().into_owned())
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
    let out = run_dictation(&app, &settings, &tickets);
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

/// Remove a dictionary phrase.
#[tauri::command]
fn remove_word(phrase: String) -> Result<(), String> {
    let store = susurro_storage::SqliteDictionary::open(&shared_db_path())
        .map_err(|e| e.to_string())?;
    store.remove(&phrase).map_err(|e| e.to_string())
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
    let stt = susurro_adapters_stt_local::WhisperLocal::base_en(
        resolve_whisper(&settings.whisper_model).into(),
    );
    let passthrough = susurro_adapters_cleanup::PassthroughCleanup;
    let regex = susurro_adapters_cleanup::RegexCleanup;
    let ollama = susurro_adapters_cleanup::OllamaCleanup::new(&settings.ollama_model);
    let cleanup_name: &str = match profile_style.as_deref() {
        Some("formal") => "ollama",
        Some("casual") => "regex",
        Some("verbatim") => "none",
        _ => settings.cleanup.as_str(),
    };
    let cleanup: &dyn TextPostProcessorPort = match cleanup_name {
        "ollama" => &ollama,
        "regex" => &regex,
        _ => &passthrough,
    };
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
    let out = Pipeline::run_staged(
        &mut capture,
        &stt,
        cleanup,
        &GuiInjector,
        tickets,
        session,
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
    let out = run_dictation(&app, &settings, &tickets);
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
            let _ = run_dictation(&app, &settings, &tickets);
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
                    let _ = run_dictation(&handle, &settings, &tickets);
                    state.release();
                });
            }
            "settings" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
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
    });
    let hotkey_state = app_state.clone();

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
            onboarding_status,
            requirements_status,
            download_model,
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
            build_tray(app.handle())?;
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
}
