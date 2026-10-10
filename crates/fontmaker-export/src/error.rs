//! Errors produced by the exporters.

use std::fmt;

/// Result alias for export operations.
pub type Result<T> = std::result::Result<T, ExportError>;

/// Everything that can go wrong while writing a font file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// The document contains no glyphs at all.
    NoGlyphs,
    /// The font encodes no characters, so no `cmap` can be written.
    EmptyCmap,
    /// A contour was left open. TrueType and CFF both require closed contours.
    OpenContour { glyph: String },
    /// A glyph could not be converted into font format.
    MalformedGlyph { glyph: String, reason: String },
    /// The input file is not a font FontMaker understands.
    MalformedSfnt(String),
    /// The output would exceed what the format can address.
    TooLarge(String),
    /// An unsupported export format was requested.
    UnsupportedFormat(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoGlyphs => write!(f, "This font has no glyphs to export."),
            ExportError::EmptyCmap => {
                write!(f, "This font has no encoded characters yet. Assign a character to at least one glyph before exporting.")
            }
            ExportError::OpenContour { glyph } => write!(
                f,
                "The glyph '{glyph}' has an open contour. Close every contour before exporting."
            ),
            ExportError::MalformedGlyph { glyph, reason } => {
                write!(f, "The glyph '{glyph}' could not be converted: {reason}")
            }
            ExportError::MalformedSfnt(reason) => write!(f, "Malformed font file: {reason}"),
            ExportError::TooLarge(reason) => write!(f, "The font is too large to export: {reason}"),
            ExportError::UnsupportedFormat(format) => {
                write!(f, "Exporting to {format} is not supported yet.")
            }
        }
    }
}

impl std::error::Error for ExportError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_the_glyph() {
        let err = ExportError::OpenContour { glyph: "B".into() };
        assert!(err.to_string().contains("'B'"));
    }

    #[test]
    fn empty_cmap_suggests_a_fix() {
        assert!(ExportError::EmptyCmap.to_string().contains("character"));
    }
}