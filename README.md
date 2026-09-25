# Susurro

> Talk-to-text that works even when the internet doesn't.

Offline-first voice dictation for Linux (Hyprland) + Windows. Local STT by default, optional free-tier cloud (Groq, NVIDIA NIM) when online.

See `susurro-project-plan.md` for the full plan. This repo starts at **v0.0.1 — Hello, voice**.

## v0.0.1 scope

- Hyprland keybind triggers a socket call into the app
- Record audio, run `whisper.cpp` `base.en` on CPU
- Raw clipboard paste, no cleanup
- No UI, no persistence — prove capture-to-injection end to end
- `ci.yml` + `release.yml` skeletons from the first commit

## Quick start

```sh
cargo test
cargo run -p susurro-cli -- doctor
cargo run -p susurro-cli -- listen-once
cargo run -p susurro-cli -- hyprland-bind
```

Set `SUSURRO_MODEL` to your `base.en` model path. Install `wl-clipboard`, `ydotool` (+ `ydotoold` running), `whisper-cli`, `socat` for the real loop.

## Workspace

- `core/` — state machine, pipeline, port traits. No platform imports.
- `adapters-audio/` — cpal capture (stub) + mock
- `adapters-stt-local/` — whisper.cpp via binary + mock
- `adapters-stt-cloud/` — stub until v0.3.0
- `adapters-cleanup/` — passthrough until v0.1.0
- `adapters-linux/` — Hyprland socket + wl-copy/ydotool paste
- `adapters-windows/` — stub until v0.6.0
- `storage/` — in-memory stubs until v0.2.0 SQLite
- `cli/` — `susurro doctor`, `listen-once`, `hyprland-bind`
- `app-tauri/` — UI lands in v0.1.0, placeholder only
- `.github/workflows/` — CI + release skeletons

## Conventional commits

This repo uses conventional commits for versioning (`release-plz` / `git-cliff` in later milestones). Write commits like `feat(core): ...`, `fix(linux): ...`.
