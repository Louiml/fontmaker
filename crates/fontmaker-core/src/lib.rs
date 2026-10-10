//! FontMaker core: the typeface document, its geometry and its edit history.
//!
//! This crate has no GUI dependency so it can be unit tested headlessly and
//! reused by the exporter, the shaping bridge and the app.

pub mod document;
pub mod geom;
pub mod glyph;
pub mod history;
pub mod stroke;
pub mod variation;

pub use document::{CharsetEntry, Document, FontMeta, Script, VerticalMetrics};
pub use geom::{contains_point, contour_to_bezpath, flatten, mirror_x, mirror_y, transform_nodes, Node};
pub use glyph::{insert_point_on_segment, Component, Contour, Form, Glyph, JoiningType};
pub use history::{Edit, History};
pub use stroke::{
    ellipse, freehand_to_bezpath, path_to_contour, polygon, rectangle, stroke_to_path, StrokeCap,
    StrokeJoin, WidthProfile,
};
pub use variation::{Axis, GlyphDeltas, GlyphDeltaSet, Location, LocationKey};

/// Bump when the JSON layout changes in a way that needs migration.
pub const DOCUMENT_VERSION: u32 = 1;

/// Wrap the document in the on-disk envelope.
pub mod project {
    use super::{Document, DOCUMENT_VERSION};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ProjectFile {
        pub format_version: u32,
        #[serde(default)]
        pub generator: String,
        pub document: Document,
    }

    impl ProjectFile {
        pub fn new(document: Document) -> Self {
            Self {
                format_version: DOCUMENT_VERSION,
                generator: format!("FontMaker {}", env!("CARGO_PKG_VERSION")),
                document,
            }
        }
    }

    /// Serialise a document to pretty JSON.
    pub fn to_json(document: &Document) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&ProjectFile::new(document.clone()))
    }

    /// Parse a document from JSON, rejecting unknown future formats.
    ///
    /// The version is checked before the document body is parsed so that a file
    /// from a newer FontMaker gets a helpful message instead of a confusing
    /// deserialisation error about fields this build has never heard of.
    pub fn from_json(text: &str) -> Result<Document, String> {
        let probe: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("Not a valid FontMaker project: {e}"))?;
        let version = probe
            .get("format_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "This file has no format_version and is not a FontMaker project.".to_string())?;
        if version > DOCUMENT_VERSION as u64 {
            return Err(format!(
                "This project was made with a newer FontMaker (format v{version}). Please update."
            ));
        }
        let file: ProjectFile =
            serde_json::from_value(probe).map_err(|e| format!("Corrupt project file: {e}"))?;
        Ok(file.document)
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;

    #[test]
    fn project_json_roundtrip() {
        let mut doc = Document {
            script: Script::Arabic,
            ..Default::default()
        };
        let mut g = Glyph::new("uni0627").with_char('\u{0627}');
        g.form = Form::Isol;
        g.contours.push(crate::stroke::rectangle(0.0, 0.0, 100.0, 700.0));
        g.anchors.insert("entry".into(), kurbo::Point::new(0.0, 0.0));
        doc.add_glyph(g);

        let json = project::to_json(&doc).unwrap();
        assert!(json.contains("\"format_version\": 1"));
        let back = project::from_json(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn future_format_is_rejected() {
        let json = r#"{"format_version": 999, "document": null}"#;
        let err = project::from_json(json).unwrap_err();
        assert!(err.contains("newer FontMaker"));
    }
}