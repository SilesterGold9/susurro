---
id: 2026-10-05-transforms
date: 2026-10-05
status: binding
canon: prove-it-works
question: Can post-dictation rewrites ship without lying about which ones discard the speaker's words?
verdict: Yes, and the honesty requirement is what shaped it. `core/src/transforms.rs` names four rewrites and marks which discard words. Only Tidy runs on the bundled engine; the other three are labelled rewrites, need the opt-in LLM tier, and report `unavailable` rather than a fake result when the tier fails open. Preview is the default in both surfaces.
evidence:
  - core/src/transforms.rs: Transform { Tidy, Organize, Shorten, Formalize } with rewrites(), tier(), prompt(), parse(); 4 tests (58 -> 62 core) covering tier routing, both spellings of organize/formalize, prompt shape, and that every rewrite blurb says so
  - storage/src/lib.rs: set_cleaned writes cleaned_text and never raw_text, returns the row count. set_cleaned_rewrites_the_clean_column_only proves raw survives and an unknown session reports 0 rows rather than inventing one (16 -> 17 storage tests)
  - Tauri: run_transform shared by preview_transform and apply_transform, so the two surfaces cannot diverge. transform_candidates serves every stored session, not only ones with a cleaned column, because reworking yesterday's notes is the normal case
  - live proof, CLI on this machine's real history: `transform tidy` -> "result (tidied on device): The deploy failed again, we need to roll back to the previous version immediately", "preview only. Store it with --apply." then `--apply` -> "stored. The raw transcript is untouched", then `restore` -> "restored raw transcript (80 chars)."
  - live proof, the honesty path: `transform shorten` with no server -> "result: unavailable. The ollama tier failed open to the tidier, and Shorten rewrites text." Nothing stored. The same guard rejects a fake rewrite in the Tauri path
  - transforms.tsx replaces the placeholder: session picker, four buttons with on-device vs rewrites-text badges, preview card that spells out when a model rewrote, "use this" disabled until a preview exists. Session ids are listed, not guessed
  - cargo fmt --all clean; workspace clippy --all-targets -D warnings clean; cargo test --workspace green (24 suites); src-tauri clippy -D warnings clean and 3 tests green; npm run build green (145 modules)
filed_by: cardinal
---

Two findings worth carrying forward, both from things that went wrong
mid-build rather than from the design.

Inserting a method into storage broke clippy in a way that looked
unrelated: `#[allow(clippy::let_and_return)]` sat directly above
`recent`, and my new function landed between the attribute and its
function, so the allow stopped applying. The lint that then fired was
about a `let` binding in code I had not touched. Lesson: when an
attribute is load-bearing, insert after the whole annotated item, not
inside it.

Disk hit zero mid-session and a 0-byte storage/src/lib.rs was the
symptom. Cause: the sherpa-onnx crate had cached both a static (1010
MB) and a shared (22 MB) prebuilt archive, and we only use shared.
Deleted the static tree, recovered the file from git, and left 3.8 GB
free. Worth knowing that `target/sherpa-onnx-prebuilt` holds a GB if
the default (static) features are ever enabled.

Honest gaps. The rewriting tiers are unproven against a live model:
Ollama was not running here, so organize/shorten/formalize are proven
only along their failure path, which is the path that matters most but
is not the path a user wants. Also, transform starts from raw every
time, so stacking two transforms cannot compound one model's rewrite
into the next; that is deliberate and means applying a transform
discards the previous transform's output, not the raw.

Next: 57 scratchpad, then the 60/61 ruling. Queue: 57, 60, 61, 62, 65
plus 58 deferred.