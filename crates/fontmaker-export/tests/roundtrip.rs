//! End-to-end tests: build a font, read it back, and prove the data survived.
//!
//! These use `read-fonts`, an independent parser, rather than trusting our own
//! writer. `tools/validate_fonts.py` repeats the exercise with fontTools for
//! even wider coverage, but that needs Python.

use fontmaker_core::stroke::{ellipse, rectangle};
use fontmaker_core::{Document, Glyph};
use fontmaker_export::{export, ExportFormat, ttf, woff2};
use font_types::{GlyphId, NameId};
use read_fonts::tables::{
    glyf::Glyph as TtGlyph,
    head::Head,
    hhea::Hhea,
    os2::Os2,
};
use read_fonts::{FontRef, TableProvider};

fn sample() -> Document {
    let mut doc = Document::default();
    doc.meta.family_name = "Test Face".into();
    doc.meta.style_name = "Bold".into();
    doc.meta.designer = "Someone".into();

    let mut a = Glyph::new("A").with_char('A');
    a.contours.push(fontmaker_core::stroke::polygon(
        kurbo::Point::new(200.0, 0.0),
        180.0,
        3,
        std::f64::consts::FRAC_PI_2,
    ));
    a.advance = 420.0;
    a.lsb = 20.0;
    doc.add_glyph(a);

    let mut b = Glyph::new("B").with_char('B');
    b.contours.push(rectangle(20.0, 0.0, 300.0, 700.0));
    b.contours.push(rectangle(80.0, 80.0, 240.0, 620.0));
    b.advance = 400.0;
    doc.add_glyph(b);

    let mut zero = Glyph::new("zero").with_char('0');
    zero.contours.push(ellipse(kurbo::Point::new(180.0, 330.0), 160.0, 330.0));
    zero.advance = 400.0;
    doc.add_glyph(zero);

    let space = doc.glyph_mut("space").expect("space exists");
    space.advance = 260.0;
    space.unicode = vec![' '];
    doc
}

fn font_ref(bytes: &[u8]) -> FontRef<'_> {
    FontRef::new(bytes).expect("read-fonts accepts the font")
}

#[test]
fn ttf_parses_back_with_every_table() {
    let doc = sample();
    let bytes = export(&doc, ExportFormat::Ttf).unwrap();
    let font = font_ref(&bytes);

    let head: Head = font.expect_table().unwrap();
    assert_eq!(head.units_per_em(), 1000);
    assert_eq!(head.magic_number(), 0x5F0F_3CF5);

    let hhea: Hhea = font.expect_table().unwrap();
    assert_eq!(i16::from(hhea.ascender()), 750);
    assert_eq!(i16::from(hhea.descender()), -250);

    let os2: Os2 = font.expect_table().unwrap();
    assert_eq!(os2.sx_height(), Some(500));
    assert_eq!(os2.s_cap_height(), Some(700));

    let hmtx = font.hmtx().unwrap();
    assert_eq!(hmtx.h_metrics().len(), 5);

    let name = font.name().unwrap();
    let family = name
        .name_record()
        .iter()
        .find(|r| r.name_id() == NameId::FAMILY_NAME && r.platform_id() == 3)
        .map(|r| r.string(name.string_data()))
        .expect("family name record")
        .expect("name string decodes");
    assert_eq!(family.to_string(), "Test Face");
}

#[test]
fn cmap_maps_the_expected_codepoints() {
    let doc = sample();
    let bytes = export(&doc, ExportFormat::Ttf).unwrap();
    let font = font_ref(&bytes);
    let cmap = font.cmap().expect("cmap decompiles");

    assert_eq!(cmap.map_codepoint('A'), Some(GlyphId::new(2)), "'A' -> glyph 2");
    assert_eq!(cmap.map_codepoint('B'), Some(GlyphId::new(3)));
    assert_eq!(cmap.map_codepoint('0'), Some(GlyphId::new(4)));
    assert_eq!(cmap.map_codepoint(' '), Some(GlyphId::new(1)));
    assert_eq!(cmap.map_codepoint('\u{1F600}'), None, "no emoji");
}

