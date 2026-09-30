//! Linux adapters (v0.0.1): Hyprland hotkey + clipboard-paste injection.
//!
//! Compiles on all platforms: Linux-specific process spawning is
//! runtime-gated, and non-Linux targets get an actionable error.
//! This keeps Windows CI green while Linux behaviour is proven.

use susurro_core::ports::{GlobalHotkeyPort, HotkeyEvent, TextInjectionPort};
use susurro_core::{CoreError, Ticket};

/// Listen on a Unix socket for a Hyprland `bind = SUPER_SHIFT_R, exec, ...`
/// that echoes into it. Example Hyprland snippet (docs, not code):
/// `bind = SUPER_SHIFT, R, exec, echo toggle | socat - UNIX-CONNECT:/tmp/susurro.sock`
pub struct HyprlandSocket {
    pub socket_path: String,
}

impl HyprlandSocket {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn bind_snippet(&self) -> String {
        format!(
            "bind = SUPER_SHIFT, R, exec, echo toggle | socat - UNIX-CONNECT:{}",
            self.socket_path
        )
    }
}

impl GlobalHotkeyPort for HyprlandSocket {
    fn wait_for_hotkey(&self) -> Result<HotkeyEvent, CoreError> {
        #[cfg(not(target_os = "linux"))]
        {
            Err(CoreError::Config(
                "Hyprland hotkey is Linux-only. This is expected on Windows CI.".into(),
            ))
        }
        #[cfg(target_os = "linux")]
        {
            use std::io::{BufRead, BufReader};
            use std::os::unix::net::UnixListener;
            let _ = std::fs::remove_file(&self.socket_path);
            let listener = UnixListener::bind(&self.socket_path)
                .map_err(|e| CoreError::Config(format!("bind {} failed: {e}", self.socket_path)))?;
            let (stream, _) = listener
                .accept()
                .map_err(|e| CoreError::Config(format!("hotkey accept failed: {e}")))?;
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|e| CoreError::Config(format!("hotkey read failed: {e}")))?;
            Ok(HotkeyEvent::ToggleDictation)
        }
    }
}

/// Injection in one subprocess where possible (v0.4.0, issue 25).
/// `wtype` types the text directly: one spawn, one Wayland roundtrip,
/// and the user clipboard survives. Without it, the clipboard path
/// below runs: `wl-copy <text>` then `ydotool key ctrl+v` in a single
/// batched key sequence. Copy-only mode (`use_ydotool` false) skips
/// keystrokes on both paths.
pub struct LinuxPasteInjector {
    pub use_ydotool: bool,
}

impl LinuxPasteInjector {
    pub fn new() -> Self {
        Self { use_ydotool: true }
    }
}

impl Default for LinuxPasteInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// wtype invocation for `text`: `--` ends option parsing so leading
/// dashes type literally, and argv carries the text with no shell.
/// Linux-only helper: the whole wtype path is compiled out elsewhere.
#[cfg(target_os = "linux")]
fn wtype_command(text: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("wtype");
    cmd.arg("--").arg(text);
    cmd
}

/// Outcome of the wtype attempt: typed, missing (fall back to the
/// clipboard path), or failed at runtime (error out, never double-paste
/// by falling back after a tool already ran).
#[cfg(target_os = "linux")]
enum WtypeOutcome {
    Typed,
    Missing,
    Failed(String),
}

#[cfg(target_os = "linux")]
fn try_wtype(text: &str) -> WtypeOutcome {
    match wtype_command(text).status() {
        Err(_) => WtypeOutcome::Missing,
        Ok(status) if status.success() => WtypeOutcome::Typed,
        Ok(status) => WtypeOutcome::Failed(format!(
            "wtype exited with {status}. Check compositor virtual-keyboard support."
        )),
    }
}

