# Susurro logo — handover notes

## What was decided and why
Redesign of the existing waveform tile (see `brand/brief.md` for the critique and equity audit).
Three directions were built and tested (sheet: `brand/work/concepts.png`); **C, Dictation Comma**, was approved.
Rationale: derived from what dictation does (speech becomes text), clear of every voice-app cliché
(no mic, no broadcast arcs, no equalizer bars), two shapes that survive 16 px and one colour.
Full test evidence: `brand/work/preview.html` (size ladder, pixel test, backgrounds, treatments incl. mirrored/rotated,
contexts) plus audit scores 96–100/100 and 16 px renders in `brand/work/renders-16/`.

## File map
- `susurro-symbol.svg` — primary master (teal tile, cream comma).
- `susurro-symbol-small.svg` — small-size cut, chunkier comma; source for all favicon rasters. Use below 32 px.
- `susurro-symbol-standalone.svg` — teal comma, no tile (light backgrounds).
- `susurro-symbol-standalone-reversed.svg` — cream comma, optically thinned ~5 % against irradiation (dark backgrounds).
- `susurro-horizontal.svg` / `-reversed` / `susurro-stacked.svg` / `susurro-wordmark.svg` — lockups.
- `dist/` — script-built one-colour set from the standalone master (black/white/teal + square + PNGs),
  hand-built knockout one-colour tile and lockups (`-knockout-black`, `-black`, `-mono-0f6e56`), all-white horizontal.
- `dist/web/` — favicon.svg/.ico/-16/-32/-48, apple-touch-icon, icon-192/512, maskable-512, site.webmanifest, head-snippet.html.
- `board/` — final presentation (`presentation.html` + `slides/`), software-industry mockups (README, app icon, website, terminal, sticker, social).

## Known limitations (be honest downstream)
1. **Lockup wordmarks use live `<text>`** (Inter / Liberation Sans / Arial stack, bold). No font tooling
   (fontTools, Inkscape) was available in this environment to convert to outlines, and hand-drawing seven
   glyphs would have been worse craft. Before sending anything to print or a third party: open
   `susurro-horizontal.svg` in a vector editor with the brand font, convert text to outlines, re-check
   the `-2` letter-spacing. PNG exports in `dist/` and `renders/` are already correctly baked with Liberation Sans.
2. **Masters are stroke-based** (comma tail) with nested transforms. The audit flags this as INFO;
   it is documented, not hidden: strokes render identically in every tested backend (rsvg-convert, Chrome).
   Expand strokes to fills during the outline-conversion pass above if a cutting/embroidery vendor requires it.
3. Maskable icon reuses the full-bleed tile; the comma stays inside the ~80 % safe zone by construction
   (art spans roughly the central 60 %).
4. No trademark clearance was performed. Run a professional search plus a reverse-image search before release.

## Suggested rollout (redesign §6)
1. Replace `susurro-logo.svg` at the repo root with `brand/kit/susurro-symbol.svg` (keep the old file in git history).
2. Wire `dist/web/` into the Tauri app icons and any site `<head>`.
3. Roll out consistently; update `assets/` and docs on day one so old and new marks never mix.
