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

    // Draw the positional forms of a dual-joining letter so the GSUB writer
    // has something to say, and add a kern pair for GPOS.
    for name in ["uni0628.init", "uni0628.medi", "uni0628.fina"] {
        let mut g = Glyph::new(name);
        g.contours.push(ellipse(kurbo::Point::new(180.0, 350.0), 120.0, 250.0));
        g.advance = 300.0;
        doc.add_glyph(g);
    }
    doc.set_kern("A", "B", -60.0);

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

fn arabic_document() -> Document {
    let mut doc = Document::default();
    doc.meta.family_name = "FontMaker Arabic".into();
    doc.meta.style_name = "Regular".into();
    fontmaker_script::apply_script(&mut doc, fontmaker_core::Script::Arabic);

    // Give every letter a placeholder outline so the sample font is complete;
    // the validator flags encoded-but-empty glyphs.
    let entries: Vec<(u32, String)> = doc
        .charset
        .iter()
        .map(|e| (e.codepoint, e.glyph.clone()))
        .collect();
    for (i, (_, name)) in entries.iter().enumerate() {
        let width = 200.0 + (i as f64 % 5.0) * 60.0;
        let glyph = doc.glyph_mut(name).expect("charset glyph exists");
        glyph.advance = width + 80.0;
        glyph.contours.push(rectangle(20.0, 0.0, 20.0 + width, 700.0));
    }

    // Distinct outlines for the positional forms of a dual-joining letter so
    // the GSUB features have work to do.
    let widths = [
        ("uni0628.init", 100.0),
        ("uni0628.medi", 200.0),
        ("uni0628.fina", 400.0),
    ];
    for (name, width) in widths {
        let glyph = doc.glyph_mut(name).expect("form glyph exists");
        glyph.contours.push(ellipse(kurbo::Point::new(width / 2.0, 350.0), width / 2.0, 320.0));
        glyph.advance = width + 60.0;
    }
    doc.set_kern("uni0628", "uni0627", -50.0);
    doc
}

fn main() -> std::io::Result<()> {
    let out_dir = Path::new("target/samples");
    std::fs::create_dir_all(out_dir)?;

    for doc in [sample_document(), arabic_document()] {
        for format in ExportFormat::ALL {
            let bytes = export(&doc, format).expect("sample document must export");
            let path = out_dir.join(format.file_name(&doc));
            std::fs::write(&path, &bytes)?;
            println!("wrote {} ({} bytes)", path.display(), bytes.len());
        }
    }
    Ok(())
}