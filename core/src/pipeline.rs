//! Pipeline orchestration: capture -> STT -> cleanup -> inject.
//!
//! v0.0.1 scope: cleanup is passthrough, injection is clipboard-paste,
//! history write is a no-op (SQLite lands in v0.2.0).

use crate::ports::{AudioCapturePort, SpeechToTextPort, TextInjectionPort, TextPostProcessorPort};
use crate::{SessionId, State, Ticket, TicketRegistry};

#[derive(Debug)]
pub struct UtteranceResult {
    pub session: SessionId,
    pub raw_text: String,
    pub cleaned_text: String,
}

pub struct PassthroughCleanup;

impl TextPostProcessorPort for PassthroughCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, crate::CoreError> {
        Ok(raw.to_string())
    }
}

pub struct Pipeline;

impl Pipeline {
    pub fn run_once(
        capture: &mut dyn AudioCapturePort,
        stt: &dyn SpeechToTextPort,
        cleanup: &dyn TextPostProcessorPort,
        inject: &dyn TextInjectionPort,
        tickets: &TicketRegistry,
        session: SessionId,
    ) -> Result<UtteranceResult, crate::CoreError> {
        run_once(capture, stt, cleanup, inject, tickets, session)
    }
}

/// Run one utterance end to end through the state machine.
///
/// Callers drive `capture` until `is_final`, then this function
/// transcribes, cleans, and injects exactly once per ticket.
pub fn run_once(
    capture: &mut dyn AudioCapturePort,
    stt: &dyn SpeechToTextPort,
    cleanup: &dyn TextPostProcessorPort,
    inject: &dyn TextInjectionPort,
    tickets: &TicketRegistry,
    session: SessionId,
) -> Result<UtteranceResult, crate::CoreError> {
    let mut state = State::Idle;
    state = state.transition_to(State::Listening)?;

    // v0.0.1: push-to-talk accumulation. VAD streaming in v0.1.0/v0.4.0.
    let mut pcm: Vec<i16> = Vec::new();
    loop {
        let chunk = capture.next_chunk().map_err(|e| match e {
            crate::CoreError::Capture(msg) => crate::CoreError::Capture(msg),
            other => crate::CoreError::Capture(other.to_string()),
        })?;
        pcm.extend_from_slice(&chunk.samples);
        if chunk.is_final {
            break;
        }
    }

    state = state.transition_to(State::Transcribing)?;
    let transcript = stt.transcribe(&pcm)?;

    state = state.transition_to(State::Cleanup)?;
    // Fail-open (v0.2.0 full): if cleanup panics/errs, inject raw.
    let cleaned = cleanup
        .cleanup(&transcript.text)
        .unwrap_or_else(|_| transcript.text.clone());

    state = state.transition_to(State::Injecting)?;
    let ticket = Ticket::new(session, "inject");
    tickets.claim_once(&ticket)?;
    inject.inject(&cleaned, &ticket)?;

    state = state.transition_to(State::Idle)?;
    debug_assert_eq!(state, State::Idle);

    Ok(UtteranceResult {
        session,
        raw_text: transcript.text,
        cleaned_text: cleaned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::AudioChunk;

    struct MockCapture {
        chunks: Vec<AudioChunk>,
        i: usize,
    }
    impl AudioCapturePort for MockCapture {
        fn start(&mut self) -> Result<(), crate::CoreError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), crate::CoreError> {
            Ok(())
        }
        fn next_chunk(&mut self) -> Result<AudioChunk, crate::CoreError> {
            let c = self.chunks.get(self.i).cloned().unwrap_or(AudioChunk {
                samples: vec![],
                is_final: true,
            });
            self.i += 1;
            Ok(c)
        }
    }

    struct MockStt;
    impl SpeechToTextPort for MockStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<crate::ports::Transcript, crate::CoreError> {
            Ok(crate::ports::Transcript {
                text: "hello world".into(),
                is_partial: false,
            })
        }
        fn model_name(&self) -> &str {
            "mock"
        }
    }

    struct MockInject {
        pub seen: std::sync::Mutex<Vec<String>>,
    }
    impl TextInjectionPort for MockInject {
        fn inject(&self, text: &str, _t: &Ticket) -> Result<(), crate::CoreError> {
            self.seen.lock().unwrap().push(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn pipeline_injects_cleaned_text() {
        let mut cap = MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 1600],
                is_final: true,
            }],
            i: 0,
        };
        let inject = MockInject {
            seen: Default::default(),
        };
        let reg = TicketRegistry::new();
        let out = run_once(
            &mut cap,
            &MockStt,
            &PassthroughCleanup,
            &inject,
            &reg,
            SessionId::new(7),
        )
        .unwrap();
        assert_eq!(out.cleaned_text, "hello world");
        assert_eq!(inject.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn replay_is_blocked_by_ticket() {
        let reg = TicketRegistry::new();
        let session = SessionId::new(9);
        // First claim wins.
        reg.claim_once(&Ticket::new(session, "inject")).unwrap();
        // Second run_once with same session must fail at inject gate.
        let mut cap = MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 160],
                is_final: true,
            }],
            i: 0,
        };
        let inject = MockInject {
            seen: Default::default(),
        };
        let err = run_once(
            &mut cap,
            &MockStt,
            &PassthroughCleanup,
            &inject,
            &reg,
            session,
        )
        .unwrap_err();
        assert!(matches!(err, crate::CoreError::DuplicateEffect(_)));
        assert!(inject.seen.lock().unwrap().is_empty());
    }
}
