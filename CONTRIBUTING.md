# Contributing to Susurro

Small repo, strict habits. This file is the whole process.

## Setup

Linux needs `libasound2-dev` and `pkg-config`. The Tauri app needs
`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
plus `npm ci` in `app-tauri/`. Windows builds on stable Rust with no
extra setup. Install `just` with `cargo install just` for the short
commands (`just dev`, `just lint`, `just doctor`).

## Prove it works

Every change proves itself against the real artifact. Run, from the
repo root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p susurro-cli -- doctor
cargo run -p susurro-cli -- listen --mock --stdout
```

Then the Tauri side, from `app-tauri/` and `app-tauri/src-tauri/`:

```sh
npm run build
cargo clippy --all-targets -- -D warnings
```

"It compiles" is not proof. Run the feature, read the value, inspect
the diff. Hardware proofs use `doctor` plus a real `listen`; mock
proofs use `--mock --stdout`. CI runs the same recipe on Ubuntu and
Windows, so a Windows-only failure is a real finding, never a flake.

## Commits

Conventional commits: `feat(core): ...`, `fix(linux): ...`,
`test(property): ...`, `docs: ...`, `chore(lockfile): ...`. One
concern per commit. Migrate callers in the same pass and delete the
legacy API instead of leaving compatibility layers. Push straight to
`main`; feature branches stay ephemeral and get deleted after merge.

## Architecture

Hexagonal. `core/` defines behavior only and never imports a
platform crate. Platform code lives in `adapters-*`. Every side
effect is gated by a session-keyed ticket, storage degrades instead
of blocking dictation, and cleanup fails open to the raw transcript.
Validation happens at the edges: CLI parsing, adapter constructors,
Tauri commands. If the same review note fires twice, promote it to
structure (a type, a check, a test), not prose.

## Issues

Check open issues before starting; addenda on closed issues are
finished scope, not open scope. Close an issue with an evidence
comment naming what the change does and the proof behind it: the
commands run, the output that counts, the tests that passed.
