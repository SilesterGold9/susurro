# Architecture decision records

This folder holds accepted records for Susurro v0.7.0 polish and dx work. Each record states context, decision, consequences, alternatives, and links to real files. Read the plan first, then read records in numeric order.

## Index

* 000: adr template. Copy this file to start a new record.
* 001: Tauri overlay shell. Why Tauri v2 owns pill, tray, settings, and updater. See `001-tauri-overlay.md`.
* 002: whisper.cpp for local speech to text. Why the external binary, partial windows, mock, and OpenVINO policy form the local path. See `002-whisper-cpp-local-stt.md`.
* 003: hexagonal architecture with ports and adapters. Why core stays platform free and adapters implement port traits. See `003-hexagonal-ports-adapters.md`.

## Grounding files

* `core/src/ports.rs`
* `core/src/state.rs`
* `app-tauri/`
* `adapters-stt-local/`
* `susurro-project-plan.md`
