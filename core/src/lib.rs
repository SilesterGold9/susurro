//! Susurro core domain: OS-agnostic state machine, pipeline, and port traits.
//!
//! Rule: this crate must never import a platform-specific crate
//! (no cpal, tauri, winapi, nix, ydotool bindings, etc.).
//! Platform code lives in `adapters-*`. Core only defines behaviour.

pub mod error;
pub mod event_log;
pub mod pipeline;
pub mod ports;
pub mod privacy;
pub mod session;
pub mod stages;
pub mod state;

pub use error::CoreError;
pub use event_log::{now_ms, EventKind, SessionEvent};
pub use pipeline::Pipeline;
pub use privacy::{PrivacyPolicy, DEFAULT_BLOCKLIST};
pub use session::{SessionId, Ticket, TicketRegistry};
pub use stages::{progress_for, Stage};
pub use state::State;
