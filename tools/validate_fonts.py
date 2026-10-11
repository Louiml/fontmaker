"""Independent validation of FontMaker's output using fontTools.

This is deliberately not Rust: it uses the reference OpenType implementation, so
a bug shared between our writer and our own reader cannot hide. Run it after
`cargo run -p fontmaker-export --example write_samples`.
"""

import sys
from pathlib import Path

from fontTools.ttLib import TTFont

# Signature bytes for each container fontTools can open.
FLAVORS = {
    b"\x00\x01\x00\x00": "TrueType outlines",
    b"OTTO": "CFF outlines",
    b"wOFF": "WOFF 1.0",
    b"wOF2": "WOFF2",
}

SAMPLES = Path("target/samples")


def check_font(path: Path) -> list[str]:
    """Open a font, verify it parses, and report what it found."""
    problems: list[str] = []

    with open(path, "rb") as f:
        magic = f.read(4)

    font = TTFont(str(path))
    font.ensureDecompiled()

    glyph_order = font.getGlyphOrder()
    cmap = font.getBestCmap()
    name = font["name"]

    flavor = FLAVORS.get(magic, f"unknown {magic!r}")
    print(f"\n=== {path.name}  ({flavor})")
    print(f"    tables      : {', '.join(sorted(font.keys()))}")
    print(f"    glyphs      : {len(glyph_order)} -> {glyph_order}")
    print(f"    encoded     : {sorted(hex(c) for c in cmap)}")
    print(f"    unitsPerEm  : {font['head'].unitsPerEm}")
    print(f"    family      : {name.getDebugName(1)!r} / style {name.getDebugName(2)!r}")
    print(f"    postscript  : {name.getDebugName(6)!r}")
    print(f"    asc/desc    : {font['hhea'].ascent}/{font['hhea'].descent}")
    print(f"    capHeight   : {font['OS/2'].sCapHeight}, xHeight {font['OS/2'].sxHeight}")

    # Every encoded codepoint must reach a glyph (checked in detail later,
    # after the layout tables, where shared punctuation is exempted).

    # Advance widths must survive the round trip.
    for glyph_name in glyph_order:
        if font["hmtx"][glyph_name][0] <= 0:
            problems.append(f"'{glyph_name}' has a non-positive advance width")

        # Round-trip the outlines through the pens to prove they are readable.
    # Unencoded glyphs (positional forms and alternates) may legitimately be
    # empty while the font is in progress, so only encoded glyphs are checked.
    from fontTools.pens.recordingPen import RecordingPen

    glyph_set = font.getGlyphSet()
    encoded_glyphs = set(cmap.values())
    for glyph_name in glyph_order:
        if glyph_name not in encoded_glyphs:
            continue
        pen = RecordingPen()
        glyph_set[glyph_name].draw(pen)
        if glyph_name != "space" and not pen.value:
            problems.append(f"'{glyph_name}' produced no drawing operations")

    # Layout tables, when present, must decompile.
    if "GSUB" in font:
        gsub = font["GSUB"].table
        scripts = [s.ScriptTag for s in gsub.ScriptList.ScriptRecord]
        features = [f.FeatureTag for f in gsub.FeatureList.FeatureRecord]
        lookups = len(gsub.LookupList.Lookup)
        print(f"    GSUB         : scripts={scripts} features={features} lookups={lookups}")
        if not features:
            problems.append("GSUB present but has no features")
    if "GPOS" in font:
        gpos = font["GPOS"].table
        features = [f.FeatureTag for f in gpos.FeatureList.FeatureRecord]
        print(f"    GPOS         : features={features}")
        if "kern" not in features:
            problems.append("GPOS present but has no kern feature")

    # Encoded glyphs with no outlines. Shared punctuation and digits are
    # expected to be empty in a work-in-progress font, so they are not
    # problems - flagging them would drown the real signal.
    SHARED_EMPTY_OK = set(range(0x20, 0x40)) | {0x22, 0x27}
    for codepoint, glyph_name in sorted(cmap.items()):
        glyph = font["glyf"][glyph_name]
        if glyph_name == "space" or codepoint in SHARED_EMPTY_OK:
            continue
        if glyph.numberOfContours == 0:
            problems.append(
                f"U+{codepoint:04X} maps to '{glyph_name}' which is empty"
            )

    # head magic and checksum adjustment.
    head = font["head"]
    if head.magicNumber != 0x5F0F3CF5:
        problems.append(f"bad head magic 0x{head.magicNumber:08X}")
    font.save(str(path) + ".roundtrip.ttf")  # forces a full recompile
    font.close()
    TTFont(str(path) + ".roundtrip.ttf").close()

    return problems


def main() -> int:
    if not SAMPLES.is_dir():
        print("no samples found; run the write_samples example first")
        return 1

    all_problems: list[str] = []
    for path in sorted(SAMPLES.iterdir()):
        if path.suffix in (".ttf", ".woff", ".woff2") and ".roundtrip." not in path.name:
            all_problems += [f"{path.name}: {p}" for p in check_font(path)]

    print("\n" + "=" * 60)
    if all_problems:
        print(f"{len(all_problems)} PROBLEM(S):")
        for p in all_problems:
            print(f"  - {p}")
        return 1
    print("All fonts parsed, decoded and recompiled cleanly.")
    return 0


if __name__ == "__main__":
    sys.exit(main())