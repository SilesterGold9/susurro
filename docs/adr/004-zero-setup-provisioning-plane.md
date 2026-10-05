# 004: zero-setup provisioning plane

Status: accepted
Date: 2026-10-02
Relates to: post-v1.1.0 hardening, issues 58, 59, 60, 61, 65

## Context

Susurro promises talk-to-text that works even when the internet does not, for
average users on Windows and Linux. The first-run chain as of v1.1.0 breaks
that promise: the local STT adapter shells out to a `whisper-cli` binary found
only via `PATH`, models arrive through one hardcoded `curl` call (or a manual
HuggingFace download plus `SUSURRO_MODEL`), cleanup quality requires a manual
Ollama install plus model pull, and Linux injection needs five manual tool and
config steps. Onboarding, `doctor`, and `bench` detect and report these gaps
but never close them. Installer day-0 budget is 100 MB, accepted 2026-10-02,
as long as dictation works offline minutes after install.

## Decision

The project builds a provisioning plane that owns everything the app needs but
does not ship as code:

1. Link the STT engine in process with `whisper-rs` behind the existing
   `SpeechToTextPort`. The `whisper-cli` shell-out adapter stays as an opt-in
   escape hatch, never the default. Build fragility (CMake, libclang, pinned
   `whisper.cpp`) is contained in CI with pre-warmed runners for both triples.
2. Ship a `provision/` crate: signed asset manifest (Ed25519, same trust root
   as the updater key in `docs/signing-rotation.md`), versioned per-OS asset
   store, resumable downloads (`.partial` plus `Range`), hash-while-writing,
   atomic swap, N-1 version retention. An interrupted download can never
   corrupt a working install.
3. Bundle `tiny-q5_1` (~31 MB) as a Tauri resource for day-0 offline
   dictation. Fetch `base` in the background while onboarding runs and promote
   it on verify; `tiny` remains the eternal fallback.
4. Demote Ollama to an opt-in rewrite tier. Default cleanup becomes a bundled
   ONNX punctuation adapter (issue 59) with the existing fail-open chain
   (ONNX to Ollama to regex) intact.
5. Turn `doctor` into a convergence loop that runs on boot and network-regain,
   shared by CLI and GUI through one `provision::health()`, with categorized
   failures (network, permission, disk-full, checksum) surfaced in a System
   Status view. Onboarding starts the fetch on first paint and gates completion
   on engine plus day-0 model plus one working injection path only.

Sequencing is Phase 0 asset pipeline behind the current engine, Phase 1 link
`whisper-rs`, Phase 2 bundle `tiny` plus background `base` plus onboarding
rewrite, Phase 3 capability matrix UI and Linux paste honesty, Phase 4 ONNX
cleanup default, Phase 5 Parakeet opt-in (issue 58) as a manifest entry.

## Consequences

Fresh installs dictate offline within minutes with no terminal, no `PATH`
edits, and no manual downloads, inside the 100 MB day-0 budget. Model updates
bypass the app updater, so weekly fixes stop re-downloading hundreds of
megabytes. The `SpeechToTextPort` boundary keeps Parakeet, cloud, and mock
backends working unchanged, and each phase ships independently with no
flag-day.

The project takes on build complexity (pinned native deps, dual-triple CI),
model-hosting bandwidth, and a permanent ~30 MB bundled-model floor. Wayland
restrictions stay honest limits: no universal zero-setup global hotkey plus
auto-paste exists there, so GNOME and KDE fall back to clipboard mode by
design, and Windows cannot inject into elevated windows from medium integrity.

## Alternatives

The project rejected Tauri sidecar `whisper-cli` binaries, because they solve
none of the model distribution problem and add version skew, antivirus
heuristics, and full-installer updater downloads. The project rejected
bundling `small` or larger models inside the installer, because the updater
has no binary diff and weekly fixes would re-download hundreds of megabytes.
The project rejected lazy download with no day-0 model, because first-run
offline would mean a dead app. The project rejected starting with
`sherpa-onnx` plus Parakeet as the default engine, because the 680 MB model
and monthly upstream churn cost more than the accuracy buys before the
provisioning pipeline itself is proven.

## Links

* `adapters-stt-local/src/lib.rs` implements the current shell-out `WhisperLocal`
* `core/src/ports.rs` defines `SpeechToTextPort` the linked engine implements
* `adapters-cleanup/src/lib.rs` implements the Ollama plus regex fail-open chain
* `app-tauri/src-tauri/src/main.rs` implements `download_model`, `run_bench`, onboarding gating
* `app-tauri/src/onboarding.tsx` implements the six check-and-hint screens
* `docs/signing-rotation.md` defines the trust root the manifest reuses
* `docs/adr/002-whisper-cpp-local-stt.md` records the shell-out decision this supersedes
* `susurro-project-plan.md` defines the offline-first promise and 100 MB day-0 budget
