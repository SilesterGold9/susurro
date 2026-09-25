---
id: 2026-09-25-conclave-init
date: 2026-09-25
status: binding
canon: prove-it-works
question: How does the chamber rule on Susurro without a project profile?
verdict: Seat the constitution, verification skill, and verdicts ledger, all proven against the real toolchain.
evidence:
  - .opencode/constitution.md
  - .opencode/skills/verification/SKILL.md
  - "cargo fmt --all -- --check exits 0 with no diff"
  - "cargo clippy --workspace --all-targets -- -D warnings exits 0"
  - "cargo test --workspace exits 0 (13 audio, 3 cleanup, 3 stt-local, 2 e2e_mock incl. mock_e2e_injects_transcript_once and double_hotkey_cannot_double_inject, 8 core)"
  - "conclave self-test: All ritual self-tests passed, constitution present, proof-gate smoke test passed"
filed_by: cardinal
---

Susurro had the full Conclave stack seated but no per-project law.
`conclave self-test` flagged `constitution missing` and `recommend`
flagged no constitution plus an empty ledger. Every session until now
ruled on vibes.

The constitution profiles the real repo: Rust workspace with nine
members, app-tauri excluded, core OS-agnostic with session-keyed ticket
idempotency, CI as fmt plus clippy denied plus workspace tests on Linux
and Windows with a separate Tauri check. The verification skill encodes
the same recipe so `/verify` runs structure, not memory. Precedent
continues from this file by id.

Canon: prove-it-works for the evidence gate, encode-lessons-in-structure
for writing the recipe into the skill instead of a reminder.
