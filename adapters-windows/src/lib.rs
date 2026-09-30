//! Windows adapters (v0.6.0, issue 30).
//!
//! - `WindowsHotkey`: global hotkey via `RegisterHotKey`, blocking
//!   on the message loop until `WM_HOTKEY` arrives.
//! - `WindowsSendInput`: direct Unicode typing via `SendInput` with
//!   `KEYEVENTF_UNICODE`, one batched call. The clipboard survives,
//!   mirroring the wtype fast path on Linux.
//!
//! The crate compiles on all platforms: the Win32 calls are
//! `cfg(windows)`-gated and every other target gets an actionable
//! error. Pure helpers (UTF-16 expansion, hotkey validation) are
//! cfg-free so Linux CI proves the logic while Windows CI proves
//! the FFI.

use susurro_core::ports::{GlobalHotkeyPort, HotkeyEvent, TextInjectionPort};
use susurro_core::{CoreError, Ticket};

/// Default hotkey: Win+Shift+R, mirroring the Hyprland SUPER_SHIFT+R
/// bind on Linux. Virtual-key code for 'R'.
pub const DEFAULT_MODIFIERS_WIN: u32 = 0x0008 | 0x0004;
pub const DEFAULT_VK_R: u32 = 0x52;

/// Single id for our hotkey registration. One hotkey per process.
pub const HOTKEY_ID: i32 = 1;

/// Global hotkey via `RegisterHotKey`.
pub struct WindowsHotkey {
    pub modifiers: u32,
    pub vk: u32,
}

impl WindowsHotkey {
    pub fn new(modifiers: u32, vk: u32) -> Self {
        Self { modifiers, vk }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_MODIFIERS_WIN, DEFAULT_VK_R)
    }
}

impl Default for WindowsHotkey {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Validate a hotkey before registering: zero modifiers or zero key
/// never registers usefully and fails late inside Win32.
pub fn validate_hotkey(modifiers: u32, vk: u32) -> Result<(), String> {
    if modifiers == 0 {
        return Err("hotkey needs at least one modifier (win, shift, ctrl, alt)".into());
    }
    if vk == 0 {
        return Err("hotkey needs a non-zero virtual-key code".into());
    }
    Ok(())
}

impl GlobalHotkeyPort for WindowsHotkey {
    fn wait_for_hotkey(&self) -> Result<HotkeyEvent, CoreError> {
        #[cfg(not(target_os = "windows"))]
        {
            Err(CoreError::Config(
                "Windows hotkey needs Windows. Expected on Linux CI.".into(),
            ))
        }
        #[cfg(target_os = "windows")]
        {
            validate_hotkey(self.modifiers, self.vk).map_err(CoreError::Config)?;
            // SAFETY: RegisterHotKey with a null window posts WM_HOTKEY
            // to this thread's queue; GetMessageW pumps it. No window,
            // no callback, no shared state.
            unsafe {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                    RegisterHotKey, UnregisterHotKey,
                };
                use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
                if RegisterHotKey(std::ptr::null_mut(), HOTKEY_ID, self.modifiers, self.vk) == 0 {
                    return Err(CoreError::Config(format!(
                        "RegisterHotKey failed (in use?). Pick another hotkey: {}",
                        windows_sys::Win32::Foundation::GetLastError()
                    )));
                }
                let mut msg = std::mem::zeroed::<MSG>();
                loop {
                    let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                    if ret == 0 {
                        // WM_QUIT: unregister and report, never hang.
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
                        return Err(CoreError::Config(
                            "hotkey loop got WM_QUIT. Restart the daemon.".into(),
                        ));
                    }
                    if ret == -1 {
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
                        return Err(CoreError::Config(format!(
                            "hotkey pump failed: {}",
                            windows_sys::Win32::Foundation::GetLastError()
                        )));
                    }
                    if msg.message == WM_HOTKEY {
                        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
                        return Ok(HotkeyEvent::ToggleDictation);
                    }
                }
            }
        }
    }
}

/// UTF-16 units for `text`, surrogates split. Pure so both CI
/// runners prove the expansion; only Windows turns units into INPUTs.
pub fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// One SendInput batch holds key-down plus key-up per unit.
pub fn input_count_for(units: &[u16]) -> usize {
    units.len() * 2
}

