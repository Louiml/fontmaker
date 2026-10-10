//! Writes sample fonts in every supported format so external tools can check
//! them. Run with `cargo run -p fontmaker-export --example write_samples`.

use std::path::Path;

use fontmaker_core::stroke::{ellipse, rectangle};
use fontmaker_core::{Document, Glyph};
use fontmaker_export::{export, ExportFormat};

fn sample_document() -> Document {
    let mut doc = Document::default();
    doc.meta.family_name = "FontMaker Test".into();
    doc.meta.style_name = "Regular".into();
    doc.meta.designer = "FontMaker".into();
    doc.meta.version = "1.0".into();

    // A is a triangle, B a box, lowercase a an ellipse, and 0 a rounded bowl:
    // enough geometry to prove outlines, metrics and the cmap all work.
    let mut a = Glyph::new("A").with_char('A');
    a.contours.push(
        fontmaker_core::stroke::polygon(kurbo::Point::new(200.0, 0.0), 180.0, 3, std::f64::consts::FRAC_PI_2),
    );
    a.advance = 420.0;
    doc.add_glyph(a);

    let mut b = Glyph::new("B").with_char('B');
    b.contours.push(rectangle(20.0, 0.0, 300.0, 700.0));
    b.contours.push(rectangle(80.0, 80.0, 240.0, 620.0));
    b.advance = 400.0;
    doc.add_glyph(b);

    let mut a_lower = Glyph::new("a").with_char('a');
    a_lower.contours.push(ellipse(kurbo::Point::new(180.0, 250.0), 150.0, 240.0));
    a_lower.contours.push(rectangle(150.0, 0.0, 210.0, 250.0));
    a_lower.advance = 380.0;
    doc.add_glyph(a_lower);

    let mut zero = Glyph::new("zero").with_char('0');
    zero.contours.push(ellipse(kurbo::Point::new(180.0, 330.0), 160.0, 330.0));
    zero.advance = 400.0;
    doc.add_glyph(zero);

    let mut space = doc.glyph_mut("space").expect("space exists");
    space.advance = 260.0;
    space.unicode = vec![' '];
    doc.charset.push(fontmaker_core::CharsetEntry {
        codepoint: 0x20,
        glyph: "space".into(),
        joining: Default::default(),
        transparent: true,
    });

    doc
}

fn main() -> std::io::Result<()> {
    let out_dir = Path::new("target/samples");
    std::fs::create_dir_all(out_dir)?;

    let doc = sample_document();
    for format in ExportFormat::ALL {
        let bytes = export(&doc, format).expect("sample document must export");
        let path = out_dir.join(format.file_name(&doc));
        std::fs::write(&path, &bytes)?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}