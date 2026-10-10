//! FontMaker font exporters.
//!
//! Binary formats are built here; the project JSON lives in `fontmaker-core`.
//! Every exporter takes a [`fontmaker_core::Document`] and returns bytes, so
//! they can be tested headlessly and reused by the live-preview builder.

pub mod error;
pub mod layout;
pub mod ttf;
pub mod woff;
pub mod woff2;

pub use error::{ExportError, Result};
pub use ttf::build_ttf;
pub use woff::encode_woff;
pub use woff2::encode_woff2;

use fontmaker_core::Document;

/// A file format the exporter can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// TrueType outlines.
    Ttf,
    /// WOFF 1.0, the original web font format.
    Woff,
    /// WOFF2, the current web font format.
    Woff2,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] =
        [ExportFormat::Ttf, ExportFormat::Woff, ExportFormat::Woff2];

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Ttf => "ttf",
            ExportFormat::Woff => "woff",
            ExportFormat::Woff2 => "woff2",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ExportFormat::Ttf => "TrueType font (.ttf)",
            ExportFormat::Woff => "WOFF font (.woff)",
            ExportFormat::Woff2 => "WOFF2 font (.woff2)",
        }
    }

    /// Suggested file name for this document, e.g. `MyFont-Regular.woff2`.
    pub fn file_name(self, doc: &Document) -> String {
        format!("{}.{}", doc.meta.postscript_name(), self.extension())
    }
}

/// Export a document in the given binary format.
///
/// Web formats are produced by building a TrueType font first and then wrapping
/// it, which guarantees the three outputs always describe the same outlines.
pub fn export(doc: &Document, format: ExportFormat) -> Result<Vec<u8>> {
    match format {
        ExportFormat::Ttf => build_ttf(doc),
        ExportFormat::Woff | ExportFormat::Woff2 => {
            let ttf = build_ttf(doc)?;
            match format {
                ExportFormat::Woff => encode_woff(&ttf),
                _ => encode_woff2(&ttf),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontmaker_core::stroke::rectangle;
    use fontmaker_core::Glyph;

    fn sample() -> Document {
        let mut doc = Document::default();
        let mut g = Glyph::new("A").with_char('A');
        g.contours.push(rectangle(0.0, 0.0, 300.0, 700.0));
        g.advance = 400.0;
        doc.add_glyph(g);

        let mut b = Glyph::new("B").with_char('B');
        b.contours.push(rectangle(0.0, 0.0, 280.0, 700.0));
        b.advance = 380.0;
        doc.add_glyph(b);
        doc
    }

    #[test]
    fn all_three_formats_produce_output() {
        let doc = sample();
        for format in ExportFormat::ALL {
            let bytes = export(&doc, format).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert!(bytes.len() > 100, "{format:?} produced only {} bytes", bytes.len());
        }
    }

    #[test]
    fn file_names_use_the_postscript_name() {
        let mut doc = sample();
        doc.meta.family_name = "Test Face".into();
        doc.meta.style_name = "Regular".into();
        assert_eq!(
            ExportFormat::Woff2.file_name(&doc),
            "TestFace-Regular.woff2"
        );
    }

    #[test]
    fn woff2_wraps_a_ttf() {
        let doc = sample();
        let ttf = export(&doc, ExportFormat::Ttf).unwrap();
        let woff2 = export(&doc, ExportFormat::Woff2).unwrap();
        // The WOFF2 must preserve the table set of the font it wraps.
        let expected = ttf::glyf_table_tags(&ttf);
        let actual = woff2::read_woff2_header(&woff2).unwrap();
        assert_eq!(expected, actual.keys().cloned().collect::<Vec<_>>());
    }
}