impl TextInjectionPort for LinuxPasteInjector {
    fn inject(&self, text: &str, _ticket: &Ticket) -> Result<(), CoreError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = text;
            Err(CoreError::Injection(
                "Linux paste injection is Linux-only. Expected on Windows CI.".into(),
            ))
        }
        #[cfg(target_os = "linux")]
        {
            use std::process::{Command, Stdio};
            if text.is_empty() {
                return Ok(());
            }
            if self.use_ydotool {
                match try_wtype(text) {
                    WtypeOutcome::Typed => return Ok(()),
                    WtypeOutcome::Failed(msg) => {
                        return Err(CoreError::Injection(msg));
                    }
                    WtypeOutcome::Missing => {}
                }
            }
            // 1. Put text on Wayland clipboard.
            let mut child = Command::new("wl-copy")
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    CoreError::Injection(format!(
                        "Couldn't run wl-copy. Install wl-clipboard, then retry: {e}"
                    ))
                })?;
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .ok_or_else(|| CoreError::Injection("wl-copy stdin missing".into()))?
                .write_all(text.as_bytes())
                .map_err(|e| CoreError::Injection(format!("wl-copy write failed: {e}")))?;
            let status = child
                .wait()
                .map_err(|e| CoreError::Injection(format!("wl-copy wait failed: {e}")))?;
            if !status.success() {
                return Err(CoreError::Injection(
                    "wl-copy failed. Install wl-clipboard, then retry.".into(),
                ));
            }
            // 2. Paste via ydotool in one batched key sequence.
            // NOTE: ydotool >= 1.0 takes raw keycodes only
            // (KEY_LEFTCTRL=29, KEY_V=47); names like "ctrl+v" silently no-op.
            if self.use_ydotool {
                let st = Command::new("ydotool")
                    .args(["key", "29:1", "47:1", "47:0", "29:0"])
                    .status()
                    .map_err(|e| {
                        CoreError::Injection(format!(
                            "Couldn't run ydotool. Start ydotoold and check permissions: {e}"
                        ))
                    })?;
                if !st.success() {
                    return Err(CoreError::Injection(
                        "ydotool paste failed. Is ydotoold running? See `susurro doctor`.".into(),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Hardware-free injector for tests: records what would be pasted.
pub struct MockInjector {
    pub seen: std::sync::Mutex<Vec<String>>,
}

impl MockInjector {
    pub fn new() -> Self {
        Self {
            seen: Default::default(),
        }
    }
}

impl Default for MockInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInjectionPort for MockInjector {
    fn inject(&self, text: &str, _ticket: &Ticket) -> Result<(), CoreError> {
        self.seen.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

/// Focused app class on Hyprland for privacy routing (v0.3.0, issue 21).
/// Runs `hyprctl activewindow -j` and reads the `class` field.
/// Returns None off Linux, when hyprctl is missing, or when parsing fails.
/// Never errors, so a broken detector fails open to the normal chain.
pub fn focused_app() -> Option<String> {
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("hyprctl")
            .args(["activewindow", "-j"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_active_window_class(&String::from_utf8_lossy(&out.stdout))
    }
}

/// Parse the `class` field from `hyprctl activewindow -j` output.
pub fn parse_active_window_class(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let class = v.get("class")?.as_str()?.trim();
    if class.is_empty() {
        return None;
    }
    Some(class.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_hyprland_active_window() {
        let sample = r#"{"address":"0x1234","class":"kitty","title":"shell"}"#;
        assert_eq!(
            super::parse_active_window_class(sample).as_deref(),
            Some("kitty")
        );
        assert_eq!(super::parse_active_window_class(r#"{"class":"  "}"#), None);
        assert_eq!(super::parse_active_window_class("not json"), None);
        assert_eq!(super::parse_active_window_class(r#"{"title":"x"}"#), None);
    }

    #[test]
    fn focused_app_never_panics() {
        // Missing hyprctl or no compositor must yield None, never a panic.
        let _ = super::focused_app();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn wtype_command_types_text_without_shell() {
        let dbg = format!("{:?}", super::wtype_command("hello -- world"));
        assert!(dbg.contains("wtype"), "{dbg}");
        assert!(dbg.contains("\"--\""), "{dbg}");
        assert!(dbg.contains("hello -- world"), "{dbg}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn empty_text_injects_nothing() {
        let injector = super::LinuxPasteInjector::new();
        let ticket = susurro_core::Ticket::new(susurro_core::SessionId::new(1), "inject");
        assert!(super::TextInjectionPort::inject(&injector, "", &ticket).is_ok());
    }

    /// Fake tool bin dir on PATH. Serializes PATH mutation across tests.
    /// Linux-only: the fakes are shell scripts.
    #[cfg(target_os = "linux")]
    mod path_tests {
        use std::sync::{Mutex, OnceLock};

        fn path_lock() -> &'static Mutex<()> {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
        }

        fn fake_bin(name: &str, body: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "susurro-fakebin-{}",
                susurro_core::SessionId::generate()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            dir
        }

        fn with_path(dir: &std::path::Path, f: impl FnOnce()) {
            struct Restore(String);
            impl Drop for Restore {
                fn drop(&mut self) {
                    std::env::set_var("PATH", &self.0);
                }
            }
            let prior = std::env::var("PATH").unwrap_or_default();
            let _restore = Restore(prior.clone());
            // Prepend: fakes shadow system tools, system tools (sh, cat)
            // stay reachable. Assumes no system wtype, true on CI images.
            std::env::set_var("PATH", format!("{}:{prior}", dir.display()));
            f();
        }

        #[test]
        fn wtype_wins_and_receives_text() {
            let _guard = path_lock().lock().unwrap();
            let dir = fake_bin(
                "wtype",
                "#!/bin/sh\necho \"$2\" >> \"$WTYPE_LOG\"\nexit 0\n",
            );
            let log = dir.join("typed.log");
            std::env::set_var("WTYPE_LOG", &log);
            with_path(&dir, || {
                let injector = super::super::LinuxPasteInjector::new();
                let ticket = susurro_core::Ticket::new(susurro_core::SessionId::new(2), "inject");
                super::super::TextInjectionPort::inject(&injector, "hello wtype", &ticket).unwrap();
            });
            std::env::remove_var("WTYPE_LOG");
            assert_eq!(std::fs::read_to_string(&log).unwrap(), "hello wtype\n");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn missing_wtype_falls_back_to_clipboard() {
            let _guard = path_lock().lock().unwrap();
            let dir = fake_bin("wl-copy", "#!/bin/sh\ncat >> \"$PASTE_LOG\"\nexit 0\n");
            // ydotool fake alongside so the fallback completes.
            std::fs::write(dir.join("ydotool"), "#!/bin/sh\nexit 0\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("ydotool"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            let log = dir.join("paste.log");
            std::env::set_var("PASTE_LOG", &log);
            with_path(&dir, || {
                let injector = super::super::LinuxPasteInjector::new();
                let ticket = susurro_core::Ticket::new(susurro_core::SessionId::new(3), "inject");
                super::super::TextInjectionPort::inject(&injector, "hello paste", &ticket).unwrap();
            });
            std::env::remove_var("PASTE_LOG");
            assert_eq!(std::fs::read_to_string(&log).unwrap(), "hello paste");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn failing_wtype_errors_without_fallback() {
            let _guard = path_lock().lock().unwrap();
            let dir = fake_bin("wtype", "#!/bin/sh\nexit 3\n");
            with_path(&dir, || {
                let injector = super::super::LinuxPasteInjector::new();
                let ticket = susurro_core::Ticket::new(susurro_core::SessionId::new(4), "inject");
                let err = super::super::TextInjectionPort::inject(&injector, "hello", &ticket)
                    .unwrap_err();
                assert!(err.to_string().contains("wtype"), "{err}");
            });
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
