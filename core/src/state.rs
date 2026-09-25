//! Session-scoped state machine:
//! Idle -> Listening -> Transcribing -> Cleanup -> Injecting -> Idle

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Idle,
    Listening,
    Transcribing,
    Cleanup,
    Injecting,
}

impl State {
    pub fn can_transition_to(self, next: State) -> bool {
        use State::*;
        matches!(
            (self, next),
            (Idle, Listening)
                | (Listening, Transcribing)
                | (Transcribing, Cleanup)
                |             (Cleanup, Injecting)
                | (Injecting, Idle)
                // Fail-open: any active state may return to Idle.
                | (Listening, Idle)
                | (Transcribing, Idle)
                | (Cleanup, Idle)
        )
    }

    pub fn transition_to(self, next: State) -> Result<State, crate::CoreError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(crate::CoreError::InvalidTransition {
                from: self,
                to: next,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_is_valid() {
        let mut s = State::Idle;
        for next in [
            State::Listening,
            State::Transcribing,
            State::Cleanup,
            State::Injecting,
            State::Idle,
        ] {
            s = s.transition_to(next).expect("valid transition");
        }
        assert_eq!(s, State::Idle);
    }

    #[test]
    fn skips_are_rejected() {
        assert!(State::Idle.transition_to(State::Transcribing).is_err());
        assert!(State::Listening.transition_to(State::Cleanup).is_err());
        assert!(State::Idle.transition_to(State::Injecting).is_err());
    }

    #[test]
    fn any_active_state_can_abort_to_idle() {
        for s in [State::Listening, State::Transcribing, State::Cleanup] {
            assert_eq!(s.transition_to(State::Idle).unwrap(), State::Idle);
        }
    }
}
