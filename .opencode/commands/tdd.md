---
description: "Test-driven development. Write the test first (red), make it pass (green), clean up (refactor). The test is the proof; the refactor is the discipline. Use when building a feature or fixing a bug test-first."
agent: cardinal
---

# /tdd

Red, green, refactor. The test is the proof before the code is the
implementation.

## Arguments

- `$ARGUMENTS` — the feature or bug to address. Be specific about what
  the test should assert.

## Procedure

1. Read the ledger for precedent on similar changes.
2. Write the test first. It must fail (red). Show the failure output.
3. Write the minimum code that makes the test pass (green). Show the
   passing output.
4. Refactor: apply the canons. Subtract before you add. Encode lessons
   in structure. Keep the type system honest.
5. Run the test again after refactor. It must still pass.
6. File the evidence in the ledger: the test, the red output, the green
   output, the refactored code.

## Output

A test that proves the behavior, code that makes it pass, and a
ledger record with the evidence trail from red to green to clean.
