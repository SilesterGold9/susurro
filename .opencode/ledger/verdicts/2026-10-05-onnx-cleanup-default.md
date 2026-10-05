---
id: 2026-10-05-onnx-cleanup-default
date: 2026-10-05
status: binding
canon: prove-it-works
question: Can default cleanup drop the "install Ollama, pull a 400 MB LLM" step without losing punctuation?
verdict: Yes, for a first run that reads like speech. The default tier is now a bundled 7.6 MB int8 CNN-BiLSTM ONNX punctuation model running in-process; Ollama is demoted to an opt-in rewrite tier and the fail-open chain to regex is intact everywhere. Ollama's absence is no longer a first-run gap.
evidence:
  - provision manifest gains two pinned assets: punct-cnn-bilstm.int8.onnx (7,490,500 bytes, sha256 9d611f44...) and punct-bpe.vocab (149,430 bytes, sha256 e118b7ad...) fetched through the existing plane; PUNCT_MODEL_NAME/PUNCT_VOCAB_NAME exported so callers never hardcode a path
  - adapters-cleanup/src/punctuate.rs: PunctuateCleanup behind the existing TextPostProcessorPort. accept_or_fallback is the safety property and is testable with no model on disk: punctuation and case pass through, changed words fall back to regex, empty output is treated as failure not deletion
  - process-wide session cache keyed by (model, vocab) so weights load once and new files on disk reload; sherpa-onnx documents one object safe for single-object use, and dictation is already serialised by the in-flight guard
  - adapters-cleanup::by_name(tier, punct, ollama_model) is the one place that decides the chain, so CLI and app cannot disagree about what "onnx" means; four tests cover tier resolution, the bad-name error, the missing-models-dir message, and fail-open with no model
  - live proof, this machine, real 148 MB base model: "so i was thinking we should ship this on friday but then the build broke and now we are late" -> "So I was thinking we should ship this on Friday, but then the build broke. and now we are late". Three more utterances confirm comma placement, question marks, and sentence splits. listen --mock --stdout needs no --cleanup flag now: onnx is the default
  - doctor reports "punctuation model (onnx): ready" and reframes the Ollama lines as opt-in ("only matters for --cleanup ollama")
  - System page shows the active cleanup tier and bundled-vs-pending state; settings lists onnx first with ollama marked opt-in; settings default and CLI default both moved to onnx
  - 18 adapters-cleanup tests pass (was 10); cargo fmt --all clean; workspace clippy --all-targets -D warnings clean; cargo test --workspace green; npm run build green; src-tauri clippy -D warnings and tests green
filed_by: cardinal
---

The blocker, recorded because it will bite anyone who tries this
next: sherpa-onnx 1.13.8 ships no Windows build against the dynamic
CRT. Both its static and shared archives are `static-MT-Release`, and
whisper-rs-sys builds `MD_DynamicRelease`, so linking the two fails
with LNK2038 RuntimeLibrary mismatch plus duplicate MSVC symbols. The
fix is the shared feature: sherpa-onnx-c-api.dll and friends move the
CRT boundary behind a DLL, and the binary links clean. Cost is four
DLLs (~22 MB) that must be staged next to the binary, which is what
`stage_native_runtime` in build.rs and `extend_dll_search_path` in
adapters-windows exist for: Tauri resources land in a `resources`
subdirectory the Windows loader does not search, so without the
SetDllDirectoryW call the DLLs would be invisible and punctuation
would silently degrade to regex.

Two honest gaps. The model URL is a HuggingFace mirror of the
sherpa-onnx release, not k2-fsa's own release host; the hash is
pinned so bytes cannot drift, but the canonical URL should replace it
before release. And the 7.6 MB pair is bundled as a Tauri resource
rather than fetched, so day-0 budget moves from ~77 MB to ~85 MB
against a 100 MB ceiling; the static-CRT Windows build that would
avoid shipping the DLLs at all is a separate piece of work.

Also learned: `listen-once` bypasses the cleanup chain entirely
(PassthroughCleanup by design), so it proved nothing about
punctuation. `listen --mock` runs the real chain, which is why the
live proof uses it. Added `--mock-text` so a realistic utterance can
be driven through without a microphone.

Next: Phase 5 Parakeet opt-in tier (issue 58). Open queue unchanged:
56, 57, 60-62, 64, 65.