# Ledger schema

The record formats in this ledger. The proof gate and the later automation
read by these shapes, so keep the frontmatter keys exact.

## Verdict record — `verdicts/YYYY-MM-DD-<slug>.md`

```markdown
---
id: 2026-08-19-<slug>
date: 2026-08-19
status: binding          # binding | overruled
canon: <canon name>      # e.g. prove-it-works, fix-root-causes
question: <one sentence conflict or question>
verdict: <one sentence ruling>
evidence:                # one bare line per artifact, quoted output inline
  - <command or file path>
  - <output that counts as passing>
filed_by: <agent name>   # cardinal | tribunal | ...
---

Rationale, in prose. Cite precedent by id when continuing or departing from it.
```

Rules:

- `status` is `binding` by default. To overrule an earlier verdict, file a new
  record with `status: overruled` and put `overruled: <old id>` in the
  frontmatter.
- A `binding` verdict with an empty `evidence` block is void on arrival.
  The proof gate flags done claims whose verdict lacks evidence.
- `question` and `verdict` stay one sentence; the rationale carries the
  argument, not the frontmatter.

## Case note — a dated entry in `case-notes.md`

```markdown
## 2026-08-19 — <0-6 word subject>

Claim: <what we believed>
Evidence: <what was actually observed, the run, the number, the diff>
Consequence: <what changed as a result>
```

Append; never rewrite history. A case note that surfaces a recurring
correction becomes a canon amendment (see canon-state).

## The file — `the-file.md`

```markdown
# Case file

## Open questions
- (one line each, with why it matters)

## Pending rulings
- [ ] (one line each, with the blocking unknown)

## Next actions
- (one line each, with the evidence expected)

## Closed this session
- (the rulings made, verdict ids in verdicts/)
```

## Canon state — `canon-state.md`

```markdown
# Canons in force

| Canon | Status | Amended | Repealed by |
|---|---|---|---|
| prove-it-works | adopted | — | — |

## Amendments
- 2026-08-19: <the amendment, the trigger case note, the evidence>

## Repeals
- <date>: <canon>, <reason, the case note it cites>
```

V2 formalizes the lifecycle; this file records the votes in the meantime.