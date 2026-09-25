---
name: workflow
description: "Workflow disciplines of the Conclave: how the codebase works, why it is the way it is, shockwave (what a change breaks around it), teach (explain to a newcomer or a fresh session). Use ONLY when about to change code you do not fully understand, when asked how or why the code is structured a certain way, when planning a change with wide blast radius, or when orienting a fresh session or teammate. Not for routine edits in a familiar file."
---

# Workflow

The habits that keep changes safe and cheap to understand. Invoke before the
edit, not after the bug report.

## how

Read the real code before touching it. Trace the path a value takes end to
end: where it is created, where it is validated, where it is consumed, where
it can fail. Answer in terms of the actual files and functions, not vibes.
If a subsystem is a black box, say so and open it.

## why

Every shape has a reason, and the reason is usually a past failure. Look for
the decision in comments, tests, history, and docs. Before proposing a change,
say what the current shape was reacting to, then show your change does not
reopen it.

## shockwave

A change is evaluated by what it breaks, not what it builds. Before editing,
enumerate the blast radius: callers, tests, config, stored data, other
services, docs that reference the thing. Bisect which of those you can verify
now against the real artifact and which need a call-out.

## teach

Explain to a fresh listener and catch the gaps in your own understanding while
you do it. Newcomers and fresh sessions ask the questions the author stopped
asking. Walk the actual code, not the diagram in the README.