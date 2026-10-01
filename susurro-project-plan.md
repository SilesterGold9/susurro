# Susurro — project plan

> "Talk-to-text that works even when the internet doesn't."

A cross-platform (Linux/Hyprland + Windows), offline-first voice dictation
app. Local speech-to-text and cleanup by default; optional free-tier cloud
providers (Groq, NVIDIA NIM) as a quality/speed boost when online. Built to
run well on low/mid-range CPU-only hardware (reference machine: 8-core 10th
gen Intel, iGPU only, 16GB DDR4-3200 dual channel), with a clean upgrade
path to GPU acceleration later.

## Branding

- **Name**: Susurro (Spanish/Portuguese for "whisper"). Alternates: Sussurro
  (PT spelling), Fala.
- **Tagline**: "Talk-to-text that works even when the internet doesn't."
- **Palette**: primary teal `#0F6E56`, accent coral `#D85A30`, neutrals
  `#1C1C1A` (dark) / `#F1EFE8` (light).
- **Logo**: `susurro-logo.svg` holds a dictation comma, cream on a teal
  tile. It means speech written down. Masters and rules in `brand/kit/`.
  The wordmark is `susurro,` in bold lowercase with a coral comma.
- **Typography**: system/geometric sans (Inter or platform default) — no
  custom type needed for a dev-facing tool.
- **Tone**: quiet, precise, no hype. Error messages are actionable, not
  cute.

## UI design system

Warm, restrained "Claude" chrome for anything casual (the pill, onboarding,
settings) fused with a "LeetCode"-style satisfying, data-dense treatment
for technical surfaces (history, provider health, benchmarks).

- Warm neutral surfaces, never pure black/white, in both light and dark mode.
- Teal and coral (from the logo) are the only two accent colors on screen —
  one accent action per view, everything else neutral.
- Wordmark is `susurro,` in bold lowercase sans with a coral comma;
  serif is reserved for the onboarding headline only; sans for all UI
  chrome; monospace for anything numeric or technical (latencies, model
  names, timestamps).
- History/status rows styled like LeetCode submissions: snippet, a small
  colored provider badge (local/groq/nim), latency in monospace. Badge
  colors carry exactly one meaning each — green healthy/fast, amber
  degraded, gray idle.
- The one animation worth real craft: the settle moment after a successful
  injection. Brief (150-250ms) scale + checkmark, muted rather than a
  banner — this fires dozens of times a day, so only low-intensity
  satisfaction survives past day three.
- Copy: sentence case, no exclamation points, errors state what happened
  and what to do next ("Couldn't reach Groq. Using local instead.", never
  "Oops! Something went wrong!").

## Architecture summary (see full conversation for rationale)

- **Core domain** (Rust, OS-agnostic): state machine
  (Idle → Listening → Transcribing → Cleanup → Injecting), pipeline
  orchestration, and port traits. Never imports a platform-specific crate.
- **Ports** (traits): `AudioCapturePort`, `VoiceActivityDetectorPort`,
  `SpeechToTextPort`, `TextPostProcessorPort`, `TextInjectionPort`,
  `GlobalHotkeyPort`, `OverlayRendererPort`, `SettingsStorePort`,
  `HistoryStorePort`, `NetworkStatusPort`.
- **Provider chain** (STT + cleanup): ordered adapters with circuit
  breaker + rate-limit awareness — Groq → NVIDIA NIM → local
  (whisper.cpp / Ollama). Local is the guarantee, not the fallback.
  One generic `OpenAiCompatibleAdapter`, config-driven, covers any
  OpenAI-compatible provider (Groq, NIM, future ones) instead of one
  class per vendor.
- **Idempotency**: every side effect (injection, provider call, history
  write, file write) is gated by a session-keyed "ticket" so retries,
  replays, and double-triggered hotkeys can never duplicate an effect.
- **Privacy-aware routing**: per-app policy list forces local-only
  processing in blocklisted apps (password managers, terminals),
  regardless of network status.
