# Susurro — logo guidelines (compact)

> One page for the team. When in doubt, use the primary lockup as-is and give it room.

## 1. The logo
- **Idea**: a dictation comma. The smallest unit of written speech: voice, written down.
- **Versions**: primary symbol (teal tile, cream comma) · standalone comma (teal / cream) · horizontal lockup · stacked lockup · wordmark-only (`susurro,` — the coral comma is part of the name).
- **Files**: `brand/kit/*.svg` are the masters · `brand/kit/dist/` holds one-colour variants and PNGs · `brand/kit/dist/web/` holds favicon, app icons, `site.webmanifest` and the site `<head>` snippet. Screens use the SVGs/PNGs as shipped (sRGB).

## 2. Clear space
Keep a clear zone of **1 comma-head** (the diameter of the comma's round head) around the symbol on all sides, and **1 x-height** around lockups. It scales with the logo. Nothing enters the zone.

## 3. Minimum size
| Version | Screen | Note |
|---|---|---|
| Symbol (tile) | 16 px | below 32 px use `susurro-symbol-small.svg` (chunkier cut) |
| Standalone comma | 16 px | holds; do not go smaller |
| Horizontal lockup | 120 px wide | below that, use the symbol alone |
| Favicon | 16 px | `dist/web/favicon.*` (built from the small cut) |

## 4. Colour
| Name | HEX | RGB | CMYK (coated, approx) |
|---|---|---|---|
| Teal (primary) | `#0F6E56` | 15, 110, 86 | 86, 0, 22, 57 |
| Coral (accent) | `#D85A30` | 216, 90, 48 | 0, 58, 78, 15 |
| Ink | `#1C1C1A` | 28, 28, 26 | 0, 0, 7, 89 |
| Cream | `#F1EFE8` | 241, 239, 232 | 0, 1, 4, 5 |

Measured contrast: cream on teal **5.4:1** (passes text and graphics) · coral on cream **3.4:1** (graphics and large type only) · **coral on teal is 1.6:1 — never place coral on teal or teal on coral.**
Approved pairs: full colour on cream/white · standalone teal comma on light · cream comma on dark · one-colour black on light · all-white on dark. Coral belongs to the wordmark's comma, active/tray states and highlights — never to the symbol body, never on teal.

## 5. Typography
- Wordmark and UI: system/geometric sans (Inter where available, Liberation Sans/Arial metrics otherwise), bold lowercase. No custom typeface for this brand.
- Monospace for anything numeric or technical (latencies, model names, timestamps). Serif for the "Susurro" onboarding headline only (per the UI design system in `susurro-project-plan.md`).
- Never retype the wordmark with a different font or spacing. The trailing coral comma is mandatory.

## 6. Don'ts
Don't stretch or squash · don't recolour outside the pairs above · don't rotate (a rotated comma is a different punctuation mark) · don't add shadows, outlines, gradients or effects · don't rearrange lockup parts · don't put the comma art on teal in coral · don't use the old waveform bars for new surfaces (kept in `brand/work/` for history).

## 7. Masters and questions
Masters: `brand/kit/`. Working files and rejected directions: `brand/work/`. Presentation: `brand/kit/board/presentation.html`. Open items before first public release: trademark/reverse-image search; convert the lockup wordmark `<text>` to outlines (see `brand/kit/handover.md`).
