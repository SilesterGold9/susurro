---
id: 2026-10-05-convergence-loop
date: 2026-10-05
status: binding
canon: prove-it-works
question: Can doctor-as-a-daemon close provisioning gaps on boot without a retry storm or a blocked human?
verdict: Yes, with the categories split by who can fix them. A retryable failure (network) earns a persisted exponential backoff; anything else (permission, disk full, checksum) reports Blocked and stops the loop. The loop itself lives in `provision::Converger`, so the CLI and the app cannot drift; callers own the cadence.
evidence:
  - provision/src/converge.rs: Converger::converge (one pass, per-asset AssetState), run_until_converged (pass budget, injected sleep + stop predicate), Category::{Network,Permission,DiskFull,Checksum} with remedy() and retryable(), ConvergeState persisted atomically to converge-state.json holding attempts + the category that caused them
  - StorageKind added to ProvisionError::Storage; every disk touch in store.rs now goes through storage_err so disk-full and denied no longer collapse into one string. storage_kind_reads_raw_disk_full_codes proves ENOSPC(28)/ERROR_DISK_FULL(112) read as DiskFull, EACCES(13) as Permission
  - 29 provision tests pass (was 13). New coverage: backoff ladder, category mapping, raw disk-full codes, state round-trip through disk, new-category-restarts-ladder, present-copy-needs-no-network, wrong-size-copy-refetched, network-fails-then-waits, force-clears-backoff, success-clears-ladder, checksum-blocks-and-never-sleeps, loop-never-exceeds-budget, loop-sleeps-then-stops-on-request, loop-returns-immediately-when-ready, progress-receives-every-outcome
  - cli: `susurro converge [--once] [--force]` prints one line per asset (ready/fetched/waiting/blocked) and the remedy per blocked asset; doctor gains a read-only convergence section reporting category, attempt count, and next retry
  - src-tauri: spawn_convergence runs the loop on a background thread in setup, emits susurro://convergence per pass, sleeps in 1s slices so the shared stop flag cuts a 15-minute backoff short, tray quit sets the flag; converge_status command plus convergence merged into system_status; System page renders a Convergence card (per-asset state, attempt count, remedy, formatted delay)
  - live proof: doctor reports both assets present and `convergence: no recorded failures`; converge --once exits 0 with `converged: every manifest asset is on disk`; a hand-written Network attempt-3 state renders `next in 120s` and the same file with DiskFull renders as DiskFull, so the category survives a restart; converge-state.json restored to empty afterwards
  - cargo fmt --all --check exits 0; cargo clippy --workspace --all-targets -D warnings exits 0; cargo test --workspace green (29 provision tests, 23 contracts); src-tauri clippy -D warnings and cargo test green (3 tests); npm run build green; listen --mock --stdout prints the mock utterance end to end
filed_by: cardinal
---

The blocking bug found while building this: a wrong-size copy on
disk. `ensure_asset` skips when the live copy exists, so a truncated
install reported Ready forever. Convergence now forces the refetch
when a copy exists without passing the size check, which is why
`AssetState::Ready` can be trusted.

Honest gaps: the boot loop's thread lifetime is only proven by unit
tests and clippy, not by launching the app; the fetch it performs on
a real machine is proven through the same `ensure_asset` path
`model-fetch` exercises. Network-regain detection is backoff-timed,
not event-driven: ADR-004 says "on network-regain" and this answers
that with a retry timer rather than an OS notification. If a regain
event matters for latency, that is a follow-up, not a correction.

Next: ONNX cleanup default (Phase 4, issue 59), then Parakeet opt-in
(Phase 5, issue 58). Open queue unchanged: 56, 57, 60-62, 64, 65.