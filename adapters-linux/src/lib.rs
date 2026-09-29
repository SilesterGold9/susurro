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

/// Clipboard-paste injection: `wl-copy <text>` then `ydotool key ctrl+v`.
/// Falls back to stdout logging when tools are missing so
/// `susurro doctor` can diagnose instead of failing silently.
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
            // 2. Paste via ydotool (or wtype fallback documented in doctor).
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
        return None;
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
}
