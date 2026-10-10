//! TrueType (`.ttf`) output.
//!
//! Outlines are stored in `glyf`, so cubic curves are converted to quadratics
//! first. Everything else is the minimum set of tables a font needs to be
//! usable: `head hhea maxp hmtx cmap name OS/2 post` plus `glyf/loca`.

use std::collections::BTreeMap;

use fontmaker_core::document::{Document, Script};
use fontmaker_core::geom::simplify_path;
use fontmaker_core::glyph::Glyph;
use font_types::{Fixed, FWord, GlyphId, LongDateTime, NameId, Tag, UfWord, Version16Dot16};
use kurbo::{BezPath, PathEl, Point, Shape};
use write_fonts::{
    tables::{
        cmap::Cmap,
        glyf::{Bbox, Glyph as TtGlyph, GlyfLocaBuilder, SimpleGlyph},
        head::{Flags, Head, MacStyle},
        hhea::Hhea,
        hmtx::{Hmtx, LongMetric},
        loca::Loca,
        maxp::Maxp,
        name::{Name, NameRecord},
        os2::{Os2, SelectionFlags},
        post::Post,
    },
    FontBuilder,
};

use crate::error::{ExportError, Result};

/// Maximum error, in font units, allowed when converting cubics to quadratics.
/// One font unit at 1000 upem is invisible at text sizes.
const QUAD_TOLERANCE: f64 = 1.0;

/// Seconds between 1904-01-01 (the TrueType epoch) and the Unix epoch.
const MAC_EPOCH_OFFSET: i64 = 2_082_844_800;

/// The current time in the `head` table's 1904-based long date format.
///
/// Real editors stamp the real time; a zero timestamp makes fontTools warn on
/// every open, which looks like a bug in the font.
fn now_as_long_datetime() -> LongDateTime {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    LongDateTime::new((seconds + MAC_EPOCH_OFFSET).clamp(0, i64::MAX))
}

/// Convert a cubic path to an equivalent quadratic path.
///
/// TrueType has no cubic segments. kurbo subdivides each cubic into as many
/// quadratics as the tolerance requires, which is what font editors call
/// "curve conversion".
pub fn cubic_to_quadratic(path: &BezPath, tolerance: f64) -> BezPath {
    let mut out = BezPath::new();
    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) => out.move_to(p),
            PathEl::LineTo(p) => out.line_to(p),
            PathEl::ClosePath => out.close_path(),
            PathEl::QuadTo(c, p) => out.quad_to(c, p),
            PathEl::CurveTo(c1, c2, p) => {
                let start = out.current_position().unwrap_or(c1);
                let cubic = kurbo::CubicBez::new(start, c1, c2, p);
                for (_, _, quad) in cubic.to_quads(tolerance) {
                    out.quad_to(quad.p1, quad.p2);
                }
            }
        }
    }
    out
}

