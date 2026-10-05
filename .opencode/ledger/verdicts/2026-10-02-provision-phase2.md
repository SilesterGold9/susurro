---
id: 2026-10-02-provision-phase2
date: 2026-10-02
status: binding
canon: prove-it-works
question: Can install-then-dictate work offline with base streaming in the background?
verdict: Yes. tiny.en ships as a build-fetched Tauri resource with pinned hash; resolution prefers tier then quality then bundled; onboarding prefetches base plus auto-bench on first paint with single-flight download.
evidence:
  - provision manifest pins real hashes (base a03779c8/147964211B, tiny 921e4cf8/77704715B) + verify_file helper with unit test
  - src-tauri/build.rs fetches tiny.en on first build per machine, fails closed on mismatch, SUSURRO_SKIP_MODEL_FETCH=1 escape; tauri.conf resources entry validated by tauri_build
  - build.rs fetch proof: resources/models/tiny.en.bin 77704715 bytes, certutil hash equals pinned; git check-ignore confirms it stays out of git
  - main.rs: tier-aware resolve_whisper_with (explicit, env, tier file, small/base/tiny, bundled, missing fallback) + bundled_tiny resolved once in setup + start_model_prefetch (bench then fetch, single-flight) + download_model joins the flight
  - src-tauri tests: resolution_prefers_tier_then_quality_then_bundled passes; src-tauri clippy -D warnings exits 0
  - cargo fmt, workspace clippy, workspace tests all green; npm run build green with prefetch invoke and new copy
filed_by: cardinal
---

Honest gaps: the prefetch join branch needs app runtime (unproven
headless; the shared ensure path underneath is proven). True fresh-VM
acceptance (installer, kill Wi-Fi, dictate) belongs to release CI.
Two process notes: build scripts validate resources before main
build, so the fetch runs before tauri_build; and a "successful" edit
report means nothing without re-reading the file (the manifest pin
silently failed to persist once, caught only by its own test).
Next: Phase 3+ (capability matrix UI, ONNX cleanup default, Parakeet
opt-in) or open issues #56-65.
