# 003: hexagonal architecture with ports and adapters

Status: accepted
Date: 2026-09-30
Relates to: v0.7.0 polish and dx, issue 37, contract test suite

## Context

Susurro targets Linux with Hyprland first and Windows next, with local and cloud providers behind the same pipeline. The project needs hardware free development and CI, a contract suite every adapter must pass, mock adapters, session replay for debugging, and a core that stays testable as platforms grow. Platform assumptions must fail fast in CI rather than leak silently into shared logic.

## Decision

The project uses a hexagonal layout with a platform free core. `core/src/lib.rs` declares the rule directly. Core never imports platform crates. It defines behavior only.

`core/src/ports.rs` defines the port traits. Audio capture, voice activity detection, speech to text, text post processing, text injection, global hotkey, overlay rendering, settings store, history store, and network status each form one trait. Audio uses 16 kHz mono S16 only. Transcripts mark partial hypotheses, and callers treat partials as display only. Injection takes a session ticket and keeps exactly once behavior.

`core/src/state.rs` defines the session state machine. Sessions move through idle, listening, transcribing, cleanup, injecting, and back to idle. Skipped states fail. Any active state can abort to idle for fail open recovery.

Adapters implement ports per platform and provider. Examples include audio capture and cues in `adapters-audio/`, local STT in `adapters-stt-local/`, cloud STT in `adapters-stt-cloud/`, Linux hotkey and paste in `adapters-linux/`, Windows hotkey and SendInput in `adapters-windows/`, and history and config in `storage/`. `app-tauri/` and `cli/` compose ports through the pipeline without adding domain rules. `TicketRegistry` gates side effects by session, privacy policy forces local only routing in blocklisted apps, and the event log supports replay.

## Consequences

Core stays portable and unit testable. Mocks replace hardware in dev and CI. Contract tests can check each adapter against documented port behavior. New providers and platforms add code at the edge without rewriting the pipeline. The state machine makes illegal transitions explicit and testable.

The choice adds trait and crate boilerplate. Shared shapes like audio format and ticket handling need discipline, because every adapter depends on them. The team must maintain the contract suite, or mocks and real adapters can drift apart.

## Alternatives

The project rejected a monolith with direct platform calls in shared code, because Linux and Windows paths would tangle and CI would miss leaks until the Windows port. The project rejected feature flagged platform code inside core, because flags hide coupling and complicate tests. The project rejected provider specific core logic, such as one class per cloud vendor, and chose one OpenAI compatible adapter driven by config instead.

## Links

* `core/src/ports.rs` defines all port traits, audio format, transcript shape, and ticketed injection
* `core/src/state.rs` defines states, legal transitions, and abort to idle behavior
* `core/src/lib.rs` states the no platform import rule
* `adapters-stt-local/` shows a port implementation with mock, local binary, and OpenVINO policy
* `app-tauri/` shows composition of capture, STT, cleanup, and injection through the pipeline
* `susurro-project-plan.md` architecture summary, workspace layout, and v0.7.0 section
