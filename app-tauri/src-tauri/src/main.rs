//! Susurro Tauri backend (v0.1.0): pill overlay, tray, settings,
//! updater wiring. The dictation pipeline reuses the workspace crates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use susurro_adapters_audio::{EndpointDecision, VadEndpoint};
use susurro_core::ports::{AudioCapturePort, TextInjectionPort, TextPostProcessorPort};
use susurro_core::{Pipeline, SessionId, TicketRegistry};
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
        }
    }
}

struct AppState {
    settings: Mutex<Settings>,
    dir: PathBuf,
}

impl AppState {
    fn settings_file(dir: &std::path::Path) -> PathBuf {
        dir.join("settings.json")
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
        let found = std::process::Command::new(probe)
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
            // Window is 420x72 logical; place center-x, ~92% down.
            // Below 768p screens this clips a few pixels; dragging
            // overrides the dock wherever the compositor honors moves.
            let x = size.width / 2.0 - 420.0 / 2.0;
            let y = size.height * 0.92;
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
    let _ = app.emit("susurro://context", serde_json::json!({ "app": focused }));
    let cues = susurro_adapters_audio::CuePlayer::new(settings.sound);

    let pcm = capture_pcm(app, settings, &cues)?;

    emit_state(app, "processing");
    let stt = susurro_adapters_stt_local::WhisperLocal::base_en(
        resolve_whisper(&settings.whisper_model).into(),
    );
    let passthrough = susurro_adapters_cleanup::PassthroughCleanup;
    let regex = susurro_adapters_cleanup::RegexCleanup;
    let ollama = susurro_adapters_cleanup::OllamaCleanup::new(&settings.ollama_model);
    let cleanup: &dyn TextPostProcessorPort = match settings.cleanup.as_str() {
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
    let out = Pipeline::run_staged(
        &mut capture,
        &stt,
        cleanup,
        &GuiInjector,
        tickets,
        SessionId::generate(),
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
        raw: out.raw_text,
        cleaned: out.cleaned_text,
        latency_ms: t0.elapsed().as_millis() as u64,
    };
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
    let out = std::process::Command::new("hyprctl")
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
    let out = std::process::Command::new("hyprctl")
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
        let (address, x, y) = pill_address(&clients)
            .ok_or_else(|| "pill window not found in hyprctl clients.".to_string())?;
        hyprland_move(&address, x, y)?;
        Ok(DragAnchor { address, x, y })
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
        hyprland_move(&address, x, y)
    }
}

/// Frontend-invoked dictation (pill button / tray). Blocks; progress via events.
#[tauri::command]
fn start_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UtteranceResult, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let tickets = TicketRegistry::new();
    show_pill(&app, settings.sound);
    run_dictation(&app, &settings, &tickets)
}

/// Background hotkey listener: each press dictates. The source is
/// platform-owned: Hyprland socket on Linux, RegisterHotKey on
/// Windows. Anything else sleeps instead of spinning.
fn spawn_hotkey_listener(app: AppHandle, state: Arc<AppState>) {
    use susurro_core::ports::GlobalHotkeyPort;
    std::thread::spawn(move || {
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
        let hotkey: Box<dyn GlobalHotkeyPort> =
            Box::new(susurro_adapters_windows::WindowsHotkey::with_defaults());
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let hotkey: Box<dyn GlobalHotkeyPort> = {
            // No listener here: sleep forever instead of hot-spinning.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        };
        let tickets = TicketRegistry::new();
        loop {
            if hotkey.wait_for_hotkey().is_err() {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            show_pill(&app, state.settings.lock().map(|s| s.sound).unwrap_or(true));
            let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
            let _ = run_dictation(&app, &settings, &tickets);
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
                    let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
                    let tickets = TicketRegistry::new();
                    let _ = run_dictation(&handle, &settings, &tickets);
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
            pill_drag_start,
            pill_drag_move
        ])
        .setup(move |app| {
            spawn_hotkey_listener(app.handle().clone(), hotkey_state.clone());
            build_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("susurro failed to start");
}
