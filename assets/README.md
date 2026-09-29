# assets

Logo source: `../susurro-logo.svg` (dictation comma: cream comma on a teal tile).
Masters, lockups, one-colour variants, and the favicon pack live in
`../brand/kit/` under the rules in `../brand/kit/brand-guidelines.md`.
App and tray icons in `../app-tauri/src-tauri/icons/` are rendered from
the kit (small cut at 32px and below, primary symbol above).
Brand: teal `#0F6E56`, coral `#D85A30`, dark `#1C1C1A` / light `#F1EFE8`.

## Icon render log (reproducible)

Tools are rsvg-convert 2.62.3 and ImageMagick 7.1.2-23. ImageMagick
resizes PNGs only, so the steps below assemble ICO and ICNS separately.

```sh
I=app-tauri/src-tauri/icons
rsvg-convert -w 32 -h 32 brand/kit/susurro-symbol-small.svg -o $I/32x32.png
rsvg-convert -w 64 -h 64 brand/kit/susurro-symbol.svg -o $I/64x64.png
rsvg-convert -w 128 -h 128 brand/kit/susurro-symbol.svg -o $I/128x128.png
rsvg-convert -w 256 -h 256 brand/kit/susurro-symbol.svg -o $I/128x128@2x.png
rsvg-convert -w 512 -h 512 brand/kit/susurro-symbol.svg -o $I/icon.png
```

`icon.ico` holds PNG-compressed entries 16/24/32/48/64/128/256 built from
the renders above (small cut up to 32, primary above). `icon.icns` holds
PNG payloads icp4/icp5/icp6/ic07/ic08/ic09/ic10 (16 to 1024) plus a TOC.
`cli/tests/brand_assets.rs` parses both files back and fails the suite
on size mismatch.

## Notes

- The retired root logo carried a C2PA manifest from its generator. The
  new master has none. Nothing was stripped; the manifest never existed
  for the new art.
- No trademark or reverse-image clearance has run. Gate the first public
  release on it (see `../brand/kit/handover.md` open items).
- Deleted template leftovers (Windows Store, android, ios icon sets)
  belong to untargeted platforms. Regenerate from the kit with the
  commands above if a store or mobile target ever lands.
