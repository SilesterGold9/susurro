# Case file

The resume file. Read before acting, append before leaving.

## Open questions

- Boundaries: exact ticket-gating points for new side effects (constitution marks human-to-confirm).
- Convergence reacts to network regain on a backoff timer, not an OS
  notification (ADR-004 wording). Revisit only if regain latency matters.

## Pending rulings

## Next actions

- Remaining plane work: ONNX cleanup default (Phase 4, issue 59),
  Parakeet opt-in tier as manifest entry (Phase 5, issue 58).
- Open queue, verified against GitHub 2026-10-05 (all OPEN): 56 post-dictation
  rewrites, 57 scratchpad notepad, 60 Silero VAD, 61 auto
  language detect, 62 wake word, 64 voice fingerprint stats, 65 voidEngine i18n.
  Issue 58 Parakeet is tracked separately as Phase 5 plane work.
- Decide whether `skills/verification/SKILL.md` should also be versioned: it is generated from the constitution but currently git-ignored, so fresh clones regenerate it via `conclave init`.

## Closed this session

- 2026-10-05: boot-time convergence loop (verdict `2026-10-05-convergence-loop`):
  `Converger` in provision with persisted backoff split by who fixes the failure,
  `susurro converge`, doctor convergence section, Tauri boot loop plus System card.
- 2026-10-05: ledger queue reconciled against GitHub (63 closed by the dictionary
  commit); constitution and ledger files committed as `a85f181`.
- 2026-10-02: constitution regenerated from the real environment (verdict `2026-10-02-constitution`); verification skill updated to match.