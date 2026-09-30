# Susurro

<img src="susurro-logo.svg" width="64" alt="Susurro logo: a cream dictation comma on a teal tile">

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
cargo run -p susurro-cli -- listen --mock --stdout
cargo run -p susurro-cli -- hyprland-bind
```

Set `SUSURRO_MODEL` to your `base.en` model path. Install `wl-clipboard`, `ydotool` (+ `ydotoold` running), `whisper-cli`, `socat` for the real loop.

## Install (v0.1.0+)

Prebuilt bundles on the [releases page](https://github.com/SilesterGold9/susurro/releases):
AppImage / .deb (Linux), NSIS setup (Windows), plus standalone `susurro` CLI binaries.

```sh
# Linux quick start
./Susurro_0.3.0_amd64.AppImage
```

The AppImage needs whisper.cpp (`whisper-cli`), a whisper model, and (for
paste) `wl-copy` + `ydotool`/`ydotoold` — see `susurro doctor`.
In-app updates check `latest.json` on the releases page and surface a
quiet indicator in settings (never a forced modal).

## v0.0.1 end-to-end (Linux/Hyprland, CLI)

1. Download the model:
   ```sh
   mkdir -p ~/.local/share/susurro/models
   # from https://huggingface.co/ggerganov/whisper.cpp — ggml base.en
   # place as ~/.local/share/susurro/models/base.en.bin
   # or: export SUSURRO_MODEL=/path/to/base.en.bin
   ```
2. Install tools: `wl-clipboard`, `ydotool` (run `ydotoold`), `whisper-cli`, `socat`.
3. Check: `cargo run -p susurro-cli -- doctor` — mic, tools, and model should read "found".
4. One-shot real run: `cargo run -p susurro-cli -- listen --seconds 6`
   Records 6s from the default mic, transcribes with base.en, pastes via wl-copy + ydotool.
   Use `--stdout` to print instead of pasting, `--mock` to skip hardware.
5. Hotkey loop:
   ```sh
   cargo run -p susurro-cli -- hyprland-bind  # add the bind to hyprland.conf
   cargo run -p susurro-cli -- daemon
   ```
   Press SUPER_SHIFT+R, speak, and the transcript is pasted at the cursor.
   `cli/tests/e2e_mock.rs` proves the same loop hardware-free in CI.
   The `hyprland-bind` output also prints the pill overlay rule block
   (float, fixed size, bottom-center move, no border, shadow, blur, or
   focus). The compositor owns placement on Wayland, so the rule block
   in `windowrules.conf` is what docks the pill, not client positioning.

## Workspace

- `core/` — state machine, pipeline, port traits. No platform imports.
- `adapters-audio/` — cpal 16kHz mono capture + mock
- `adapters-stt-local/` — whisper.cpp via binary (+ OpenVINO iGPU encoder offload where present) + mock
- `adapters-stt-cloud/` — stub until v0.3.0
- `adapters-cleanup/` — passthrough until v0.1.0
- `adapters-linux/` — Hyprland socket + wl-copy/ydotool paste
- `adapters-windows/` — stub until v0.6.0
- `storage/` — in-memory stubs until v0.2.0 SQLite
- `cli/` — `susurro doctor`, `listen`, `daemon`, `listen-once`, `hyprland-bind`, `bench`
- `app-tauri/` — UI lands in v0.1.0, placeholder only
- `.github/workflows/` — CI + release skeletons

## Conventional commits

This repo uses conventional commits for versioning (`release-plz` / `git-cliff` in later milestones). Write commits like `feat(core): ...`, `fix(linux): ...`.