- **Performance**: 16kHz mono capture (no resampling), lock-free ring
  buffer, streaming/incremental STT decoding so cleanup can start on a
  partial transcript, clipboard-paste injection (not per-key), persistent
  HTTP/2 connections to cloud providers, hardware auto-benchmark on first
  run to pick model tier and backend.
- **Intel-specific**: whisper.cpp OpenVINO backend offloads the *encoder*
  to the iGPU (decoder stays on CPU — set expectations accordingly); an
  ONNX Runtime + execution-provider adapter (OpenVINO EP on Linux,
  DirectML EP on Windows) is a second candidate benchmarked against it.
- **Graceful degradation ladder**: cloud chain → local GPU-accelerated →
  local CPU-only → emergency minimal-model tier, chosen automatically.
- **UX**: feedback within ~400ms of hotkey press, waveform driven by real
  amplitude, semantic "scratch that" undo, transparent privacy-mode
  indicator, progressive disclosure (simple pill by default, power
  settings one layer down).
- **DX**: mock adapters for hardware-free dev/CI, a `susurro doctor`
  command that diagnoses environment issues (ydotoold, model checksums,
  keyring, API keys), contract tests every adapter must pass, session
  event replay for debugging.

## CI/CD & release engineering (built in from v0.0.1, not bolted on later)

- `ci.yml`: lint (`clippy`, `rustfmt --check`) + contract test suite, on
  every push/PR, on both a Linux and Windows runner from day one — even
  while only Linux adapters exist. Catches a platform assumption leaking
  into `core/` long before the Windows adapters are written.
- `release.yml`: triggered by a version tag. Cross-compiles signed
  binaries (Linux AppImage/.deb; MSI once v0.6.0 lands), generates the
  updater manifest (`latest.json`), publishes to GitHub Releases.
  Versioning + changelog driven by conventional commits (`release-plz` or
  `git-cliff`), not hand-edited.
- Stable + beta release channels from the same pipeline — ship
  continuously, dogfood pre-releases yourself, without exposing anyone
  else to a half-finished build. An explicit in-app setting, not a hidden
  flag.
- In-app updates: Tauri's updater plugin checks the manifest, verifies the
  ed25519 signature against a public key embedded in the binary,
  downloads, applies. Surfaced as a quiet settings indicator, never a
  forced modal — the user decides when to update.

## Suggested workspace layout

```
susurro/
  core/                # domain, state machine, port traits
  adapters-audio/      # cpal capture, VAD
  adapters-stt-local/  # whisper.cpp bindings + OpenVINO variant
  adapters-stt-cloud/  # Groq / NVIDIA NIM (generic OpenAI-compatible)
  adapters-cleanup/    # Ollama local + cloud LLM cleanup
  adapters-linux/      # hotkey (evdev/Hyprland), injection (ydotool)
  adapters-windows/    # hotkey (RegisterHotKey), injection (SendInput)
  storage/             # SQLite history, keyring key storage, config
  app-tauri/           # pill overlay, tray, settings window (React/Tailwind)
  cli/                 # `susurro doctor`, bench, etc.
  assets/              # logo, icons
  .github/workflows/   # ci.yml, release.yml
```

## Milestones

### v0.0.1 — "Hello, voice" (Linux proof of concept)
- [ ] Hyprland keybind triggers a socket call into the app
- [ ] Record audio, run through whisper.cpp (`base.en`, CPU, no GPU work yet)
- [ ] Raw clipboard paste, no cleanup
- [ ] No UI, no persistence — prove capture-to-injection end to end
- [ ] `ci.yml` skeleton: lint + test job running on every push, Linux and
  Windows runners both, from the very first commit
- [ ] `release.yml` skeleton and versioning config (conventional commits),
  even before there's anything worth releasing

### v0.1.0 — "It thinks"
- [ ] VAD-based auto end-of-speech detection (no more push-to-talk only)
- [ ] Local cleanup: Ollama small model + regex fallback if not installed
- [ ] Minimal Tauri overlay: pill with live waveform, idle/listening/processing states
- [ ] Basic settings window: model picker, hotkey remap
- [ ] Tauri updater plugin wired; quiet "update available" indicator in settings
- [ ] Cut the first real signed release through the pipeline (tag → CI
  builds → GitHub Release)

