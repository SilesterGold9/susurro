//! Session event log (v0.7.0, issue 36).
//!
//! One row per observable moment of a dictation session: start,
//! pipeline stages, winning provider, errors, done. The CLI records
//! them best-effort; `susurro replay` reads them back for debugging.
//! Types only: persistence lives in storage, recording at the edges.

use crate::SessionId;

/// One moment of one session. `at_ms` is unix epoch millis; replay
/// prints times relative to the first event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    pub session: SessionId,
    pub at_ms: u64,
    pub kind: EventKind,
    pub detail: String,
}

/// Fixed vocabulary so replays stay greppable. `Stage` carries the
/// stage name in `detail` ("transcribing", "polishing", "injecting").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Started,
    Stage,
    Provider,
    Error,
    Done,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Stage => "stage",
            Self::Provider => "provider",
            Self::Error => "error",
            Self::Done => "done",
        }
    }

    /// Unknown names are None, never a guess: a corrupt row must not
    /// invent history.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "started" => Some(Self::Started),
            "stage" => Some(Self::Stage),
            "provider" => Some(Self::Provider),
            "error" => Some(Self::Error),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

/// Unix epoch millis for event stamps.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_roundtrip_and_reject_unknown() {
        for kind in [
            EventKind::Started,
            EventKind::Stage,
            EventKind::Provider,
            EventKind::Error,
            EventKind::Done,
        ] {
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EventKind::parse("transcribing"), None);
        assert_eq!(EventKind::parse(""), None);
    }

    #[test]
    fn clock_moves_forward() {
        assert!(now_ms() > 0);
    }
}