/// Direct Unicode injector via `SendInput`. Types the text into the
/// focused window without touching the clipboard. Empty text injects
/// nothing and skips the FFI entirely.
pub struct WindowsSendInput;

impl TextInjectionPort for WindowsSendInput {
    fn inject(&self, text: &str, _ticket: &Ticket) -> Result<(), CoreError> {
        // Exactly-once lives in the pipeline ticket gate above this
        // port, same as the Linux injector: no per-port dedup here.
        if text.is_empty() {
            return Ok(());
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err(CoreError::Injection(
                "SendInput injection needs Windows. Expected on Linux CI.".into(),
            ))
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY: INPUT array is built from the units below, length
            // checked before the call, pointer valid for the call only.
            unsafe {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
                let units = utf16_units(text);
                let mut inputs: Vec<INPUT> = Vec::with_capacity(input_count_for(&units));
                for unit in &units {
                    inputs.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: 0,
                                wScan: *unit,
                                dwFlags: KEYEVENTF_UNICODE,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    });
                    inputs.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: 0,
                                wScan: *unit,
                                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    });
                }
                let sent = SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr(),
                    std::mem::size_of::<INPUT>() as i32,
                );
                if sent as usize != inputs.len() {
                    return Err(CoreError::Injection(format!(
                        "SendInput typed {} of {} inputs: {}",
                        sent,
                        inputs.len(),
                        windows_sys::Win32::Foundation::GetLastError()
                    )));
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mirror_linux_bind() {
        let h = WindowsHotkey::with_defaults();
        // Win (0x8) plus Shift (0x4), 'R'.
        assert_eq!(h.modifiers, 0x0008 | 0x0004);
        assert_eq!(h.vk, 0x52);
    }

    #[test]
    fn validation_rejects_zero_parts() {
        assert!(validate_hotkey(0x0004, 0x52).is_ok());
        assert!(validate_hotkey(0, 0x52).is_err());
        assert!(validate_hotkey(0x0004, 0).is_err());
    }

    #[test]
    fn utf16_expansion_splits_surrogates() {
        // 'a' plus U+1D11E plus 'b': lone units count, pairs split.
        assert_eq!(utf16_units("a𝄞b"), vec![0x61, 0xD834, 0xDD1E, 0x62]);
        assert_eq!(utf16_units(""), Vec::<u16>::new());
        assert_eq!(input_count_for(&utf16_units("hi")), 4);
        assert_eq!(input_count_for(&[]), 0);
    }

    #[test]
    fn stubs_error_actionably_off_windows() {
        let ticket = Ticket::new(susurro_core::SessionId::new(1), "inject");
        // Empty text never reaches the FFI on any platform.
        assert!(WindowsSendInput.inject("", &ticket).is_ok());
        #[cfg(not(target_os = "windows"))]
        {
            assert!(WindowsHotkey::with_defaults().wait_for_hotkey().is_err());
            assert!(WindowsSendInput.inject("hi", &ticket).is_err());
        }
    }

    /// Real registration roundtrip, Windows CI only: proves the flags
    /// and id reach Win32 and come back. No message loop, never blocks.
    /// The default bind can be held by the host (1409 on CI runners);
    /// then an obscure fallback combo still proves the mechanics, and
    /// the code lands in the message either way.
    #[test]
    #[cfg(target_os = "windows")]
    fn hotkey_registers_and_releases() {
        unsafe {
            use windows_sys::Win32::Foundation::GetLastError;
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                RegisterHotKey, UnregisterHotKey,
            };
            // Win+Shift+R, else Ctrl+Alt+F24.
            let combos = [
                (HOTKEY_ID, DEFAULT_MODIFIERS_WIN, DEFAULT_VK_R),
                (HOTKEY_ID + 1, 0x0002 | 0x0001, 0x87),
            ];
            let mut registered = None;
            for (id, mods, vk) in combos {
                if RegisterHotKey(std::ptr::null_mut(), id, mods, vk) != 0 {
                    registered = Some(id);
                    break;
                }
            }
            let id = registered.unwrap_or_else(|| {
                panic!("RegisterHotKey failed, GetLastError={}", GetLastError())
            });
            assert_ne!(UnregisterHotKey(std::ptr::null_mut(), id), 0);
        }
    }
}
