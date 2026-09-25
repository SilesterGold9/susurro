---
description: "Reopen the case file to resume work. Reads .opencode/ledger/the-file.md and presents the open questions, pending rulings, and next actions from the last session. Use at the start of any session in a Conclave-adapted project."
agent: cardinal
---

# /the-file

Resume is not a blank slate. The case file holds the rulings, the open
questions, and the next actions from the last session. Read it before
doing anything.

## Arguments

- `$ARGUMENTS` — optional. A specific section to focus on (e.g.,
  "open questions", "next actions"). If empty, present the full file.

## Procedure

1. Read `.opencode/ledger/the-file.md`.
2. Present the sections: open questions, pending rulings, next actions,
   closed this session.
3. If a specific section was requested, expand on it with current context.
4. Identify what is still blocking and what can proceed.
5. Update the file with any new observations before moving to other work.

## Output

The contents of the case file, annotated with current status. If the
file is empty or missing, say so and offer to initialize it from the
template.
