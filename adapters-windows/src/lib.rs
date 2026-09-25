//! Windows adapters — stubs until v0.6.0.
//!
//! `RegisterHotKey` + `SendInput` land in v0.6.0. This crate exists
//! from v0.0.1 so CI compiles it on both runners and catches
//! platform assumptions leaking into `core`.

use susurro_core::ports::{GlobalHotkeyPort, HotkeyEvent, TextInjectionPort};
use susurro_core::{CoreError, Ticket};

pub struct WindowsHotkey;

impl GlobalHotkeyPort for WindowsHotkey {
    fn wait_for_hotkey(&self) -> Result<HotkeyEvent, CoreError> {
        Err(CoreError::Config(
            "RegisterHotKey adapter lands in v0.6.0 (issue #30).".into(),
        ))
    }
}

pub struct WindowsSendInput;

impl TextInjectionPort for WindowsSendInput {
    fn inject(&self, _text: &str, _ticket: &Ticket) -> Result<(), CoreError> {
        Err(CoreError::Injection(
            "SendInput adapter lands in v0.6.0 (issue #30).".into(),
        ))
    }
}
