# 001: Tauri overlay shell

Status: accepted
Date: 2026-09-30
Relates to: v0.7.0 polish and dx, issue 37, v0.1.0 overlay work

## Context

Susurro needs a small desktop shell on Linux with Hyprland and on Windows. The shell shows a pill overlay with live level, a tray menu, and a settings window. It also needs signed updates through the existing release pipeline. The plan requires feedback within about 400 ms of the hotkey press, a pill that never steals focus, and a quiet update indicator in settings.

## Decision

The project uses Tauri v2 with a React frontend for the shell. Tauri owns windows, tray, and updater. Rust in `app-tauri/src-tauri/src/main.rs` owns dictation runs, hotkey threads, and stage events. The frontend renders state only.

Concretely, Tauri configures two windows in `app-tauri/src-tauri/tauri.conf.json`. The pill uses label pill, width 420, height 72, transparent true, decorations false, always on top, skip taskbar, no focus, hidden until a run starts. The settings window uses label settings, width 480, height 720, hidden until opened from the tray. The backend positions the pill bottom center on the current monitor, emits `susurro://state`, `susurro://level`, `susurro://progress`, and `susurro://result` events, and plays start, stop, done, and error cues through the audio adapter. The updater plugin checks the release manifest, verifies the embedded public key, and surfaces availability as a settings indicator. The user decides when to apply an update.

## Consequences

The shell stays small and native feeling. One codebase covers Linux and Windows windows, tray, and updates. The pill can stay focus free, so paste lands in the app that holds the caret. The event model keeps rendering simple, because the frontend subscribes and draws.

The choice adds a Node and WebView dependency to builds and CI. Frontend and backend can drift, so command names and event names need contract care. Hyprland placement still needs a compositor hint and a drag fallback, because Wayland owns window position. Windows installer work waits for v0.6.0, but the shell shape already anticipates it.

## Alternatives

The project rejected Electron, because the runtime weight conflicts with a tool that idles in the tray and fires many times per day. The project rejected separate native shells in GTK and Win32, because two UI stacks double maintenance for a small team. The project rejected an immediate mode Rust UI in the main process, because it complicates text input, accessibility, and settings form work.

## Links

* `app-tauri/src-tauri/src/main.rs` defines commands, tray, pill placement, and dictation runs
* `app-tauri/src-tauri/tauri.conf.json` defines pill and settings windows, bundle targets, and updater endpoints
* `app-tauri/README.md` describes dev and build commands
* `core/src/ports.rs` defines `OverlayRendererPort` with idle, listening, and processing states
* `core/src/state.rs` defines the session states the pill mirrors
* `susurro-project-plan.md` v0.1.0, v0.6.0, and v0.7.0 sections
* `docs/waveform-inspiration.md` section 5 steal list for the pill
