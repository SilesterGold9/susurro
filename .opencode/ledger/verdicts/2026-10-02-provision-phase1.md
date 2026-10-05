---
id: 2026-10-02-provision-phase1
date: 2026-10-02
status: binding
canon: prove-it-works
question: Can the engine link in-process behind the port with the shell-out as an opt-in escape hatch?
verdict: Yes. WhisperNative (whisper-rs 0.16, whisper.cpp 1.8.3) is the default in CLI and Tauri; --engine cli preserves the shell-out including OpenVINO; WindowedPartial is generic over both.
evidence:
  - adapters-stt-local/src/native.rs (context load-once behind mutex with reload, beam-5/patience--1, en, no-speech 0.6, prompt, silent prints, segment join) + lib.rs (LocalEngine parse, WindowDecoder trait, generic WindowedPartial)
  - cargo build -p susurro-adapters-stt-local (whisper.cpp via CMake+MSVC+libclang, first try, 2m46s)
  - cargo fmt --all -- --check exits 0; cargo clippy --workspace --all-targets -- -D warnings exits 0; src-tauri clippy exits 0
  - cargo test --workspace exits 0: 5 native unit tests, 2 new contract tests, tightened is_blank_transcript (tag-plus-punctuation now blank, well (known) fact still speech)
  - SUSURRO_ACCURACY_MODEL/WAV=jfk proof: tiny.en decodes "And so my fellow Americans, ask not..." in 2.8s CPU
  - cargo run -p susurro-cli -- listen --seconds 3 --stdout: mic capture, native decode, silence gate refuses with "heard only silence" (exit 1, correct)
  - cargo run -p susurro-cli -- doctor (native engine: linked (whisper.cpp 1.8.3); whisper-cli binary: absent — only needed for --engine cli); listen --mock --stdout green; npm run build green
  - .github/workflows/ci.yml + release.yml gain cmake/clang/libclang-dev (linux) and winget LLVM (windows) on all four build jobs
  - doctor, requirements_status, help page, README, release notes, constitution updated: no binary to install anywhere
filed_by: cardinal
---

Two lessons, encoded. One: accepted sockets inherit nonblocking mode
(Phase 0 verdict) has a sibling — test doubles must be boring and
blocking. Two: the first hardware run caught a real gate hole
(`>> [BLANK_AUDIO]` injected through the starts-with-bracket check);
the gate now strips bracketed tags and demands an alphanumeric
remainder. Environment notes: LIBCLANG_PATH persisted to user env;
C: sat at 81MB free mid-phase (npm/temp/crate caches cleaned, 6.1GB
reclaimed); release builds need gigabytes of headroom.
Next: ADR-004 Phase 2 (bundle tiny, background base, onboarding rewrite).
