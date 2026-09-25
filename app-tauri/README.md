# app-tauri — Susurro desktop shell (v0.1.0)

Tauri v2 + React: floating pill overlay with live waveform, tray menu,
settings window (model picker, cleanup, VAD, update channel), and the
updater plugin with a quiet settings indicator.

## Develop

```sh
npm install
npm run tauri dev
```

## Build

```sh
npm run build          # frontend -> dist/
cargo check --manifest-path src-tauri/Cargo.toml
```

Signed bundles + `latest.json` come from `.github/workflows/release.yml`
on version tags. Updater public key lives in
`src-tauri/tauri.conf.json`; the private key is in CI secrets plus
`~/.local/share/susurro/tauri-signing.key` (never in git).
