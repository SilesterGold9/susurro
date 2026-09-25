---
description: "Read the repo and generate or update the per-project constitution: which canons apply, what prove-it-works means, CI commands, gotchas. Use when adapting Conclave to a new project or when the project's shape changes."
agent: cardinal
---

# /constitution

The project answers the questions the chamber needs answered. Not vibes,
not assumptions. Read the real environment and write the profile.

## Arguments

- `$ARGUMENTS` — optional. A section to regenerate (e.g., "canons",
  "verify", "ci"). If empty, regenerate the full constitution.

## Procedure

1. Read the project root: `package.json`, `requirements.txt`, `Cargo.toml`,
   `go.mod`, `Makefile`, `AGENTS.md`, `.github/workflows/`, `.gitlab-ci.yml`,
   `docs/`, `README.md`.
2. Detect: language, package manager, test command, build command, start
   command, port URLs, CI workflows.
3. Read the canons skill and determine which apply to this project.
4. Write `.opencode/constitution.md` with the profile.
5. If a verification skill exists at `.opencode/skills/verification/SKILL.md`,
   update it from the constitution's verify section.
6. File the evidence in the ledger.

## Output

The constitution file, with the detectable facts filled in and the
canons rated. Gotchas and boundaries are starting points for the human
to fill in.
