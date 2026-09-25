---
description: "Generate a setup wizard: a bash script that walks you through manual setup step by step, opening browsers, giving exact instructions, capturing values, and writing .env files and GitHub secrets. Use when the project needs env vars, API keys, database URLs, or third-party service setup."
agent: cardinal
---

# /wizard

No more re-explaining the OAuth dance. The wizard scans your project,
finds what's missing, and generates a script that holds your hand.

## Arguments

- `$ARGUMENTS` — optional. A specific area to set up (e.g., "stripe",
  "database", "oauth"). If empty, scan the full project.

## Procedure

### 1. Scan the project

Read these files to find what needs setup:

- `.env.example`, `.env.sample`, `.env.*` — env vars the project expects
- `.github/workflows/*.yml` — `secrets.*` and `vars.*` references
- `.gitlab-ci.yml` — `$VARIABLE` references in CI jobs
- `docker-compose.yml`, `docker-compose.yaml` — services, ports, volumes
- `package.json` — scripts that start dev servers, ports
- `README.md` — setup instructions, external service links
- `wrangler.toml`, `vercel.json`, `railway.json` — deployment config
- Existing `.env` — what's already set (skip those)

### 2. Map each missing value

For each missing value, determine:

- **Name** — the env var or secret name
- **Source** — where the human gets it (which URL, which page, what to click)
- **Destination** — where it goes (.env, GitHub secret, both)
- **Secret** — is it sensitive? (hidden input vs visible)
- **Instructions** — the exact step-by-step path in the UI

### 3. Generate the wizard

Read the wizard template from the conclave install path
(`lib/wizard-template.sh` relative to the conclave root). The template
provides the library: progress bars, URL opening, ask/ask_secret, write_env,
set_secret/set_var, finish summary.

Write the generated script to `.opencode/wizard-setup.sh` with:

- The full library above the STAGES marker (copied from template)
- One `stage` per missing value, in dependency order
- Each stage: open the URL, give exact instructions, capture the value,
  write to .env, set GitHub secret if CI needs it
- `TOTAL_STAGES` and `TOTAL_MINUTES` set to honest estimates
- `chmod +x` the script

### 4. Hand off

Tell the user:

- The script is at `.opencode/wizard-setup.sh`
- How to run it: `bash .opencode/wizard-setup.sh`
- That it's ephemeral by default (delete after run)
- If the setup path is repeatable, suggest committing it

## Output

The generated wizard script, plus a summary of what it configures.
If no missing values are found, report that the project is fully set up.

## Gotchas

- If `gh` is not authenticated, the script warns and records skipped secrets
  with manual instructions.
- The script is idempotent: re-running it reads existing .env values as
  defaults and skips already-set GitHub secrets.
- Secrets are never written to .env if the user says no. The script asks
  before every write.
