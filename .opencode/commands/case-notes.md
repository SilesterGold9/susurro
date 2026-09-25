---
description: "File a lesson into the case notes. Records what was believed, what was actually observed, and what changed as a result. Use whenever a lesson surfaces or a question is settled during work."
agent: cardinal
---

# /case-notes

A lesson not filed is a lesson paid for twice. File it now, in the
ledger, before moving on.

## Arguments

- `$ARGUMENTS` — optional. A short subject for the note. If empty, the
  cardinal derives one from the current context.

## Procedure

1. Identify the lesson: what did we believe, what did we observe, what
   changed?
2. Format it per the ledger schema:
   ```
   ## YYYY-MM-DD — <subject>
   Claim: <what we believed>
   Evidence: <what was actually observed>
   Consequence: <what changed as a result>
   ```
3. Append to `.opencode/ledger/case-notes.md`. Never rewrite history.
4. If this lesson surfaces a recurring correction, note that it is a
   candidate for canon amendment.

## Output

The appended case note, confirmed in place. If the lesson is a repeat
of a previous note, flag it as a canon amendment candidate.
