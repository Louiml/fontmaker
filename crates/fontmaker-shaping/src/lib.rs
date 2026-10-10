//! Shaping bridge for the FontMaker live text preview.
//!
//! Builds a real font in memory, then shapes text with allsorts (a HarfBuzz-
//! class shaper). This validates the whole export pipeline: if GSUB or GPOS
//! were written wrong, these tests fail.

#[cfg(test)]
mod tests {
    use allsorts::binary::read::ReadScope;
    use allsorts::font_data::FontData;
    use allsorts::gsub::{FeatureMask, FeatureMaskExt};
    use allsorts::tables::FontTableProvider;
    use allsorts::{tag, Font};
    use fontmaker_core::stroke::rectangle;
    use fontmaker_core::{Document, Glyph, Script};
    use fontmaker_script as scripts;

    /// Build a TTF from a document, in memory.
    fn build(doc: &Document) -> Vec<u8> {
        fontmaker_export::build_ttf(doc).expect("document must export")
    }

    fn shape(bytes: &[u8], text: &str, script_tag: u32) -> Vec<allsorts::gpos::Info> {
        let scope = ReadScope::new(bytes);
        let font_file = scope.read::<FontData<'_>>().expect("font parses");
        let provider = font_file.table_provider(0).expect("font index");
        let mut font = Font::new(provider).expect("font loads");
        let glyphs =
            font.map_glyphs(text, script_tag, allsorts::font::MatchingPresentation::NotRequired);
        font.shape(
            glyphs,
            script_tag,
            None,
            FeatureMask::default_mask(),
            &[],
            None,
            true,
        )
        .expect("shaping succeeds")
    }

    #[test]
    fn arabic_letters_join_into_positional_forms() {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Arabic);

        // Give the isolated, initial, medial and final forms of ب distinct
        // outline widths so we can tell which glyph the shaper picked.
        let widths = [
            ("uni0628", 300.0),
            ("uni0628.init", 100.0),
            ("uni0628.medi", 200.0),
            ("uni0628.fina", 400.0),
        ];
        for (name, width) in widths {
            let glyph = doc.glyph_mut(name).expect("form glyph exists");
            glyph.contours.push(rectangle(0.0, 0.0, width, 700.0));
        }
        let order = doc.glyph_order.clone();
        let bytes = build(&doc);

        // Three dual-joining letters in a row must become init + medi + fina.
        let shaped = shape(&bytes, "\u{0628}\u{0628}\u{0628}", tag::ARAB);
        assert_eq!(shaped.len(), 3, "one glyph per input letter");

        let gid = |name: &str| {
            order
                .iter()
                .position(|n| n == name)
                .expect("glyph in order") as u16
        };
        let mut got: Vec<u16> = shaped.iter().map(|info| info.glyph.glyph_index).collect();
        got.sort();
        let mut expected = vec![
            gid("uni0628.init"),
            gid("uni0628.medi"),
            gid("uni0628.fina"),
        ];
        expected.sort();
        assert_eq!(got, expected, "ببب must shape into the three joining forms");
    }

    #[test]
    fn isolated_arabic_stays_isolated() {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Arabic);
        let glyph = doc.glyph_mut("uni0628").expect("base glyph");
        glyph.contours.push(rectangle(0.0, 0.0, 300.0, 700.0));
        let order = doc.glyph_order.clone();
        let bytes = build(&doc);

        // A hamza before the letter blocks joining on both sides.
        let shaped = shape(&bytes, "\u{0621}\u{0628}", tag::ARAB);
        assert_eq!(shaped.len(), 2);
        let gid = |name: &str| order.iter().position(|n| n == name).unwrap() as u16;
        assert_eq!(
            shaped[1].glyph.glyph_index,
            gid("uni0628"),
            "ب after a non-joiner stays in its isolated form"
        );
    }

    #[test]
    fn kerning_reaches_the_shaper() {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Latin);
        // Draw A and V so they are real glyphs.
        for (name, width) in [("A", 300.0), ("V", 280.0)] {
            let glyph = doc.glyph_mut(name).expect("glyph exists");
            glyph.contours.push(rectangle(0.0, 0.0, width, 700.0));
        }
        doc.set_kern("A", "V", -120.0);
        let bytes = build(&doc);

        let shaped = shape(&bytes, "AV", tag::LATN);
        assert_eq!(shaped.len(), 2);
        assert_eq!(
            shaped[0].kerning, -120,
            "the GPOS kern pair must apply between A and V"
        );
        assert_eq!(shaped[1].kerning, 0, "no kerning on the trailing glyph");
    }

    #[test]
    fn latin_shapes_unchanged_without_kerning() {
        // A default document has no charset, so apply Latin to get a cmap.
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Latin);
        let bytes = build(&doc);
        let shaped = shape(&bytes, "AB", tag::LATN);
        assert_eq!(shaped.len(), 2);
        // Neither A nor B has outlines yet, but both are encoded; the shaper
        // must map them to their real glyph ids, not .notdef.
        let order = &doc.glyph_order;
        let gid = |name: &str| order.iter().position(|n| n == name).unwrap() as u16;
        assert_eq!(shaped[0].glyph.glyph_index, gid("A"));
        assert_eq!(shaped[1].glyph.glyph_index, gid("B"));
    }

    /// The glyph model round-trips through shaping without the exporter being
    /// involved — a smoke test for `Glyph` construction.
    #[test]
    fn glyph_model_smoke() {
        let mut g = Glyph::new("test").with_char('t');
        g.contours.push(rectangle(0.0, 0.0, 100.0, 100.0));
        assert!(g.is_drawn());
        assert_eq!(g.bounds().unwrap().width(), 100.0);
    }
}
