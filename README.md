# FontMaker

A typeface editor written in pure Rust. No webview, no Electron, no browser
engine: the interface is native and the font formats are written by our own
code.

Design glyphs with an Illustrator-like pen tool, then export the same outlines
as TrueType, WOFF, WOFF2 or the other formats listed below.

## Status

Working today:

- Document model with undo/redo, autosave-ready JSON project files.
- TrueType (`.ttf`) export: `head hhea maxp hmtx cmap name OS/2 post glyf loca`.
- WOFF 1.0 (`.woff`) export.
- WOFF2 (`.woff2`) export, written by hand because no pure-Rust WOFF2 encoder
  exists on crates.io. Tables use the null transform, which the specification
  allows; the files are valid and load in browsers and font tools.
- `.notdef` gets the conventional hollow box automatically.
- Round-trip tests that read the produced fonts back with `read-fonts`, plus
  `tools/validate_fonts.py`, which validates them independently with fontTools.

Still to come: the iced interface, the shaping-based live preview, Arabic and
Hebrew form generation, and the OTF/CFF2, UFO and SVG font writers.

## Layout

| Crate                 | Purpose                                                        |
| --------------------- | -------------------------------------------------------------- |
| `fontmaker-core`      | Document, glyph and geometry model, edit history, project JSON |
| `fontmaker-script`    | Charsets, joining rules and form generation per script         |
| `fontmaker-export`    | TrueType, WOFF and WOFF2 writers                               |
| `fontmaker-shaping`   | Build in memory, shape, return outlines for the preview        |
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

```
cargo run -p fontmaker-export --example write_samples
py tools\validate_fonts.py
```

This writes the three web formats into `target/samples` and then opens each
one with fontTools, checking the table set, cmap, metrics, name records and
that every drawn glyph produces real drawing operations. It also re-saves each
font, which forces a full recompile.

## License

Apache-2.0. See [LICENSE](LICENSE).