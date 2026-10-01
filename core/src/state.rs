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

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_state() -> impl Strategy<Value = State> {
        prop_oneof![
            Just(State::Idle),
            Just(State::Listening),
            Just(State::Transcribing),
            Just(State::Cleanup),
            Just(State::Injecting),
        ]
    }

    proptest! {
        /// The bool and the Result never disagree, on any pair.
        #[test]
        fn gate_matches_predicate(from in arb_state(), to in arb_state()) {
            prop_assert_eq!(from.transition_to(to).is_ok(), from.can_transition_to(to));
        }

        /// Random walks from Idle never panic and only take legal steps.
        #[test]
        fn random_walks_stay_legal(steps in prop::collection::vec(arb_state(), 0..50)) {
            let mut s = State::Idle;
            for next in steps {
                if s.can_transition_to(next) {
                    s = s.transition_to(next).expect("legal step failed");
                    prop_assert!(matches!(
                        s,
                        State::Idle
                            | State::Listening
                            | State::Transcribing
                            | State::Cleanup
                            | State::Injecting
                    ));
                } else {
                    prop_assert!(s.transition_to(next).is_err());
                }
            }
        }

        /// Every non-idle state aborts to Idle, no exceptions.
        #[test]
        fn abort_always_available(s in arb_state()) {
            prop_assert_eq!(
                s.can_transition_to(State::Idle),
                !matches!(s, State::Idle)
            );
        }
    }
}