/// Round a font-unit coordinate onto the 1/64 grid TrueType stores on.
fn f26dot6(v: f64) -> i16 {
    (v * 64.0)
        .round()
        .clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

/// One glyph as the exporter sees it.
struct CompiledGlyph {
    name: String,
    ttf: TtGlyph,
    advance: u16,
    lsb: i16,
    codepoints: Vec<u32>,
}

/// Build a `.ttf` from a document.
pub fn build_ttf(doc: &Document) -> Result<Vec<u8>> {
    let order = export_order(doc);
    if order.is_empty() {
        return Err(ExportError::NoGlyphs);
    }

    let compiled = compile_glyphs(doc, &order)?;
    if compiled.is_empty() {
        return Err(ExportError::NoGlyphs);
    }

    let (glyf, loca, loca_format) = build_glyf(&compiled)?;
    let head = build_head(doc, &compiled, loca_format);
    let hhea = build_hhea(doc, &compiled);
    let maxp = build_maxp(&compiled);
    let hmtx = build_hmtx(&compiled);
    let cmap = build_cmap(&compiled)?;
    let name = build_name(doc);
    let os2 = build_os2(doc, &compiled);
    let post = build_post();

    let mut builder = FontBuilder::new();
    // Every table here is built from validated in-memory state, so a failure
    // here would be a bug in the exporter rather than bad user input.
    builder.add_table(&head).expect("head");
    builder.add_table(&hhea).expect("hhea");
    builder.add_table(&maxp).expect("maxp");
    builder.add_table(&hmtx).expect("hmtx");
    builder.add_table(&cmap).expect("cmap");
    builder.add_table(&name).expect("name");
    builder.add_table(&os2).expect("OS/2");
    builder.add_table(&post).expect("post");
    builder.add_table(&glyf).expect("glyf");
    builder.add_table(&loca).expect("loca");

    Ok(builder.build())
}

/// Glyph order for export. `.notdef` must be glyph 0 in every TrueType font.
fn export_order(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .glyph_order
        .iter()
        .filter(|n| doc.glyphs.contains_key(*n))
        .cloned()
        .collect();
    if !names.iter().any(|n| n == ".notdef") {
        names.insert(0, ".notdef".to_string());
    }
    names.dedup();
    names
}

/// The conventional hollow box drawn for `.notdef`, sized from the em square.
fn notdef_box(upem: f64) -> BezPath {
    let w = upem * 0.5;
    let h = upem * 0.7;
    let stroke = upem * 0.05;
    let mut path = BezPath::new();
    // Outer box, wound one way.
    path.extend(simplify_path(
        &fontmaker_core::stroke::rectangle(stroke / 2.0, 0.0, w - stroke / 2.0, h).to_bezpath(),
    ));
    // Inner counter, wound the other way to punch a hole.
    let mut inner = fontmaker_core::stroke::rectangle(
        stroke,
        stroke,
        w - stroke,
        h - stroke,
    );
    fontmaker_core::geom::mirror_x(&mut inner.nodes, w / 2.0);
    path.extend(simplify_path(&inner.to_bezpath()));
    path
}

fn compile_glyphs(doc: &Document, order: &[String]) -> Result<Vec<CompiledGlyph>> {
    let mut out = Vec::with_capacity(order.len());
    for name in order {
        let Some(glyph) = doc.glyph(name) else {
            continue;
        };
        out.push(compile_glyph(glyph, doc.metrics.upem)?);
    }
    Ok(out)
}

fn compile_glyph(glyph: &Glyph, upem: f64) -> Result<CompiledGlyph> {
    let ttf = if glyph.contours.iter().all(|c| c.is_empty()) {
        if glyph.name == ".notdef" {
            // A blank .notdef is technically valid but unreadable: any program
            // asking for a missing character should be able to see a box.
            TtGlyph::Simple(SimpleGlyph::from_bezpath(&notdef_box(upem))
                .map_err(|e| ExportError::MalformedGlyph {
                    glyph: glyph.name.clone(),
                    reason: format!("{e:?}"),
                })?)
        } else {
            TtGlyph::Empty
        }
    } else {
        let mut path = BezPath::new();
        for c in &glyph.contours {
            if !c.closed {
                return Err(ExportError::OpenContour {
                    glyph: glyph.name.clone(),
                });
            }
            path.extend(simplify_path(&c.to_bezpath()));
        }
        let quad = cubic_to_quadratic(&path, QUAD_TOLERANCE);
        let simple = SimpleGlyph::from_bezpath(&quad).map_err(|e| ExportError::MalformedGlyph {
            glyph: glyph.name.clone(),
            reason: format!("{e:?}"),
        })?;
        TtGlyph::Simple(simple)
    };

    Ok(CompiledGlyph {
        name: glyph.name.clone(),
        ttf,
        advance: glyph
            .advance
            .max(0.0)
            .round()
            .clamp(0.0, u16::MAX as f64) as u16,
        lsb: f26dot6(glyph.lsb),
        codepoints: glyph.unicode.iter().map(|c| *c as u32).collect(),
    })
}

fn build_glyf(compiled: &[CompiledGlyph]) -> Result<(write_fonts::tables::glyf::Glyf, Loca, i16)> {
    let mut builder = GlyfLocaBuilder::new();
    for g in compiled {
        builder
            .add_glyph(&g.ttf)
            .map_err(|e| ExportError::MalformedGlyph {
                glyph: g.name.clone(),
                reason: e.to_string(),
            })?;
    }
    let (glyf, loca, format) = builder.build();
    Ok((glyf, loca, format as i16))
}

fn build_head(doc: &Document, compiled: &[CompiledGlyph], loca_format: i16) -> Head {
    let (mut x_min, mut y_min, mut x_max, mut y_max) = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for g in compiled {
        if let TtGlyph::Simple(s) = &g.ttf {
            x_min = x_min.min(s.bbox.x_min);
            y_min = y_min.min(s.bbox.y_min);
            x_max = x_max.max(s.bbox.x_max);
            y_max = y_max.max(s.bbox.y_max);
        }
    }
    if x_min > x_max {
        // Nothing is drawn yet; a zero box keeps the font valid.
        (x_min, y_min, x_max, y_max) = (0, 0, 0, 0);
    }

    let bold = doc.meta.style_name.to_lowercase().contains("bold");
    let italic = doc.meta.style_name.to_lowercase().contains("italic");

    Head {
        // FontBuilder::build overwrites this with the real value.
        checksum_adjustment: 0,
        magic_number: 0x5F0F_3CF5,
        flags: Flags::empty(),
        units_per_em: doc.metrics.upem.round().clamp(16.0, 16384.0) as u16,
        created: now_as_long_datetime(),
        modified: now_as_long_datetime(),
        x_min,
        y_min,
        x_max,
        y_max,
        mac_style: mac_style(bold, italic),
        lowest_rec_ppem: 6,
        font_direction_hint: 2,
        index_to_loc_format: loca_format,
        font_revision: Fixed::from_f64(1.0),
    }
}

fn mac_style(bold: bool, italic: bool) -> MacStyle {
    let mut style = MacStyle::empty();
    if bold {
        style |= MacStyle::BOLD;
    }
    if italic {
        style |= MacStyle::ITALIC;
    }
    style
}

fn build_hhea(doc: &Document, compiled: &[CompiledGlyph]) -> Hhea {
    let mut min_lsb = i16::MAX;
    let mut min_rsb = i16::MAX;
    let mut x_max_extent = 0i16;
    let mut max_advance = 0u16;
    for g in compiled {
        max_advance = max_advance.max(g.advance);
        min_lsb = min_lsb.min(g.lsb);
        if let TtGlyph::Simple(s) = &g.ttf {
            min_rsb = min_rsb.min(s.bbox.x_min - g.lsb);
            x_max_extent = x_max_extent.max(s.bbox.x_max);
        }
    }
    if min_lsb > min_rsb {
        (min_lsb, min_rsb) = (0, 0);
    }

    Hhea {
        ascender: FWord::new(doc.metrics.ascender.round().clamp(-32768.0, 32767.0) as i16),
        descender: FWord::new(doc.metrics.descender.round().clamp(-32768.0, 32767.0) as i16),
        line_gap: FWord::new(doc.metrics.line_gap.round().clamp(-32768.0, 32767.0) as i16),
        advance_width_max: UfWord::new(max_advance),
        min_left_side_bearing: FWord::new(min_lsb),
        min_right_side_bearing: FWord::new(min_rsb),
        x_max_extent: FWord::new(x_max_extent),
        caret_slope_rise: 0,
        caret_slope_run: 1,
        caret_offset: 0,
        number_of_h_metrics: compiled.len() as u16,
    }
}

fn build_maxp(compiled: &[CompiledGlyph]) -> Maxp {
    let mut max_points = 0u16;
    let mut max_contours = 0u16;
    for g in compiled {
        if let TtGlyph::Simple(s) = &g.ttf {
            let points: u16 = s.contours.iter().map(|c| c.len() as u16).sum();
            max_points = max_points.max(points);
            max_contours = max_contours.max(s.contours.len() as u16);
        }
    }
    Maxp {
        num_glyphs: compiled.len() as u16,
        max_points: Some(max_points),
        max_contours: Some(max_contours),
        // Supplying the version 1.0 fields makes write-fonts emit a 1.0 header,
        // which is what every rasteriser expects.
        max_composite_points: Some(0),
        max_composite_contours: Some(0),
        max_zones: Some(1),
        max_twilight_points: Some(0),
        max_storage: Some(0),
        max_function_defs: Some(0),
        max_instruction_defs: Some(0),
        max_stack_elements: Some(0),
        max_size_of_instructions: Some(0),
        max_component_elements: Some(0),
        max_component_depth: Some(0),
    }
}

fn build_hmtx(compiled: &[CompiledGlyph]) -> Hmtx {
    Hmtx {
        h_metrics: compiled
            .iter()
            .map(|g| LongMetric {
                advance: g.advance,
                side_bearing: g.lsb,
            })
            .collect(),
        left_side_bearings: Vec::new(),
    }
}

fn build_cmap(compiled: &[CompiledGlyph]) -> Result<Cmap> {
    // (codepoint -> glyph id); the lowest codepoint wins for a duplicated glyph.
    let mut mapping: BTreeMap<char, GlyphId> = BTreeMap::new();
    for (gid, g) in compiled.iter().enumerate() {
        for cp in g.codepoints.iter().filter(|c| **c <= 0xFFFF) {
            let c = char::from_u32(*cp).ok_or_else(|| {
                ExportError::MalformedGlyph {
                    glyph: g.name.clone(),
                    reason: format!("invalid codepoint U+{cp:04X}"),
                }
            })?;
            mapping.entry(c).or_insert(GlyphId::new(gid as u32));
        }
    }
    if mapping.is_empty() {
        return Err(ExportError::EmptyCmap);
    }

    Cmap::from_mappings(mapping).map_err(|e| ExportError::MalformedSfnt(e.to_string()))
}

fn build_name(doc: &Document) -> Name {
    let full = doc.meta.full_name();
    let ps = doc.meta.postscript_name();
    let version = format!("Version {}", doc.meta.version);

    let mut name_record: Vec<NameRecord> = Vec::new();
    let mut push = |name_id: NameId, value: &str| {
        if value.is_empty() {
            return;
        }
        // Windows platform, Unicode BMP, US English.
        name_record.push(NameRecord {
            platform_id: 3,
            encoding_id: 1,
            language_id: 0x0409,
            name_id,
            string: write_fonts::OffsetMarker::new(value.to_string()),
        });
    };

    push(NameId::FAMILY_NAME, &doc.meta.family_name);
    push(NameId::SUBFAMILY_NAME, &doc.meta.style_name);
    push(NameId::UNIQUE_ID, &format!("{version};{ps}"));
    push(NameId::FULL_NAME, &full);
    push(NameId::VERSION_STRING, &version);
    push(NameId::POSTSCRIPT_NAME, &ps);
    push(NameId::TYPOGRAPHIC_FAMILY_NAME, &doc.meta.typographic_family);
    push(NameId::DESIGNER, &doc.meta.designer);
    push(NameId::MANUFACTURER, &doc.meta.manufacturer);
    push(NameId::DESCRIPTION, &doc.meta.description);
    push(NameId::LICENSE_DESCRIPTION, &doc.meta.license);

    Name { name_record, lang_tag_record: None }
}

/// Unicode range bits set in OS/2 for the script being designed, so that
/// Windows font fallback picks the right font.
fn unicode_range_for(script: Script) -> [u32; 4] {
    let mut ranges = [0u32; 4];
    match script {
        // Basic Latin, Latin-1 Supplement, Latin Extended-A.
        Script::Latin => {
            ranges[0] |= (1 << 0) | (1 << 1) | (1 << 2) | (1 << 29);
        }
        // Basic Latin, Latin-1, Greek and Coptic.
        Script::Greek => {
            ranges[0] |= (1 << 0) | (1 << 1) | (1 << 7) | (1 << 29);
        }
        // Basic Latin, Latin-1, Cyrillic.
        Script::Cyrillic => {
            ranges[0] |= (1 << 0) | (1 << 1) | (1 << 29);
            ranges[1] |= (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3);
        }
        // Basic Latin, Hebrew.
        Script::Hebrew => {
            ranges[0] |= (1 << 0) | (1 << 1) | (1 << 29);
            ranges[2] |= 1 << 0;
        }
        // Basic Latin, Arabic.
        Script::Arabic => {
            ranges[0] |= (1 << 0) | (1 << 1) | (1 << 29);
            ranges[2] |= (1 << 13) | (1 << 14);
        }
    }
    ranges
}

fn build_os2(doc: &Document, compiled: &[CompiledGlyph]) -> Os2 {
    let widths: Vec<u16> = compiled.iter().map(|g| g.advance).filter(|w| *w > 0).collect();
    let x_avg = if widths.is_empty() {
        0
    } else {
        (widths.iter().map(|w| *w as u64).sum::<u64>() / widths.len() as u64) as i16
    };

    let style = doc.meta.style_name.to_lowercase();
    let mut fs_selection = SelectionFlags::REGULAR;
    if style.contains("bold") {
        fs_selection |= SelectionFlags::BOLD;
    }
    if style.contains("italic") {
        fs_selection |= SelectionFlags::ITALIC;
    }

    let ranges = unicode_range_for(doc.script);
    let to_i16 = |v: f64| v.round().clamp(-32768.0, 32767.0) as i16;
    let to_u16 = |v: f64| v.round().clamp(0.0, 65535.0) as u16;

    Os2 {
        x_avg_char_width: x_avg,
        us_weight_class: 400,
        us_width_class: 5,
        fs_type: 0,
        y_subscript_x_size: 650,
        y_subscript_y_size: 600,
        y_subscript_x_offset: 0,
        y_subscript_y_offset: 75,
        y_superscript_x_size: 650,
        y_superscript_y_size: 600,
        y_superscript_x_offset: 0,
        y_superscript_y_offset: 350,
        y_strikeout_size: to_i16(doc.metrics.upem / 15.0),
        y_strikeout_position: to_i16(doc.metrics.x_height / 2.0),
        s_family_class: 0,
        panose_10: doc.meta.panose,
        ul_unicode_range_1: ranges[0],
        ul_unicode_range_2: ranges[1],
        ul_unicode_range_3: ranges[2],
        ul_unicode_range_4: ranges[3],
        ach_vend_id: Tag::new(b"FMKR"),
        fs_selection,
        us_first_char_index: 0,
        us_last_char_index: 0xFFFF,
        s_typo_ascender: to_i16(doc.metrics.ascender),
        s_typo_descender: to_i16(doc.metrics.descender),
        s_typo_line_gap: to_i16(doc.metrics.line_gap),
        us_win_ascent: to_u16(doc.metrics.ascender.max(0.0)),
        us_win_descent: to_u16(doc.metrics.descender.abs()),
        ul_code_page_range_1: Some(1), // Latin-1
        ul_code_page_range_2: Some(0),
        sx_height: Some(to_i16(doc.metrics.x_height)),
        s_cap_height: Some(to_i16(doc.metrics.cap_height)),
        us_default_char: Some(0),
        us_break_char: Some(32),
        us_max_context: Some(1),
        us_lower_optical_point_size: Some(0),
        us_upper_optical_point_size: Some(0),
    }
}

fn build_post() -> Post {
    // Version 3.0: no glyph name strings. Smaller, and every tool that reads
    // fonts understands it.
    Post {
        version: Version16Dot16::new(3, 0),
        italic_angle: Fixed::from_f64(0.0),
        underline_position: FWord::new(-100),
        underline_thickness: FWord::new(50),
        is_fixed_pitch: 0,
        min_mem_type42: 0,
        max_mem_type42: 0,
        min_mem_type1: 0,
        max_mem_type1: 0,
        num_glyphs: None,
        glyph_name_index: None,
        string_data: None,
    }
}

/// Table tags of a built font, in the order the SFNT directory stores them.
/// Used by the tests to prove the web formats wrap the same table set.
pub fn glyf_table_tags(font: &[u8]) -> Vec<String> {
    crate::woff2::parse_sfnt_tables(font)
        .map(|tables| {
            let mut tags: Vec<String> = tables
                .iter()
                .map(|t| String::from_utf8_lossy(&t.tag).to_string())
                .collect();
            tags.sort();
            tags
        })
        .unwrap_or_default()
}

/// Bounds of a path, or `None` when it has no ink. Used by the app for
/// selection rectangles.
pub fn ink_bounds(path: &BezPath) -> Option<kurbo::Rect> {
    if path.is_empty() {
        None
    } else {
        Some(path.bounding_box())
    }
}

/// First point of a path, used when placing anchors interactively.
pub fn first_point(path: &BezPath) -> Option<Point> {
    path.iter().next().and_then(|el| match el {
        PathEl::MoveTo(p) => Some(p),
        _ => None,
    })
}

/// The bounding box type used by `glyf`, exposed for callers building glyphs
/// by hand.
pub type GlyphBbox = Bbox;

#[cfg(test)]
mod tests {
    use super::*;
    use fontmaker_core::stroke::rectangle;

    fn doc_with_box() -> Document {
        let mut doc = Document::default();
        let mut g = Glyph::new("A").with_char('A');
        g.contours.push(rectangle(0.0, 0.0, 300.0, 700.0));
        g.advance = 400.0;
        doc.add_glyph(g);
        doc
    }

    #[test]
    fn cubic_conversion_removes_cubics() {
        let mut path = BezPath::new();
        path.move_to(Point::ZERO);
        path.curve_to(
            Point::new(50.0, 100.0),
            Point::new(150.0, 100.0),
            Point::new(200.0, 0.0),
        );
        let quad = cubic_to_quadratic(&path, 1.0);
        assert!(quad.iter().all(|e| !matches!(e, PathEl::CurveTo(_, _, _))));
        let a = path.bounding_box();
        let b = quad.bounding_box();
        assert!((a.width() - b.width()).abs() < 5.0, "width drifted");
        assert!((a.height() - b.height()).abs() < 5.0, "height drifted");
    }

    #[test]
    fn builds_a_font() {
        let bytes = build_ttf(&doc_with_box()).unwrap();
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x00], "TrueType sfnt version");
        let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]);
        assert_eq!(num_tables, 10, "expected the ten tables FontMaker writes");
        // A complete table directory plus three tiny glyphs is a few hundred
        // bytes; anything smaller means tables were dropped.
        assert!(bytes.len() > 400, "suspiciously small font: {} bytes", bytes.len());
    }

    #[test]
    fn export_order_forces_notdef_first() {
        let mut doc = doc_with_box();
        doc.glyph_order.retain(|n| n != ".notdef");
        assert_eq!(export_order(&doc)[0], ".notdef");
    }

    #[test]
    fn open_contours_are_rejected() {
        let mut doc = doc_with_box();
        doc.glyph_mut("A").unwrap().contours[0].closed = false;
        let err = build_ttf(&doc).unwrap_err();
        assert!(matches!(err, ExportError::OpenContour { .. }));
        assert!(err.to_string().contains('A'));
    }

    #[test]
    fn empty_charset_is_rejected() {
        assert!(matches!(
            build_ttf(&Document::default()).unwrap_err(),
            ExportError::EmptyCmap
        ));
    }

    #[test]
    fn coordinates_land_on_the_f26_grid() {
        assert_eq!(f26dot6(0.0), 0);
        assert_eq!(f26dot6(1.0), 64);
        assert_eq!(f26dot6(-0.5), -32);
    }

    #[test]
    fn table_tags_are_reported() {
        let bytes = build_ttf(&doc_with_box()).unwrap();
        let tags = glyf_table_tags(&bytes);
        for expected in ["cmap", "glyf", "head", "hhea", "hmtx", "loca", "maxp", "name", "post"] {
            assert!(tags.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn unicode_ranges_differ_per_script() {
        let latin = unicode_range_for(Script::Latin);
        let arabic = unicode_range_for(Script::Arabic);
        let hebrew = unicode_range_for(Script::Hebrew);
        // Latin claims Latin Extended-A; the joining scripts do not.
        assert_ne!(latin[0], arabic[0]);
        assert_ne!(arabic, hebrew);
        // The Arabic block lives in the third range word, bit 13.
        assert_eq!(arabic[2] & (1 << 13), 1 << 13);
        assert_eq!(hebrew[2] & (1 << 13), 0);
        // Every script claims Basic Latin.
        for ranges in [latin, arabic, hebrew] {
            assert_eq!(ranges[0] & 1, 1, "Basic Latin bit");
        }
    }

    #[test]
    fn mac_style_follows_the_style_name() {
        assert!(mac_style(true, false) & MacStyle::BOLD != MacStyle::empty());
        assert!(mac_style(false, true) & MacStyle::ITALIC != MacStyle::empty());
    }
}