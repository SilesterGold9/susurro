---
name: canons
description: "Seats the operating canons of the Conclave: prove it works, fix root causes, make operations idempotent, subtract before you add, migrate callers then delete the legacy, encode lessons in structure, put validation at boundaries, model the domain, keep the type system honest, never block on the human. Use ONLY when engineering in a Conclave-adapted project — coding, debugging, refactoring, or judging a diff. Not for planning-only chats or edits to the Conclave stack itself."
---

# Canons

The law of the chamber. Each canon names the failure it exists to prevent and
the check that catches it. Name the canon behind a decision out loud; a choice
without a canon is a guess.

## prove-it-works

Claim done only against the real artifact: run the feature, read the value,
inspect the diff. "It compiles" and "looks right" are not proof. If no way to
prove a change exists, that gap is part of the work.

Tell: the turn ends with "should work" or "I think this fixes it". The fix is
a demonstration or the verdict carries an evidence entry.

## fix-root-causes

Reproduce first, then ask why until the actual cause, then fix the pattern,
not the instance. One symptom fixed while the cause stands is a reloaded
landmine.

Tell: the same bug is patched in one place while the same shape of bug lives
on in three others.

## idempotent

Every operation converges: retries, crashes, and restarts land in the same end
state. The ceremony, the apply step, the migration, all of them run twice as
cleanly as once.

Tell: a re-run of the same command produces a different result or a full
failure.

## subtract-before-you-add

Remove dead weight first, then build on the smaller base. New code added to a
cluttered system is two problems, not one.

Tell: the diff adds a file next to three files nobody knows why they exist.

## migrate-callers

Every change to a public surface migrates the callers in the same pass, then
deletes the legacy API. Compatibility layers are debt with a future interest
rate.

Tell: an old function still lives on "because deleting it is scary".

## encode-lessons-in-structure

A recurring correction belongs in types, checks, lint, and tests, not in a
prose reminder that the model may or may not reread. When the same review note
fires twice, promote it to structure.

Tell: a guardrail exists only as a sentence in a markdown file.

## boundary-discipline

Put validation and type narrowing at the edges: input surfaces, API seams,
deserialization, config reads. Trust the core; distrust the perimeter.

Tell: a value is validated deep inside a function that many callers already fed
bad data to.

## model-the-domain

Structures and registries before scattered conditionals. If the domain has a
kind, a state, or a map, it deserves a type and a single source of truth, not
a chain of string comparisons.

Tell: the same string is compared in four places and typos point differently
each time.

## type-system-honesty

No `any` lies, no escaped illegal states. If a value can fail, the type admits
it; if a state cannot exist, the type forbids it. The compiler is a reviewer
that never sleeps.

Tell: a cast that papers over a possibly-undefined value, or a hole in the
type that a null walks through at runtime.

## never-block-on-the-human

Proceed on reversible work; pause only for irreversible writes. Ask when the
answer changes what gets written, not when it changes how diligent you are. A
question with a sane default is a decision taken, not a stall.

Tell: a session waits on a question whose answer would not alter the diff.