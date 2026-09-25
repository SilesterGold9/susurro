---
description: "Adversarial review of a diff, branch, or change. Spawns two reviewer subagents (code and judgment) who argue against the change independently, then the cardinal weighs the verdict. Use after a diff is drafted or when a change needs independent scrutiny."
agent: cardinal
---

# /tribunal

An adversarial multi-angle review. The cardinal presides; two reviewer
subagents argue against the change from different angles. The cardinal
weighs both positions and files a verdict.

## Arguments

- `$ARGUMENTS` — the diff, branch, commit range, or PR to review. If
  empty, review the current working diff.

## Procedure

1. Read the ledger before ruling: precedent in `.opencode/ledger/verdicts/`,
   open business in `.opencode/ledger/the-file.md`.
2. Spawn `reviewer-code` on the diff. They argue correctness, test coverage,
   convention adherence, and code quality.
3. Spawn `reviewer-judgment` on the diff. They argue security, product
   semantics, design coherence, and blast radius.
4. Collect both reports. Weigh the arguments against precedent and the
   canons.
5. File a verdict in `.opencode/ledger/verdicts/` with evidence from both
   reviewers and your own observation. Name the canon behind the ruling.
6. If the verdict is changes-requested, summarize what must change before
   the cardinal will sign off.

## Output

A verdict record with:
- The ruling (approve, changes-requested, or reject)
- Evidence from both reviewers
- Canon citation
- A plain summary for the author
