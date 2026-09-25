---
name: verification
description: "How to prove a change works in this project."
---

# Verification

Generated from the constitution. Run `/constitution` to update.

## Run

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p susurro-cli -- doctor`
- `cargo run -p susurro-cli -- listen --mock --stdout`

## Prove

- `cargo fmt --all -- --check` exits 0
- `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- `cargo test --workspace` exits 0, including `mock_e2e_injects_transcript_once` and `double_hotkey_cannot_double_inject`
- `susurro doctor` reports found on real hardware

## Gotchas

- Real loop needs whisper-cli, base.en model via SUSURRO_MODEL, wl-clipboard, ydotool plus ydotoold, socat. CI uses mocks.
- Linux needs libasound2-dev and pkg-config. Tauri check needs webkit/appindicator stack plus npm build in app-tauri/.
- app-tauri/src-tauri is outside the workspace. Workspace commands never cover it.
