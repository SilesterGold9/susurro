//! Session-keyed idempotency tickets.
//!
//! Every side effect (injection, provider call, history write, file write)
//! is gated by a ticket so retries, replays, and double-triggered hotkeys
//! can never duplicate an effect. Full enforcement lands in v0.2.0;
//! the type exists from v0.0.1 so adapters take it from day one.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u128);

impl SessionId {
    pub fn new(v: u128) -> Self {
        Self(v)
    }

    /// Random session id without pulling in a uuid dependency yet.
    /// Uses system time + process id — good enough for v0.0.1.
    pub fn generate() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        Self(nanos ^ (pid << 64))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ticket {
    pub session: SessionId,
    /// Names the side effect, e.g. "inject", "history-write".
    pub operation: &'static str,
}

impl Ticket {
    pub fn new(session: SessionId, operation: &'static str) -> Self {
        Self { session, operation }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.session, self.operation)
    }
}

impl fmt::Display for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key())
    }
}

/// In-memory exactly-once gate. Persistent version (SQLite) in v0.2.0.
#[derive(Debug, Default, Clone)]
pub struct TicketRegistry {
    inner: Arc<Mutex<HashSet<String>>>,
}

impl TicketRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns Ok(true) on first claim, Ok(false) if already claimed.
    pub fn claim(&self, ticket: &Ticket) -> Result<bool, crate::CoreError> {
        let mut set = self
            .inner
            .lock()
            .map_err(|e| crate::CoreError::Storage(format!("ticket lock poisoned: {e}")))?;
        Ok(set.insert(ticket.key()))
    }

    /// Claim or return DuplicateEffect error.
    pub fn claim_once(&self, ticket: &Ticket) -> Result<(), crate::CoreError> {
        if self.claim(ticket)? {
            Ok(())
        } else {
            Err(crate::CoreError::DuplicateEffect(ticket.key()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_claim_is_blocked() {
        let reg = TicketRegistry::new();
        let t = Ticket::new(SessionId::new(1), "inject");
        assert!(reg.claim(&t).unwrap());
        assert!(!reg.claim(&t).unwrap());
        assert!(reg.claim_once(&t).is_err());
    }

    #[test]
    fn different_operations_do_not_collide() {
        let reg = TicketRegistry::new();
        let s = SessionId::new(42);
        assert!(reg.claim(&Ticket::new(s, "inject")).unwrap());
        assert!(reg.claim(&Ticket::new(s, "history-write")).unwrap());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    const OPS: &[&str] = &["inject", "remove", "history-write", "restore"];

    proptest! {
        /// Random claim streams converge with a HashSet model: first
        /// claim wins, repeats lose, distinct pairs never collide.
        #[test]
        fn claims_match_set_model(
            ops in prop::collection::vec((any::<u128>(), 0..4usize), 0..100)
        ) {
            let reg = TicketRegistry::new();
            let mut model = HashSet::new();
            for (session, op) in ops {
                let ticket = Ticket::new(SessionId::new(session), OPS[op]);
                prop_assert_eq!(reg.claim(&ticket).unwrap(), model.insert(ticket.key()));
            }
        }
    }
}
