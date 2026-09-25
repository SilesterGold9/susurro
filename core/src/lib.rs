//! Susurro core domain: OS-agnostic state machine, pipeline, and port traits.
//!
//! Rule: this crate must never import a platform-specific crate
//! (no cpal, tauri, winapi, nix, ydotool bindings, etc.).
//! Platform code lives in `adapters-*`. Core only defines behaviour.

pub mod error;
pub mod pipeline;
pub mod ports;
pub mod session;
pub mod state;

pub use error::CoreError;
pub use pipeline::Pipeline;
pub use session::{SessionId, Ticket, TicketRegistry};
pub use state::State;
