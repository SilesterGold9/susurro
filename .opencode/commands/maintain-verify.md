---
description: "Recheck the verification skill against the real repo. Reruns every command in the skill, corrects what changed, and prunes stale gotchas. Use when the verification skill may have drifted from reality."
agent: cardinal
---

# /maintain-verify

A verification skill rots silently until someone runs it and it lies.
Maintenance is a diff, not a rewrite.

## Arguments

- `$ARGUMENTS` — optional. A specific section to recheck (e.g., "Run",
   "Prove", "Gotchas"). If empty, recheck the full skill.

## Procedure

1. Read the current verification skill.
2. Re-run every command in the Run section. Correct what changed.
3. Reassert Prove: the passing output strings and failure strings must
   match reality, not memory.
4. Prune Gotchas that no longer bite. Add the ones that do.
5. Edit the skill file with the corrections. Keep the description gate
   intact.
6. Verify the edited skill by running its commands end to end once.
7. File the proof in the ledger.

## Output

The diff of changes to the verification skill, plus the end-to-end
run proving the edited skill is truthful.
