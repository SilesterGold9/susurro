//! Pipeline orchestration: capture -> STT -> cleanup -> inject.
//!
//! v0.0.1 scope: cleanup is passthrough, injection is clipboard-paste,
//! history write is a no-op (SQLite lands in v0.2.0).

use crate::ports::{AudioCapturePort, SpeechToTextPort, TextInjectionPort, TextPostProcessorPort};
use crate::{SessionId, Snippet, State, Ticket, TicketRegistry};

#[derive(Debug)]
pub struct UtteranceResult {
    pub session: SessionId,
    pub raw_text: String,
    pub cleaned_text: String,
    /// Trigger that expanded, if any. History keeps raw plus cleaned,
    /// so the trigger stays noted without a schema change.
    pub snippet_trigger: Option<String>,
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
        snippets: &[Snippet],
    ) -> Result<UtteranceResult, crate::CoreError> {
        run_once(capture, stt, cleanup, inject, tickets, session, snippets)
    }

    // Eight args is the port assembly shape (six ports plus session
    // plus snippets); splitting it would scatter the call, not shrink it.
    #[allow(clippy::too_many_arguments)]
    pub fn run_staged(
        capture: &mut dyn AudioCapturePort,
        stt: &dyn SpeechToTextPort,
        cleanup: &dyn TextPostProcessorPort,
        inject: &dyn TextInjectionPort,
        tickets: &TicketRegistry,
        session: SessionId,
        snippets: &[Snippet],
        on_stage: &dyn Fn(crate::Stage),
    ) -> Result<UtteranceResult, crate::CoreError> {
        run_staged(
            capture, stt, cleanup, inject, tickets, session, snippets, on_stage,
        )
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
    snippets: &[Snippet],
) -> Result<UtteranceResult, crate::CoreError> {
    run_staged(
        capture,
        stt,
        cleanup,
        inject,
        tickets,
        session,
        snippets,
        &|_| {},
    )
}

/// Staged variant: `on_stage` fires at each post-capture boundary so
/// progress UI can track transcribing, polishing, and injecting.
/// Same behavior and guarantees as `run_once`.
// Eight args is the port assembly shape; see the method above.
#[allow(clippy::too_many_arguments)]
pub fn run_staged(
    capture: &mut dyn AudioCapturePort,
    stt: &dyn SpeechToTextPort,
    cleanup: &dyn TextPostProcessorPort,
    inject: &dyn TextInjectionPort,
    tickets: &TicketRegistry,
    session: SessionId,
    snippets: &[Snippet],
    on_stage: &dyn Fn(crate::Stage),
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
    on_stage(crate::Stage::Transcribing);
    let transcript = stt.transcribe(&pcm)?;

    state = state.transition_to(State::Cleanup)?;
    on_stage(crate::Stage::Polishing);
    // Fail-open: cleanup errors AND panics fall back to the raw transcript.
    // Injection must never be blocked by the cleanup stage.
    let cleaned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cleanup.cleanup(&transcript.text)
    }))
    .unwrap_or_else(|_| Err(crate::CoreError::Cleanup("cleanup panicked".into())))
    .unwrap_or_else(|_| transcript.text.clone());

    state = state.transition_to(State::Injecting)?;
    on_stage(crate::Stage::Injecting);
    // Snippets (issue 55): whole-utterance exact match after cleanup,
    // before injection. Partial input never expands.
    let (final_text, snippet_trigger) = match crate::find_expansion(&cleaned, snippets) {
        Some((trigger, expansion)) => (expansion.to_string(), Some(trigger.to_string())),
        None => (cleaned.clone(), None),
    };
    let ticket = Ticket::new(session, "inject");
    tickets.claim_once(&ticket)?;
    inject.inject(&final_text, &ticket)?;

    state = state.transition_to(State::Idle)?;
    debug_assert_eq!(state, State::Idle);

    Ok(UtteranceResult {
        session,
        raw_text: transcript.text,
        cleaned_text: final_text,
        snippet_trigger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::AudioChunk;

    pub(crate) struct MockCapture {
        pub(crate) chunks: Vec<AudioChunk>,
        pub(crate) i: usize,
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

    pub(crate) struct MockInject {
        pub seen: std::sync::Mutex<Vec<String>>,
        pub removed: std::sync::Mutex<Vec<String>>,
    }
    impl TextInjectionPort for MockInject {
        fn inject(&self, text: &str, _t: &Ticket) -> Result<(), crate::CoreError> {
            self.seen.lock().unwrap().push(text.to_string());
            Ok(())
        }
        fn remove_last(&self, text: &str, _t: &Ticket) -> Result<(), crate::CoreError> {
            self.removed.lock().unwrap().push(text.to_string());
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
            removed: Default::default(),
        };
        let reg = TicketRegistry::new();
        let out = run_once(
            &mut cap,
            &MockStt,
            &PassthroughCleanup,
            &inject,
            &reg,
            SessionId::new(7),
            &[],
        )
        .unwrap();
        assert_eq!(out.cleaned_text, "hello world");
        assert!(out.snippet_trigger.is_none());
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
            removed: Default::default(),
        };
        let err = run_once(
            &mut cap,
            &MockStt,
            &PassthroughCleanup,
            &inject,
            &reg,
            session,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, crate::CoreError::DuplicateEffect(_)));
        assert!(inject.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn cleanup_panic_injects_raw() {
        struct PanicCleanup;
        impl crate::ports::TextPostProcessorPort for PanicCleanup {
            fn cleanup(&self, _raw: &str) -> Result<String, crate::CoreError> {
                panic!("boom");
            }
        }
        let mut cap = MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 160],
                is_final: true,
            }],
            i: 0,
        };
        let inject = MockInject {
            seen: Default::default(),
            removed: Default::default(),
        };
        let out = run_once(
            &mut cap,
            &MockStt,
            &PanicCleanup,
            &inject,
            &TicketRegistry::new(),
            SessionId::new(11),
            &[],
        )
        .unwrap();
        assert_eq!(out.cleaned_text, "hello world");
        assert_eq!(inject.seen.lock().unwrap().as_slice(), ["hello world"]);
    }

    #[test]
    fn staged_reports_boundaries_in_order() {
        use std::cell::RefCell;
        let mut cap = MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 160],
                is_final: true,
            }],
            i: 0,
        };
        let inject = MockInject {
            seen: Default::default(),
            removed: Default::default(),
        };
        let seen = RefCell::new(Vec::new());
        run_staged(
            &mut cap,
            &MockStt,
            &PassthroughCleanup,
            &inject,
            &TicketRegistry::new(),
            SessionId::new(13),
            &[],
            &|s| seen.borrow_mut().push(s),
        )
        .unwrap();
        assert_eq!(
            *seen.borrow(),
            vec![
                crate::Stage::Transcribing,
                crate::Stage::Polishing,
                crate::Stage::Injecting
            ]
        );
    }

    fn snippet_list() -> Vec<crate::Snippet> {
        vec![
            crate::Snippet::new("my email", "me@example.com").unwrap(),
            crate::Snippet::new("standup link", "https://meet.example.com/daily").unwrap(),
        ]
    }

    struct TriggerStt(&'static str);
    impl SpeechToTextPort for TriggerStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<crate::ports::Transcript, crate::CoreError> {
            Ok(crate::ports::Transcript {
                text: self.0.into(),
                is_partial: false,
            })
        }
        fn model_name(&self) -> &str {
            "trigger-mock"
        }
    }

    fn run_with(text: &'static str, snippets: &[crate::Snippet]) -> (UtteranceResult, Vec<String>) {
        let mut cap = MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 160],
                is_final: true,
            }],
            i: 0,
        };
        let inject = MockInject {
            seen: Default::default(),
            removed: Default::default(),
        };
        let out = run_once(
            &mut cap,
            &TriggerStt(text),
            &PassthroughCleanup,
            &inject,
            &TicketRegistry::new(),
            SessionId::generate(),
            snippets,
        )
        .unwrap();
        let seen = inject.seen.lock().unwrap().clone();
        (out, seen)
    }

    #[test]
    fn snippet_trigger_injects_expansion() {
        let (out, seen) = run_with("my email", &snippet_list());
        assert_eq!(seen.as_slice(), ["me@example.com"]);
        assert_eq!(out.raw_text, "my email");
        assert_eq!(out.cleaned_text, "me@example.com");
        assert_eq!(out.snippet_trigger.as_deref(), Some("my email"));
    }

    #[test]
    fn snippet_match_tolerates_case_and_punctuation() {
        let (out, seen) = run_with("My Email.", &snippet_list());
        assert_eq!(seen.as_slice(), ["me@example.com"]);
        assert_eq!(out.snippet_trigger.as_deref(), Some("my email"));
    }

    #[test]
    fn snippet_partial_stays_dictation() {
        let (out, seen) = run_with("send my email please", &snippet_list());
        assert_eq!(seen.as_slice(), ["send my email please"]);
        assert_eq!(out.cleaned_text, "send my email please");
        assert!(out.snippet_trigger.is_none());
    }

    #[test]
    fn empty_snippets_leave_text_untouched() {
        let (out, seen) = run_with("my email", &[]);
        assert_eq!(seen.as_slice(), ["my email"]);
        assert!(out.snippet_trigger.is_none());
    }
}

