---
id: 2026-10-02-constitution
date: 2026-10-02
status: binding
canon: prove-it-works
question: Init left a placeholder constitution; what does this project actually run?
verdict: Constitution regenerated from the detected toolchain and verification skill updated to match.
evidence:
  - cargo metadata --no-deps (10 crates: susurro-core, susurro-adapters-audio, susurro-adapters-stt-local, susurro-adapters-stt-cloud, susurro-adapters-cleanup, susurro-adapters-linux, susurro-adapters-windows, susurro-storage, susurro-cli, susurro-contracts)
  - cargo --version (cargo 1.91.0)
  - .opencode/constitution.md (rewritten 2026-10-02, all 10 canons rated with project notes)
  - .opencode/skills/verification/SKILL.md (updated from the constitution verify section)
filed_by: cardinal
---

The `conclave init --yes` constitution was auto-detect scaffolding:
purpose "(detected: rust project)", constraints "(none specified)",
gotchas "(none detected)", boundaries and domain blank. Every section is
now grounded in a file that was actually read: workspace `Cargo.toml`
(10 members, `app-tauri/src-tauri` excluded), `justfile` (recipes mirror
CI), `CONTRIBUTING.md` (the full prove-it-works recipe, edge validation,
ticket gating, conventional commits), `ci.yml` (lint-test + tauri-check
matrices), `app-tauri/package.json` (Vite 6, React 19, Tauri v2), and
`docs/adr/`. No section was written from assumption; the two remaining
unknowns are marked "human to confirm" in boundaries, not filled with
vibes. Canon-state needs no amendment: all 10 canons hold as adopted,
each with a susurro-specific note.
