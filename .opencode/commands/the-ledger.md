---
description: "Record evidence for a done claim. Writes a verdict record to .opencode/ledger/verdicts/ with the command run, the output, and the test results. Use whenever a task is about to be declared done."
agent: cardinal
---

# /the-ledger

A done claim without evidence is opinion. The ledger makes it fact.

## Arguments

- `$ARGUMENTS` — optional. A short description of what was done. If
  empty, derive from the current session context.

## Procedure

1. Identify the evidence: what command was run, what output counts as
   passing, what test proved the change.
2. Format a verdict record per the ledger schema:
   ```
   ---
   id: YYYY-MM-DD-<slug>
   date: YYYY-MM-DD
   status: binding
   canon: <canon name>
   question: <one sentence>
   verdict: <one sentence>
   evidence:
     - <command or file path>
     - <output that counts as passing>
   filed_by: cardinal
   ---
   ```
3. Write to `.opencode/ledger/verdicts/YYYY-MM-DD-<slug>.md`.
4. Update `.opencode/ledger/the-file.md` with the resume state so the
   next session can pick up.

## Output

The filed verdict record, confirmed in place. If evidence is thin
or missing, say so and do not file a void verdict.
