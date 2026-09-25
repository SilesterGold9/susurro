# Conclave rules

Operating principles for the Conclave, loaded into every session via
`instructions`. These hold regardless of model or provider.

## Discipline

- Verify against the real artifact — run the feature, read the value, inspect
  the diff. "It compiles" is not proof.
- Fix root causes, not instances. Reproduce first, then ask why until the
  cause, then fix the pattern.
- Every operation converges: retries, crashes, and restarts land in the same
  end state.
- Subtract before you add. Remove dead weight first, then build on the smaller
  base.
- Migrate callers, then delete the legacy API — no compatibility layers.
- Encode recurring corrections in structure (types, checks, lint), not prose.
- Model the domain: structures and registries before scattered conditionals.
- Put validation and type narrowing at boundaries.
- Keep the type system honest: no `any` lies, no escaped illegal states.
- Proceed on reversible work; pause only for irreversible writes.

## Process

- Open a todolist before non-trivial work.
- Name the canon behind each decision.
- Write through `unslop` — no em dashes, no filler, active voice.
- Evidence goes in the ledger (`/the-ledger`); lessons go in the case file
  (`/case-notes`); resume via the file (`/the-file`).

## Constitution

The per-project constitution at `.opencode/constitution.md` defines:
- Which canons apply to this project.
- What "prove it works" means (the verification recipe).
- CI, gotchas, boundaries, and domain specifics.

Read the constitution before ruling. Run `/constitution` to regenerate it.