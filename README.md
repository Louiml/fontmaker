# FontMaker

A typeface editor written in pure Rust. No webview, no Electron, no browser
engine: the interface is native (iced) and the font formats are written by our
own code, down to the byte level.

Design glyphs with an Illustrator-style pen tool, then export the same outlines
as TrueType, WOFF, WOFF2 or a variable TrueType font.

**This is work in progress.** It can already draw, edit and ship a real font,
but it is not a finished product. The honest scorecard is below.

## What works today (verified by tests)

- **Document model** (`fontmaker-core`): glyphs with cubic Bézier contours,
  composite glyphs (components with offset and scale), anchors, positional
  forms and joining types, full undo/redo history, and versioned JSON project
  files that refuse to open formats from a newer FontMaker instead of
  corrupting them.
- **Editor GUI** (`fontmaker-app`, iced): select/pen/rectangle/ellipse/pan
  tools, on-canvas node and Bézier-handle editing, guides with snapping,
  clipboard, mirroring, zoom/fit camera, a glyph grid with renaming, a kerning
  pair editor with live pair preview, a reference-image underlay, a font info
  panel, a text preview panel, and a component inspector.
- **TrueType export** (`fontmaker-export`): `head hhea maxp hmtx cmap name
  OS/2 post glyf loca`, automatic conventional hollow-box `.notdef`, and
  composite glyphs in `glyf`.
- **OpenType layout**: GSUB carries the Arabic `init`/`medi`/`fina` forms, GPOS
  carries kerning pairs. Both are proven end to end: the `fontmaker-shaping`
  tests shape real text with allsorts and check that the substitutions and
  kerning actually reach the shaper.
- **Web formats**: WOFF 1.0 and WOFF2. The WOFF2 encoder is hand-written (no
  pure-Rust one existed); tables are brotli-compressed with the null transform,
  which the specification allows. Files open in fontTools and browsers.
- **Variable fonts**: axes, per-glyph outlines captured at axis extremes, and
  `fvar` + `gvar` export. skrifa instantiates the result and the outline lands
  where the capture said it should (this only started working recently — see
  history).
- **Scripts** (`fontmaker-script`): hand-curated charsets and joining classes
  for Latin, Cyrillic, Greek, Arabic (from Unicode's ArabicShaping.txt) and
  Hebrew, plus automatic generation of the Arabic positional form glyphs.
- **Testing**: 112 tests. Round-trip tests read every produced font back with
  the independent `read-fonts` parser; `tools/validate_fonts.py` re-validates
  all three web formats with fontTools (parse, decode, recompile). As of the
  latest commit: all tests pass and fontTools reports every sample clean.

## What is stubbed or half-done

Honesty section — things that exist but do not do real work yet:

- **Boolean operations**: the "Unify" button in the app is a stub. It returns
  the outlines unchanged and says so in the status bar. No union, subtract or
  intersect exists anywhere in the codebase.
- **Variation editing UI**: `fvar`/`gvar` export works, but the app has no
  axis sliders, no "capture at location" workflow, and no interpolated
  preview. Named instances are not written. Composite glyph deltas are
  skipped. A variation whose outline has a different point count than the
  default is silently dropped.
- **Metrics are not varied**: phantom-point deltas are written as zero, so
  advance widths do not change across the design space.
- **Live preview**: `fontmaker-shaping` exists but is only wired into tests.
  The app's preview panel draws glyphs directly and does not run the shaper,
  so you cannot see GSUB/GPOS effects (joining, kerning) while editing.
- **WOFF2**: null transform only, so files are bigger than a fully
  transformed WOFF2 would be. Valid, but not optimal.

## What is missing before this could be called finished

Roughly in the order a type designer would hit them:

1. **Mark positioning** — no mark-to-base or mark-to-ligature GPOS. Arabic and
   Hebrew vowel marks will not position correctly. This is the biggest
   functional gap for the Arabic/Hebrew focus.
2. **More GSUB/GPOS features** — ligatures, contextual forms beyond
   init/medi/fina, and an actual feature editor instead of hardcoded behavior.
3. **Real boolean operations** — union at minimum; needs a path boolean
   library or a hand-rolled engine.
4. **Variation UI** — axis management, capture, interpolation preview, named
   instances, `avar`.
5. **Charset depth** — accented Latin (no Ä/é/å at all today), Arabic
   presentation forms and marks, punctuation beyond the basics. Current
   coverage is deliberately "enough to set real text, not the whole block".
6. **Interoperability** — no UFO read/write, no designspace, no OTF/CFF or
   CFF2 export, no importing an existing font for editing. A font editor that
   cannot round-trip with the rest of the ecosystem is an island.
7. **Production hardening** — autosave (the history stack exists, autosave
   does not), crash recovery, overlap/winding QA checks, sidebearing locks,
   measurements between points.
8. **Shipping** — no CI, no releases or installers, no icon, tested only on
   Windows by the author, no accessibility or localisation pass.

Hinting (TrueType instructions) is not on the roadmap at all yet; it is listed
here so the omission is deliberate and visible.

## Honest overall assessment

The hard, format-level foundation is largely done and trustworthy: the
document model, the editors, the TrueType family of exporters, real GSUB/GPOS
that a real shaper consumes, and a test harness that validates output with
independent readers. That is the half of a font editor that most people never
see, and it is the half that is hardest to retrofit.

What remains is the long tail: interop (UFO/OTF/import), mark positioning,
boolean ops, the variation UI, character coverage, and everything it takes to
hand this to a stranger (packaging, docs, crash safety). As a rough gut
estimate by area: core model ~90%, static export ~85%, GUI editing ~60%,
variable fonts ~40%, script coverage ~35%, interop 0%, production/shipping
~15%. Blended, call it **half-finished — the foundational half**. It is
usable today for drawing and exporting simple Latin/Arabic/Greek/Cyrillic
fonts; it is not yet usable as a daily driver for real type work.

## Layout

| Crate                 | Purpose                                                        |
| --------------------- | -------------------------------------------------------------- |
| `fontmaker-core`      | Document, glyph and geometry model, edit history, project JSON |
| `fontmaker-script`    | Charsets, joining rules and form generation per script         |
| `fontmaker-export`    | TrueType, WOFF, WOFF2, GSUB/GPOS, fvar/gvar writers            |
| `fontmaker-shaping`   | Shapes exported fonts with allsorts (currently tests only)    |
| `fontmaker-app`       | The iced interface                                             |
| `xtask`               | Code generation                                                |

## Build and test

```
cargo build --workspace
cargo test --workspace --lib --tests
```

`cargo test` also runs doctests, which needs the `rustdoc` component. It is not
present in every toolchain installation, so the commands above skip it.

## Validating a font against fontTools

Needs Python with `fonttools` and `brotli`:

```
pip install fonttools brotli
cargo run -p fontmaker-export --example write_samples
python tools\validate_fonts.py
```

This writes the sample fonts (Latin and Arabic, in all three formats) into
`target/samples` and then opens each one with fontTools, checking the table
set, cmap, metrics, name records, that every drawn glyph produces real
drawing operations, and that the layout tables contain what the document
promised. It also re-saves each font, which forces a full recompile.

## License

Apache-2.0. See [LICENSE](LICENSE).