#[cfg(test)]
mod property_tests {
    use super::tests::{MockCapture, MockInject};
    use super::*;
    use crate::ports::AudioChunk;
    use proptest::prelude::*;

    struct VarStt(String);
    impl SpeechToTextPort for VarStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<crate::ports::Transcript, crate::CoreError> {
            Ok(crate::ports::Transcript {
                text: self.0.clone(),
                is_partial: false,
            })
        }
        fn model_name(&self) -> &str {
            "var-mock"
        }
    }

    fn one_chunk() -> MockCapture {
        MockCapture {
            chunks: vec![AudioChunk {
                samples: vec![0; 160],
                is_final: true,
            }],
            i: 0,
        }
    }

    proptest! {
        /// Fuzz injection end to end: arbitrary transcripts (empty,
        /// unicode, huge) flow capture to injection verbatim through
        /// passthrough, exactly once per session. A replayed session
        /// injects nothing and reports DuplicateEffect.
        #[test]
        fn arbitrary_transcripts_inject_exactly_once(
            text in "[\\s\\S]{0,500}",
            session in any::<u128>(),
        ) {
            let stt = VarStt(text.clone());
            let inject = MockInject {
                seen: Default::default(),
                removed: Default::default(),
            };
            let reg = TicketRegistry::new();
            let id = SessionId::new(session);
            let out = run_once(
                &mut one_chunk(),
                &stt,
                &PassthroughCleanup,
                &inject,
                &reg,
                id,
                &[],
            )
            .expect("fuzzed run failed");
            prop_assert_eq!(&out.raw_text, &text);
            prop_assert_eq!(&out.cleaned_text, &text);
            let seen = inject.seen.lock().unwrap().clone();
            prop_assert_eq!(seen.as_slice(), [text]);

            let err = run_once(
                &mut one_chunk(),
                &stt,
                &PassthroughCleanup,
                &inject,
                &reg,
                id,
                &[],
            )
            .unwrap_err();
            prop_assert!(matches!(err, crate::CoreError::DuplicateEffect(_)));
            prop_assert_eq!(inject.seen.lock().unwrap().len(), 1);
        }
    }
}
