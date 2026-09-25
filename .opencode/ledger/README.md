# The ledger

The Conclave's record of decisions, evidence, and open business. Per project,
portable in git. Never global; precedent does not bleed between repositories.

## Layout

- **the-file** `the-file.md` — resume. Open questions, pending rulings, next
  actions. Reopen it when a session starts, append to it before one ends.
- **case-notes** `case-notes.md` — lessons. One dated entry per lesson,
  assertion plus the evidence that grounded it. A lesson not filed here is
  a lesson paid for twice.
- **verdicts/** — precedent. One file per ruling. New work reconciles with
  these before contradicting them.
- **canon-state** — which canons the project has adopted, amended, or repealed
  (v2 lifecycle).

## How verdicts bind

A ruling filed here is law for this project until it is overruled. To
overrule, file a new verdict with `status: overruled` and cross-reference the
old id. Precedent scales like law: caps, archival, and repeal. Old verdicts
are overruled, not kept forever.

## Evidence

Every verdict carries an `evidence` block: what was run and what it returned.
A done claim with no evidence link is a finding against the chamber. See
`schema.md` for the exact record format the proof gate reads.