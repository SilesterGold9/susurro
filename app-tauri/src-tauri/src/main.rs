//! Susurro Tauri backend (v0.1.0): pill overlay, tray, settings,
//! updater wiring. The dictation pipeline reuses the workspace crates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use susurro_adapters_audio::{EndpointDecision, VadEndpoint};
use susurro_core::ports::{AudioCapturePort, TextInjectionPort, TextPostProcessorPort};
use susurro_core::{Pipeline, SessionId, TicketRegistry};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    seconds: u64,
    auto_stop: bool,
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
            seconds: 6,
            auto_stop: true,
            cleanup: "ollama".into(),
            ollama_model: "qwen2.5:0.5b".into(),
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
        std::fs::write(&tmp, serde_json::to_string_pretty(&*s).map_err(|e| e.to_string())?)
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
    }
    p.to_string()
}

fn model_file(name: &str) -> Option<String> {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".local/share/susurro/models").join(name);
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
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
        "ydotool",
        "whisper-cli",
        "socat",
        "ollama",
        "curl",
    ] {
        let found = std::process::Command::new("which")
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
        susurro_adapters_linux::LinuxPasteInjector::new().inject(text, ticket)
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
fn show_pill(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("pill") {
        if let Ok(Some(m)) = w.current_monitor() {
            let size = m.size();
            let scale = m.scale_factor();
            // Window is 364x64 logical; place center-x, ~78% down.
            let x = (size.width as f64 / scale / 2.0 - 364.0 / 2.0) as i32;
            let y = (size.height as f64 / scale * 0.78) as i32;
            let _ = w.set_position(tauri::Position::Physical(
                tauri::PhysicalPosition {
                    x: (x as f64 * scale) as i32,
                    y: (y as f64 * scale) as i32,
                },
            ));
        }
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Replay slice peaks over ~1s so the waveform visibly moves between
/// chunk recordings (true streaming lands in v0.4.0).
fn animate_levels(app: AppHandle, chunk: Vec<i16>) {
    std::thread::spawn(move || {
        let n = 10;
        let len = chunk.len();
        for i in 0..n {
            let s = len * i / n;
            let e = len * (i + 1) / n;
            let peak = susurro_adapters_audio::peak_amplitude(&chunk[s..e]);
            emit_level(&app, (peak as f32 / 32767.0).clamp(0.0, 1.0));
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
    });
}

/// Shared dictation run used by the command, tray, and hotkey thread.
fn run_dictation(
    app: &AppHandle,
    settings: &Settings,
    tickets: &TicketRegistry,
) -> Result<UtteranceResult, String> {
    let t0 = std::time::Instant::now();
    emit_state(app, "listening");

    // record_pipewire compiles everywhere and fails actionably where
    // no sound server tooling exists; no cfg gates needed here.
    let pcm = {
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
            emit_error(app, msg);
            return Err(msg.into());
        }
        all
    };

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
    let mut capture = GuiCapture { pcm, done: false };
    let out = Pipeline::run_once(
        &mut capture,
        &stt,
        cleanup,
        &GuiInjector,
        tickets,
        SessionId::generate(),
    )
    .map_err(|e| {
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

    let result = UtteranceResult {
        raw: out.raw_text,
        cleaned: out.cleaned_text,
        latency_ms: t0.elapsed().as_millis() as u64,
    };
    let _ = app.emit("susurro://result", &result);
    emit_state(app, "done");
    Ok(result)
}

/// Frontend-invoked dictation (pill button / tray). Blocks; progress via events.
#[tauri::command]
fn start_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UtteranceResult, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let tickets = TicketRegistry::new();
    show_pill(&app);
    run_dictation(&app, &settings, &tickets)
}

/// Background Hyprland socket listener: each hotkey press dictates.
fn spawn_hotkey_listener(app: AppHandle, state: Arc<AppState>) {
    use susurro_core::ports::GlobalHotkeyPort;
    std::thread::spawn(move || {
        let socket_path = state
            .settings
            .lock()
            .map(|s| s.socket_path.clone())
            .unwrap_or_else(|_| "/tmp/susurro.sock".into());
        let socket = susurro_adapters_linux::HyprlandSocket::new(&socket_path);
        let tickets = TicketRegistry::new();
        loop {
            if socket.wait_for_hotkey().is_err() {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            show_pill(&app);
            let settings = state
                .settings
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
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
                show_pill(app);
                let handle = app.clone();
                std::thread::spawn(move || {
                    let state: State<'_, Arc<AppState>> = handle.state();
                    let settings = state
                        .settings
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_default();
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
            run_doctor
        ])
        .setup(move |app| {
            spawn_hotkey_listener(app.handle().clone(), hotkey_state.clone());
            build_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("susurro failed to start");
}
