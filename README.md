# Susurro

<img src="susurro-logo.svg" width="64" alt="Susurro logo: a cream dictation comma on a teal tile">

> Talk-to-text that works even when the internet doesn't.

Offline-first voice dictation for Linux (Hyprland) + Windows. Local STT by default, optional free-tier cloud (Groq, NVIDIA NIM) when online.

## Install

Prebuilt bundles on the [releases page](https://github.com/SilesterGold9/susurro/releases):
AppImage / .deb (Linux), MSI plus NSIS setup (Windows), plus standalone `susurro` CLI binaries. Every asset ships with a `.sig` file and `latest.json` powers the in-app updater.

```sh
# Linux quick start
./Susurro_1.0.0_amd64.AppImage
```

The app needs whisper.cpp (`whisper-cli`), a whisper model, and (for
paste) `wl-copy` + `ydotool`/`ydotoold` — see `susurro doctor`.
In-app updates check `latest.json` and surface a quiet indicator in
settings (never a forced modal). First run walks through four setup
screens: model download, hotkey pick, test dictation, done.

## Model

```sh
mkdir -p ~/.local/share/susurro/models
# from https://huggingface.co/ggerganov/whisper.cpp — ggml base.en
# place as ~/.local/share/susurro/models/base.en.bin
# or: export SUSURRO_MODEL=/path/to/base.en.bin
```

`susurro model-check` verifies the model checksum (trust on first
use, compare after). `susurro bench` picks the model tier for the
machine; `susurro doctor` reports mic, tools, model, keys, and
updater reachability.

## CLI

```sh
cargo run -p susurro-cli -- doctor
cargo run -p susurro-cli -- listen --mock --stdout
cargo run -p susurro-cli -- listen --seconds 6
cargo run -p susurro-cli -- hyprland-bind  # add the bind to hyprland.conf
cargo run -p susurro-cli -- daemon        # hotkey loop: press SUPER_SHIFT+R, speak
```

| Command | What it does |
|---|---|
| `doctor` | Diagnose audio, tools, model, keys, updates |
| `listen`, `daemon` | Dictate once / on every hotkey (`--turbo` races cloud vs local) |
| `listen-once` | One mock utterance end to end, hardware-free |
| `hyprland-bind` | Bind snippet for the hotkey socket |
| `history`, `stats` | Transcript history / usage plus latency percentiles |
| `undo`, `restore` | Remove last injection / re-inject the raw transcript |
| `dict-add`, `dict-remove`, `dict-list` | Custom vocabulary boost |
| `key-set`, `key-clear` | Cloud API keys in the OS keyring, never plaintext |
| `privacy-add`, `privacy-remove`, `privacy-list` | Per-app local-only routing |
| `profile-add`, `profile-remove`, `profile-list` | Per-app tone: formal, casual, verbatim |
| `bench`, `stt-bench` | CPU tier benchmark / backend race with persisted winner |
| `model-check` | Verify the model checksum |
| `replay` | Session event log replay for debugging |

Saying exactly "scratch that" undoes the last session hands-free.
`susurro stats` shows per-day words, streak, top apps, dictionary
hits, and end-to-end p50/p95/p99.

## Workspace

- `core/` — state machine, pipeline, port traits. No platform imports.
- `adapters-audio/` — cpal/PipeWire capture, VAD, earcons, mocks
- `adapters-stt-local/` — whisper.cpp binary, OpenVINO offload, bench, checksum, mocks
- `adapters-stt-cloud/` — OpenAI-compatible STT, fallback chain, turbo race
- `adapters-cleanup/` — passthrough, regex, Ollama with fail-open
- `adapters-linux/` — Hyprland socket, wl-copy/ydotool paste
- `adapters-windows/` — RegisterHotKey, SendInput paste
- `storage/` — SQLite history, tickets, dictionary, profiles, settings
- `contracts/` — port contract suite every adapter must pass
- `cli/` — the table above
- `app-tauri/` — pill overlay, settings, onboarding, tray, updater
- `docs/` — ADRs, [signing and rotation policy](docs/signing-rotation.md)
- `.github/workflows/` — CI (Ubuntu + Windows) plus signed releases

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Prove every change with the
recipe there: fmt, clippy, workspace tests, doctor, mock listen.
Conventional commits, one concern per commit.