#[test]
fn advance_widths_survive() {
    let doc = sample();
    let bytes = export(&doc, ExportFormat::Ttf).unwrap();
    let font = font_ref(&bytes);

    let hmtx = font.hmtx().unwrap();
    let metrics = hmtx.h_metrics();
    assert_eq!(metrics.len(), 5);

    // .notdef keeps its default 500-unit advance; space was set to 260.
    let expected: [u16; 5] = [500, 260, 420, 400, 400];
    for (metric, want) in metrics.iter().zip(expected) {
        assert_eq!(metric.advance(), want);
    }
}

#[test]
fn outlines_survive() {
    let doc = sample();
    let bytes = export(&doc, ExportFormat::Ttf).unwrap();
    let font = font_ref(&bytes);

    let glyf = font.glyf().unwrap();
    let loca = font.loca(None).unwrap();

    // .notdef gets the standard hollow box even though the user drew nothing.
    let notdef = loca.get(GlyphId::new(0), &glyf).and_then(|g| g.into_glyph()).unwrap();
    assert_eq!(notdef.number_of_contours(), 2, ".notdef should be a box with a counter");

    let a = loca.get(GlyphId::new(2), &glyf).and_then(|g| g.into_glyph()).unwrap();
    match a {
        TtGlyph::Simple(s) => {
            assert_eq!(s.number_of_contours(), 1);
            // The triangle from the sample document.
            let points: Vec<_> = s.points().collect();
            assert_eq!(points.len(), 3, "triangle has three points");
            assert!(points.iter().all(|p| p.on_curve), "all points on-curve");
        }
        _ => panic!("A should be a simple glyph"),
    }

    let b = loca.get(GlyphId::new(3), &glyf).and_then(|g| g.into_glyph()).unwrap();
    assert_eq!(b.number_of_contours(), 2, "B has a bowl and a counter");

    // Space has no outline at all.
    assert!(loca.get(GlyphId::new(1), &glyf).unwrap().into_glyph().is_none());
}

#[test]
fn zero_glyph_has_an_oval() {
    let doc = sample();
    let bytes = export(&doc, ExportFormat::Ttf).unwrap();
    let font = font_ref(&bytes);
    let glyf = font.glyf().unwrap();
    let loca = font.loca(None).unwrap();

    let zero = loca.get(GlyphId::new(4), &glyf).and_then(|g| g.into_glyph()).unwrap();
    match zero {
        TtGlyph::Simple(s) => {
            // The ellipse in the sample is 320 units wide and 660 tall. There
            // is no bounding box accessor on SimpleGlyph, so measure the points.
            let points: Vec<_> = s.points().collect();
            let xs: Vec<_> = points.iter().map(|p| p.x).collect();
            let ys: Vec<_> = points.iter().map(|p| p.y).collect();
            let width = xs.iter().max().unwrap() - xs.iter().min().unwrap();
            let height = ys.iter().max().unwrap() - ys.iter().min().unwrap();
            assert!((319..=321).contains(&width), "width was {width}");
            assert!((659..=661).contains(&height), "height was {height}");
        }
        _ => panic!("zero should be a simple glyph"),
    }
}

#[test]
fn web_formats_carry_the_same_table_set() {
    let doc = sample();
    let ttf = export(&doc, ExportFormat::Ttf).unwrap();
    let expected = ttf::glyf_table_tags(&ttf);
    assert!(expected.contains(&"glyf".to_string()));

    let woff = export(&doc, ExportFormat::Woff).unwrap();
    let count = u16::from_be_bytes([woff[12], woff[13]]) as usize;
    assert_eq!(count, expected.len(), "WOFF table count");

    let woff2 = export(&doc, ExportFormat::Woff2).unwrap();
    let tables = woff2::read_woff2_header(&woff2).unwrap();
    let mut actual: Vec<String> = tables.keys().cloned().collect();
    actual.sort();
    assert_eq!(actual, expected, "WOFF2 table set");
}

#[test]
fn woff2_declares_its_own_length() {
    let doc = sample();
    let woff2 = export(&doc, ExportFormat::Woff2).unwrap();
    let declared = u32::from_be_bytes([woff2[8], woff2[9], woff2[10], woff2[11]]) as usize;
    assert_eq!(declared, woff2.len(), "length field must match the file");
    assert_eq!(woff2.len() % 4, 0, "file must be 4-byte aligned");
}

#[test]
fn all_three_formats_build() {
    let doc = sample();
    for format in ExportFormat::ALL {
        let bytes = export(&doc, format).expect("the sample font must build");
        assert!(!bytes.is_empty(), "{format:?} produced no bytes");
    }
}