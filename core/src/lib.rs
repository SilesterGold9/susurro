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
pub mod snippets;
pub mod stages;
pub mod state;
pub mod stats;
pub mod suggest;
pub mod transforms;
pub mod words;

pub use error::CoreError;
pub use event_log::{now_ms, EventKind, SessionEvent};
pub use format::{matched_profile, FormatProfile, Style};
pub use pipeline::Pipeline;
pub use privacy::{PrivacyPolicy, DEFAULT_BLOCKLIST};
pub use proc::silent_command;
pub use session::{SessionId, Ticket, TicketRegistry};
pub use snippets::{find_expansion, normalize_trigger, Snippet};
pub use stages::{progress_for, Stage};
pub use state::State;
pub use stats::{day_index, day_label, percentile, summarize, DayCount, Summary};
pub use suggest::{suggest_phrases, PhraseSuggestion};
pub use words::{preserves_words, word_f1, F1_MINIMUM};
