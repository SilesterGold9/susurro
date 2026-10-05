---
id: 2026-10-02-provision-phase3
date: 2026-10-02
status: binding
canon: prove-it-works
question: Can one capability matrix serve CLI doctor and a GUI System page from the same facts?
verdict: Yes. provision::health() reports every manifest asset across store and bundled copies; Tauri system_status merges it with requirements, resolution, and prefetch; the System page renders it; doctor prints it.
evidence:
  - provision/src/health.rs (AssetHealth/AssetCopy/CopyKind, size as cheap signal, 3 unit tests) — 13 provision tests pass
  - main.rs requirements_data shared by requirements_status and system_status (engine kind+version, models array, resolved_path, tier, prefetch_running); handler registered
  - app-tauri/src/pages/system.tsx (Engine/Models/Environment cards, active/size-ok badges) + shell nav entry + Help pointer; Badge finally has callers
  - cli doctor provision section: both assets found, size ok, manifest version 1 recorded
  - cargo fmt clean; workspace clippy + tests green; src-tauri clippy -D warnings + tests green; npm run build green
  - live proof: doctor resolves tiny.en.bin via the persisted bench tier (tier-aware promotion working on this machine)
filed_by: cardinal
---

The requirements object is now the single source both screens read,
so onboarding and System can never disagree. Remaining plane work:
boot-time convergence loop (doctor-as-daemon with backoff), ONNX
cleanup default (Phase 4), Parakeet opt-in (Phase 5). Open queue
unchanged: issues 56, 57, 59-62, 64, 65.
