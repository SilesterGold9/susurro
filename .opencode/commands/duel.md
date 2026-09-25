---
description: "Competing implementations of the same change. Two worker subagents build independent prototypes; the cardinal evaluates both against the spec and the canons, and one survives. Use when a design question has two reasonable answers and the best way to settle it is to build both."
agent: cardinal
---

# /duel

Two workers, one problem, one survivor. The cardinal sets the spec, the
workers build, the cardinal judges.

## Arguments

- `$ARGUMENTS` — the problem statement or design question. Be specific
  about what each prototype must demonstrate.

## Procedure

1. Read the ledger for precedent on similar design questions.
2. Write a short spec: what both prototypes must do, what "better" means
   in this context, which canons apply.
3. Spawn two `worker` subagents, each given the same spec but told to
   build independently. They do not see each other's work.
4. When both workers report done with evidence, evaluate each against
   the spec and the canons.
5. File a verdict in `.opencode/ledger/verdicts/` explaining why one
   survived and the other did not. The losing prototype is deleted; no
   compatibility layers, no保留.
6. Apply the winning prototype to the codebase. Prove it works.

## Output

A verdict record with:
- The spec both workers were given
- What each worker produced (file paths, test results)
- The ruling and canon citation
- The surviving change, applied and proven