### v0.2.0 — "It remembers"
- [ ] SQLite transcript history, idempotent upserts
- [ ] Custom dictionary/vocabulary boost
- [ ] Injection idempotency ticket (session-keyed, exactly-once)
- [ ] Fail-open recovery: inject raw transcript if cleanup panics

### v0.3.0 — "It's not alone" (cloud provider chain)
- [ ] Generic `OpenAiCompatibleAdapter`; wire up Groq and NVIDIA NIM
- [ ] Circuit breaker + rate-limit-aware fallback chain
- [ ] `NetworkStatusPort`; API keys in OS keyring, never plaintext
- [ ] Per-app privacy policy list forcing local-only routing

### v0.4.0 — "It's fast" (performance pass)
- [ ] Lock-free ring buffer audio capture, 16kHz mono direct
- [ ] Streaming/incremental whisper.cpp decoding
- [ ] Persistent HTTP/2 connections + DNS pre-resolution to cloud providers
- [ ] Clipboard-paste injection batching
- [ ] Hardware auto-benchmark on first run → model tier auto-selection

### v0.5.0 — "It's Intel-aware"
- [ ] whisper.cpp OpenVINO encoder-offload adapter (CPU/iGPU)
- [ ] ONNX Runtime + execution-provider adapter (OpenVINO EP / DirectML EP)
  benchmarked against the above; winner becomes default
- [ ] `doctor` command checks for compute-runtime / OpenVINO runtime presence

### v0.6.0 — Windows port
- [ ] `RegisterHotKey` + `SendInput` adapters
- [ ] Windows tray icon, MSI installer
- [ ] Extend the existing CI/release pipeline (running since v0.0.1) to
  build and sign Windows binaries too

### v0.7.0 — Polish & DX
- [ ] Contract test suite every port's adapters must pass
- [ ] Mock adapters for hardware-free dev loop and CI
- [ ] `susurro doctor` fully fleshed out
- [ ] Session event log + replay for debugging
- [ ] ADRs for the big calls (Tauri, whisper.cpp, hexagonal architecture)
- [ ] Task runner (`just dev`, `just bench`, `just test-contract`, ...)

### v0.8.0 — UX depth
- [x] Semantic "scratch that" undo (last session, not just OS undo)
- [x] Per-app formatting profiles
- [x] Onboarding flow: model download + benchmark progress clearly communicated
- [x] Accessibility pass: remappable hotkeys, contrast, screen-reader settings

### v0.9.0 — Hardening / release candidate
- [x] Full observability: tracing, per-stage P50/P95/P99 latency view
- [x] Property-based tests on the state machine (`proptest`), fuzz tests on injection
- [x] Model checksum verification, atomic config/model file writes
- [x] Optional "turbo mode": speculative race between cloud and local, first response wins

### v1.0.0 — Susurro launch
- [x] Cross-platform installers (AppImage/.deb + MSI)
- [x] Promote the beta channel to stable; finalize signing-key rotation policy
- [x] Public repo, README, CONTRIBUTING

### Beyond v1.0 (stretch)
- [ ] GPU backend (CUDA/Vulkan) activated automatically when a discrete card is present
- [ ] NPU support (Meteor Lake+) via the same OpenVINO plugin path
- [ ] macOS port
- [ ] Community adapter plugins (additional STT/LLM providers)

## First action for a coding agent

Start at v0.0.1. Scaffold the Rust workspace with the layout above, stub
every port trait in `core/`, and implement only enough of
`adapters-audio`, `adapters-stt-local`, and `adapters-linux` to make the
Hyprland-keybind-to-clipboard-paste loop work with `base.en`. Everything
else in this document is sequenced on purpose — don't skip ahead to cloud
providers or the UI before the core loop is proven end to end. Set up the
CI workflow skeleton alongside it — even a single lint+test job — so every
commit from the first one runs through the pipeline instead of CI arriving
late as an afterthought.
