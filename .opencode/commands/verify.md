---
description: "Run the project's verification recipe. Reads the verification skill and executes the run and prove steps against the real artifact. Use after a change to certify it works."
agent: cardinal
---

# /verify

Prove it works. Not "should work", not "looks right". Run the thing
and show the output.

## Arguments

- `$ARGUMENTS` — optional. A specific step or command from the verification
  skill to run. If empty, run the full recipe.

## Procedure

1. Read the verification skill for this project. If none exists, offer
   to create one via `create-verification-skill`.
2. Run the commands in the Run section against the real artifact.
3. Assert each step against the Prove section: does the output match
   the passing string? Does the build exit 0?
4. If a step fails, stop and report the failure. Do not claim done.
5. File the evidence in the ledger: the command, the output, the
   assertion result.
6. If all steps pass, report done with the evidence attached.

## Output

Pass or fail per step, with the real output. If all pass, a done
claim with evidence in the ledger. If any fail, a diagnostic with
the failing step and its output.
