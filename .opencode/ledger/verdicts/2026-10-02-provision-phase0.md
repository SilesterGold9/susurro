---
id: 2026-10-02-provision-phase0
date: 2026-10-02
status: binding
canon: prove-it-works
question: Can model provisioning stop depending on curl/PATH/manual downloads without changing frontend or CLI behavior?
verdict: Yes. New susurro-provision crate (signed manifest, resume/hash/swap store) backs both the Tauri download button and a new model-fetch command; event shapes and CLI behavior unchanged.
evidence:
  - provision/src/manifest.rs (Ed25519 key id pinning, canonical JSON, select/is_newer, embedded default manifest, fetch_manifest) + store.rs (Range resume, 416/200 fallback, hash-while-write, .prev swap, force/skip) + examples/keygen.rs
  - ASSET_PUBLIC_KEY_HEX embedded (2b42876b6abc64a4ca2af86fbf841a6c4368a70768602db0e449637613bae62c); secret handed to maintainer, never in repo
  - cargo fmt --all -- --check exits 0; cargo clippy --workspace --all-targets -- -D warnings exits 0; src-tauri clippy exits 0 (28 new locked deps)
  - cargo test --workspace exits 0: 9 new provision tests (sign/verify chain, download, resume, range-ignore restart, mismatch spares live copy, swap retires prev, skip) plus all existing suites and contracts
  - cargo run -p susurro-cli -- model-fetch (skip path: trust recorded, no download)
  - cargo run -p susurro-cli -- model-fetch --tiny --force (real path: 77704715 bytes from HF with redirects, whole-point progress, trust recorded 921e4cf8...)
  - cargo run -p susurro-cli -- doctor (model found, checksums verified); listen --mock --stdout prints the utterance end to end
  - app-tauri download_model rewritten on ensure_asset with identical susurro://onboarding event shape; curl_progress_pct deleted; README gains the model-fetch row; signing-rotation.md gains the asset key section
filed_by: cardinal
---

Interruption lesson, encoded: accepted test sockets inherit the
listener's nonblocking mode, so the test HTTP server forces blocking
streams (provision/src/store.rs tests). Production code was never at
fault. Next: ADR-004 Phase 1 (link whisper-rs, retire PATH lookup).
