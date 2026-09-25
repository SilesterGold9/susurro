//! v0.0.1 E2E proof (hardware-free): capture -> STT -> cleanup -> inject.
//! Uses mocks for every port. The real-device path is exercised
//! manually via `susurro listen` / `susurro daemon` (issues #1-#4).

use susurro_adapters_audio::MockCapture;
use susurro_adapters_cleanup::PassthroughCleanup;
use susurro_adapters_linux::MockInjector;
use susurro_adapters_stt_local::MockStt;
use susurro_core::ports::{AudioCapturePort, AudioChunk};
use susurro_core::{Pipeline, SessionId, TicketRegistry};

#[test]
fn mock_e2e_injects_transcript_once() {
    let mut capture = MockCapture::new(vec![AudioChunk {
        samples: vec![0; 16_000],
        is_final: true,
    }]);
    capture.start().unwrap();
    let stt = MockStt::new("hello susurro");
    let inject = MockInjector::new();
    let tickets = TicketRegistry::new();

    let out = Pipeline::run_once(
        &mut capture,
        &stt,
        &PassthroughCleanup,
        &inject,
        &tickets,
        SessionId::new(0xbeef),
    )
    .expect("pipeline runs");

    assert_eq!(out.raw_text, "hello susurro");
    assert_eq!(out.cleaned_text, "hello susurro");
    assert_eq!(inject.seen.lock().unwrap().as_slice(), ["hello susurro"]);
}

#[test]
fn double_hotkey_cannot_double_inject() {
    let tickets = TicketRegistry::new();
    let session = SessionId::new(1234);

    for _ in 0..2 {
        let mut capture = MockCapture::silence(160);
        capture.start().unwrap();
        let stt = MockStt::new("repeat");
        let inject = MockInjector::new();
        let res = Pipeline::run_once(
            &mut capture,
            &stt,
            &PassthroughCleanup,
            &inject,
            &tickets,
            session,
        );
        // First succeeds, second is blocked by the ticket.
        if inject.seen.lock().unwrap().is_empty() {
            assert!(res.is_err(), "replay must fail");
        } else {
            assert!(res.is_ok());
        }
    }
}
