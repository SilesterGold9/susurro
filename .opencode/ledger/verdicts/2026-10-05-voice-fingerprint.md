---
id: 2026-10-05-voice-fingerprint
date: 2026-10-05
status: binding
canon: prove-it-works
question: Does a voice profile need a model, or is counting over stored history enough to be worth shipping?
verdict: Counting is enough for the three cards the issue asks for. `fingerprint()` in core is pure, deterministic, and withholds rather than guesses: a phrase used once and a peak hour from a single timestamp both read as absent rather than as a confident zero.
evidence:
  - core/src/stats.rs: VoiceFingerprint { top_words, catchphrase, peak_hour }, content_words() dropping a 90-word stopword list, all three cards fed from BTreeMaps so ordering never wobbles
  - catchphrase needs count > 1 (a phrase used once is noise) and ties break alphabetically ascending, spelled out in a loop rather than left to max_by, whose reversal reads backwards
  - peak_hour needs at least two dated rows and counts UTC hours, because that is what the stored timestamp is; the CLI prints "11:00 UTC" and the page says "Times are UTC" rather than implying local time the store cannot produce
  - 5 new core tests (58 -> 61): stopwords never reach the card, catchphrase tie-break, determinism across identical reads AND across reordered rows, thin history withholds, peak hour counts UTC and breaks ties early, summarize carries the fingerprint
  - live proof on this machine's real history: "top words: hello 5, susurro 4, again 2, should 2 ...", "catchphrase: none repeated yet", "peak hour: 11:00 UTC (4 sessions)". The catchphrase reads absent on purpose, because nothing in 111 words has repeated yet
  - CLI stats prints the three cards; get_stats exposes them as JSON with null for absent; Insights renders a Voice fingerprint section of three cards with a word-frequency bar chart
  - cargo fmt --all clean; workspace clippy --all-targets -D warnings clean; cargo test --workspace green; src-tauri clippy -D warnings clean; npm run build green
filed_by: cardinal
---

Two tests caught my own first implementation, which is the argument
for writing them. The tie-break in `max_by` was reversing the string
comparison in a way that looked alphabetical and was not, and the
stopword assertion was wrong because "hello" and "world" are not
stopwords at all. Both failures were the tests being right and the
implementation being sloppy, which is exactly what they are for.

One deliberate limit: the stopword list is small (90 entries) rather
than aggressive. An aggressive list would strip real vocabulary out of
a working user's ranking, and "not" and "because" are signal in dictation
even though they are stopwords in English prose.

Next: the queue. 56 (transforms) and 57 (scratchpad) are both designed
empty pages waiting for a feature behind them, so each is a page plus a
storage table plus a port. 60 and 61 both need whisper-cli flags that
the linked engine does not expose. 62 is a spike whose acceptance is a
day of false-positive counting on real hardware. 65 is three phases and
the largest single piece left. Open queue: 56, 57, 60, 61, 62, 65, plus
58 (Parakeet) deferred by decision.