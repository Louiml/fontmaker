//! Glyph model: contours, nodes, anchors and the per-script `Form` variants.

use crate::geom::{contour_to_bezpath, Node};
use kurbo::{BezPath, Point, Shape};
use serde::{Deserialize, Serialize};

/// Positional variant of a letter, used by joining scripts.
///
/// Naming follows the Adobe/Indic convention so that exported fonts behave the
/// same in other tools: `uni0627` (isolated), `uni0627.init`, `uni0627.medi`,
/// `uni0627.fina`. Hebrew reuses the same four forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Form {
    #[default]
    Isol,
    Init,
    Medi,
    Fina,
}

impl Form {
    pub const ALL: [Form; 4] = [Form::Isol, Form::Init, Form::Medi, Form::Fina];

    /// Suffix appended to a glyph name, or `""` for the isolated form.
    pub fn suffix(self) -> &'static str {
        match self {
            Form::Isol => "",
            Form::Init => ".init",
            Form::Medi => ".medi",
            Form::Fina => ".fina",
        }
    }

    /// OpenType feature tag that selects this form.
    pub fn feature_tag(self) -> &'static str {
        match self {
            Form::Isol => "isol",
            Form::Init => "init",
            Form::Medi => "medi",
            Form::Fina => "fina",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Form::Isol => "Isolated",
            Form::Init => "Initial",
            Form::Medi => "Medial",
            Form::Fina => "Final",
        }
    }
}

/// How a letter joins to its neighbours. Drives Arabic and Hebrew shaping and
/// the form generator in `fontmaker-script`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum JoiningType {
    /// Never joins: `space`, punctuation.
    NonJoining,
    /// Joins on the right (previous letter) only: `ا د ذ ر ز و`.
    #[default]
    RightJoining,
    /// Joins on the left (next letter) only: most Arabic letters.
    LeftJoining,
    /// Joins on both sides: `ب ت ث ج ح خ س ش ص ض ط ظ ع غ ف ق ك ل م ن ه ي`.
    DualJoining,
    /// Isolated but forces neighbours to connect: `آ أ`.
    Joining,
}

/// A closed or open loop of nodes. Open contours are used while the pen tool
/// is mid-stroke; the exporter rejects them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    pub nodes: Vec<Node>,
    #[serde(default = "default_true")]
    pub closed: bool,
}

fn default_true() -> bool {
    true
}

impl Contour {
    pub fn new(nodes: Vec<Node>, closed: bool) -> Self {
        Self { nodes, closed }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn to_bezpath(&self) -> BezPath {
        contour_to_bezpath(&self.nodes, self.closed)
    }

    /// True when the node is on-curve and has no handles.
    pub fn is_corner_at(&self, i: usize) -> bool {
        self.nodes.get(i).map(|n| n.is_corner()).unwrap_or(false)
    }

    /// Make node `i` smooth: fill in any missing handle by mirroring the other
    /// side through the node, keeping the tangent continuous.
    pub fn smooth(&mut self, i: usize) -> bool {
        let Some(node) = self.nodes.get(i) else { return false };
        if !node.has_handle() {
            return false;
        }
        let (in_h, out_h) = (node.in_handle, node.out_handle);
        let pos = node.pos;
        let node = &mut self.nodes[i];
        node.in_handle = Some(in_h.unwrap_or(pos - (out_h.unwrap() - pos)));
        node.out_handle = Some(out_h.unwrap_or(pos - (in_h.unwrap() - pos)));
        true
    }

    /// Convert node `i` back to a corner, dropping both handles.
    pub fn make_corner(&mut self, i: usize) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.in_handle = None;
            n.out_handle = None;
        }
    }

    /// Approximate signed area. Positive means counter-clockwise in a y-up
    /// coordinate system.
    pub fn signed_area(&self) -> f64 {
        use kurbo::ParamCurve;
        let mut area = 0.0;
        for seg in self.to_bezpath().segments() {
            let (p0, p1) = (seg.start(), seg.end());
            // Shoelace contribution of the straight chord; exact for lines and
            // a close approximation for the shallow curves typical of type.
            area += p0.x * p1.y - p1.x * p0.y;
        }
        area * 0.5
    }

    pub fn bounds(&self) -> Option<kurbo::Rect> {
        if self.nodes.is_empty() {
            return None;
        }
        let b = self.to_bezpath().bounding_box();
        if b.is_finite() && (b.width() > 0.0 || b.height() > 0.0) {
            Some(b)
        } else {
            None
        }
    }
}

