# Case file

The resume file. Read before acting, append before leaving.

## Open questions

- Boundaries: exact ticket-gating points for new side effects (constitution marks human-to-confirm).
- Convergence reacts to network regain on a backoff timer, not an OS
  notification (ADR-004 wording). Revisit only if regain latency matters.
- Parakeet (58) deferred by decision, not by blocker. Its three costs:
  680 MB against a 100 MB day-0 budget, streaming semantics the offline
  API does not hand you, and bench-tier integration with the existing
  three-model resolution. Ruling: manifest opt-in flag first, STT
  backend as its own session.

## Pending rulings

- Parakeet (58): deferred by decision. Ruling filed below: opt_in flag
  on `Asset` first, then an opt-in fetch, then the backend as its own
  session. 680 MB against a 100 MB ceiling, offline API against
  streaming dictation, and tier-resolution integration are the three
  costs, none of which a rushed single pass would survive.

## Next actions

- Open queue, verified against GitHub 2026-10-05 (all OPEN): 56 post-dictation
  rewrites, 57 scratchpad notepad, 60 Silero VAD, 61 auto language
  detect, 62 wake word spike, 65 voidEngine i18n. 58 deferred above.
- 60 and 61 both need whisper-cli flags (`--vad -vm`, `-l auto`) that
  the linked engine does not expose. Ruling needed: re-expose the flags,
  or move the VAD to the energy port and the language to the model.
- Decide whether `skills/verification/SKILL.md` should also be versioned: it is generated from the constitution but currently git-ignored, so fresh clones regenerate it via `conclave init`.

## Closed this session

- 2026-10-05: voice fingerprint (verdict `2026-10-05-voice-fingerprint`):
  deterministic counting for three Insights cards, closing issue 64.
- 2026-10-05: ONNX punctuation default (verdict `2026-10-05-onnx-cleanup-default`):
  7.6 MB bundled model replaces "install Ollama" as the default cleanup;
  sherpa-onnx `shared` feature forced by the Windows CRT mismatch.
- 2026-10-05: boot-time convergence loop (verdict `2026-10-05-convergence-loop`):
  `Converger` in provision with persisted backoff split by who fixes the failure,
  `susurro converge`, doctor convergence section, Tauri boot loop plus System card.
- 2026-10-05: ledger queue reconciled against GitHub (63 closed by the dictionary
  commit); constitution and ledger files committed as `a85f181`.
- 2026-10-02: constitution regenerated from the real environment (verdict `2026-10-02-constitution`); verification skill updated to match.