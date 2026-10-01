//! Susurro core domain: OS-agnostic state machine, pipeline, and port traits.
//!
//! Rule: this crate must never import a platform-specific crate
//! (no cpal, tauri, winapi, nix, ydotool bindings, etc.).
//! Platform code lives in `adapters-*`. Core only defines behaviour.

pub mod error;
pub mod event_log;
pub mod format;
pub mod hotkey;
pub mod pipeline;
pub mod ports;
pub mod privacy;
pub mod proc;
pub mod session;
pub mod stages;
pub mod state;
pub mod stats;

pub use error::CoreError;
pub use event_log::{now_ms, EventKind, SessionEvent};
pub use format::{matched_profile, FormatProfile, Style};
pub use pipeline::Pipeline;
pub use privacy::{PrivacyPolicy, DEFAULT_BLOCKLIST};
pub use proc::silent_command;
pub use session::{SessionId, Ticket, TicketRegistry};
pub use stages::{progress_for, Stage};
pub use state::State;
pub use stats::{day_index, day_label, percentile, summarize, DayCount, Summary};
