# Case file

The resume file. Read before acting, append before leaving.

## Open questions
- Real-device loop still unverified on this machine (needs mic, whisper-cli, base.en model, wl-clipboard, ydotool). Matters because mocks prove logic, not capture-to-paste.
- Cloud provider chain, VAD, SQLite history all land in later milestones. Matters because constitution boundaries assume stubs for now.
- `conclave recommend` dies silently (exit 2) on this repo: its secret grep hardcodes `./src ./lib ./app`, grep exits 2 on missing dirs, and `set -euo pipefail` kills the script. Matters because bare `conclave` shows no next actions. Stack-owned bug, left unpatched; use `self-test` plus `check` until upstream fixes it.

## Pending rulings
- [ ] None. Init ruling closes with this session.

## Next actions
- Run the constitution recipe end to end: cargo fmt check, clippy, cargo test --workspace
- Fill in gotchas after first real `susurro doctor` plus `listen --seconds 6` run
- File evidence verdicts per change; keep precedent in verdicts/

## Closed this session
- 2026-09-25 conclave init: constitution written for Susurro, verification skill generated, verdicts/ opened (verdict 2026-09-25-conclave-init)