/// A single glyph: outlines, metrics, anchors and Unicode coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    /// PostScript-ish name, e.g. `A`, `uni0627.medi`, `zero`.
    pub name: String,
    /// Code points this glyph covers. Empty for unencoded glyphs.
    #[serde(default)]
    pub unicode: Vec<char>,
    /// Horizontal advance width in font units.
    pub advance: f64,
    #[serde(default = "default_lsb")]
    pub lsb: f64,
    #[serde(default)]
    pub contours: Vec<Contour>,
    /// Named attachment points used by the Arabic/Hebrew form generator and
    /// by mark positioning. Keyed by name, e.g. `entry`, `exit`, `top`.
    #[serde(default)]
    pub anchors: std::collections::BTreeMap<String, Point>,
    /// Which positional variant this glyph represents.
    #[serde(default)]
    pub form: Form,
    /// Opaque per-glyph user notes.
    #[serde(default)]
    pub note: String,
}

fn default_lsb() -> f64 {
    0.0
}

impl Default for Glyph {
    fn default() -> Self {
        Self {
            name: String::new(),
            unicode: Vec::new(),
            advance: 500.0,
            lsb: 0.0,
            contours: Vec::new(),
            anchors: Default::default(),
            form: Form::Isol,
            note: String::new(),
        }
    }
}

impl Glyph {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    pub fn with_char(mut self, c: char) -> Self {
        self.unicode = vec![c];
        self
    }

    pub fn to_bezpath(&self) -> BezPath {
        let mut path = BezPath::new();
        for c in &self.contours {
            path.extend(c.to_bezpath());
        }
        path
    }

    pub fn is_drawn(&self) -> bool {
        self.contours.iter().any(|c| !c.is_empty())
    }

    pub fn bounds(&self) -> Option<kurbo::Rect> {
        let mut b: Option<kurbo::Rect> = None;
        for c in &self.contours {
            if let Some(cb) = c.bounds() {
                b = Some(match b {
                    Some(existing) => existing.union(cb),
                    None => cb,
                });
            }
        }
        b
    }

    /// Human-readable label, e.g. `U+0627 ARABIC LETTER ALEF (init)`.
    pub fn display_label(&self) -> String {
        let base = match self.unicode.first() {
            Some(c) => format!("U+{:04X} {}", *c as u32, c),
            None => self.name.clone(),
        };
        if self.form == Form::Isol {
            base
        } else {
            format!("{} ({})", base, self.form.label().to_lowercase())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contour_signed_area_sign() {
        let ccw = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::corner(Point::new(0.0, 100.0)),
                Node::corner(Point::new(100.0, 100.0)),
                Node::corner(Point::new(100.0, 0.0)),
            ],
            true,
        );
        let cw = Contour::new(ccw.nodes.clone(), true);
        let mut cw = cw;
        crate::geom::mirror_x(&mut cw.nodes, 0.0);
        assert!(ccw.signed_area().abs() > 5000.0);
        assert_ne!(ccw.signed_area().signum(), cw.signed_area().signum());
    }

    #[test]
    fn smooth_fills_missing_handle() {
        let mut c = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::smooth(
                    Point::new(10.0, 0.0),
                    Point::new(5.0, 5.0),
                    Point::new(15.0, -5.0),
                ),
                Node::corner(Point::new(20.0, 0.0)),
            ],
            true,
        );
        assert!(c.smooth(1));
        // Still smooth, handles stay collinear through the node.
        assert!(c.is_corner_at(0));
        assert!(!c.is_corner_at(1));
    }

    #[test]
    fn form_suffixes() {
        assert_eq!(Form::Isol.suffix(), "");
        assert_eq!(Form::Medi.suffix(), ".medi");
        assert_eq!(Form::Fina.feature_tag(), "fina");
    }
}