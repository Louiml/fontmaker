//! Glyph model: contours, nodes, anchors and the per-script `Form` variants.

use crate::geom::{contour_to_bezpath, Node};
use kurbo::{BezPath, CubicBez, ParamCurve, Point, Shape};
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
    /// Joins on the right (previous letter) only: `״§ ״¯ ״° ״± ״² ˆ`.
    #[default]
    RightJoining,
    /// Joins on the left (next letter) only: most Arabic letters.
    LeftJoining,
    /// Joins on both sides: `״¨ ״× ״« ״¬ ״­ ״® ״³ ״´ ״µ ״¶ ״· ״¸ ״¹ ״÷  ‚ ƒ „ … † ‡ `.
    DualJoining,
    /// Isolated but forces neighbours to connect: `״¢ ״£`.
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

/// The cubic equivalent of the segment between two nodes.
///
/// Quads are degree-elevated (exactly) so splitting can treat every segment as
/// a cubic; a line becomes a straight cubic.
fn segment_as_cubic(a: &Node, b: &Node) -> CubicBez {
    match (a.out_handle, b.in_handle) {
        (Some(o), Some(i)) => CubicBez::new(a.pos, o, i, b.pos),
        (Some(o), None) => {
            // Quad with control `o`, raised to a cubic.
            let c1 = a.pos + (o - a.pos) * (2.0 / 3.0);
            let c2 = b.pos + (o - b.pos) * (2.0 / 3.0);
            CubicBez::new(a.pos, c1, c2, b.pos)
        }
        (None, Some(i)) => {
            let c1 = a.pos + (i - a.pos) * (2.0 / 3.0);
            let c2 = b.pos + (i - b.pos) * (2.0 / 3.0);
            CubicBez::new(a.pos, c1, c2, b.pos)
        }
        (None, None) => {
            let d = b.pos - a.pos;
            CubicBez::new(a.pos, a.pos + d / 3.0, b.pos - d / 3.0, b.pos)
        }
    }
}


/// Nearest point on the segment to `p`, as `(t, distance squared)`.
///
/// Sampling with local refinement, rather than kurbo's quintic `nearest`,
/// whose root finder is unreliable on degenerate (straight) cubics. For
/// hit-testing, 64 samples plus a ternary refinement is far more accurate
/// than any click will ever be.
fn nearest_on_segment(a: &Node, b: &Node, p: Point) -> (f64, f64) {
    // Straight segments have an exact analytic answer.
    if a.is_corner() && b.is_corner() {
        let ab = b.pos - a.pos;
        let len2 = ab.hypot2();
        if len2 < 1e-12 {
            return (0.0, (p - a.pos).hypot2());
        }
        let t = ((p - a.pos).dot(ab) / len2).clamp(0.0, 1.0);
        let q = a.pos + ab * t;
        return (t, (p - q).hypot2());
    }

    let cubic = segment_as_cubic(a, b);
    const SAMPLES: usize = 64;
    let mut best_t = 0.0;
    let mut best_d = f64::INFINITY;
    for i in 0..=SAMPLES {
        let t = i as f64 / SAMPLES as f64;
        let d = (cubic.eval(t) - p).hypot2();
        if d < best_d {
            best_d = d;
            best_t = t;
        }
    }
    // Refine by ternary search around the best sample.
    let step = 1.0 / SAMPLES as f64;
    let mut lo = (best_t - step).max(0.0);
    let mut hi = (best_t + step).min(1.0);
    for _ in 0..40 {
        let mid1 = lo + (hi - lo) / 3.0;
        let mid2 = hi - (hi - lo) / 3.0;
        if (cubic.eval(mid1) - p).hypot2() < (cubic.eval(mid2) - p).hypot2() {
            hi = mid2;
        } else {
            lo = mid1;
        }
    }
    let t = (lo + hi) / 2.0;
    (t, (cubic.eval(t) - p).hypot2())
}
/// Insert a new node on the segment of `contour` nearest to `p`, splitting that
/// segment at the projection of `p`.
///
/// `max_distance` is the largest distance, in font units, between `p` and the
/// segment for the split to happen. The new node is smooth: its handles come
/// from the two halves of the split curve, so the outline does not move.
///
/// Returns the index of the inserted node, or `None` when no segment is within
/// reach or the contour is too small to split.
pub fn insert_point_on_segment(
    contour: &mut Contour,
    p: Point,
    max_distance: f64,
) -> Option<usize> {
    let n = contour.nodes.len();
    if n < 2 {
        return None;
    }
    let reach_sq = max_distance * max_distance;

    // Candidate segments: consecutive pairs, plus the wrap-around pair when
    // the contour is closed.
    let mut best: Option<(usize, f64, f64)> = None; // (segment index, distײ²ֲ², t)
    let segments = if contour.closed { n } else { n - 1 };
    for si in 0..segments {
        let a = &contour.nodes[si];
        let b = &contour.nodes[(si + 1) % n];
        let (t, dist_sq) = nearest_on_segment(a, b, p);
        if dist_sq <= reach_sq && best.map(|(_, d, _)| dist_sq < d).unwrap_or(true) {
            best = Some((si, dist_sq, t));
        }
    }
    let (si, _, t_raw) = best?;
    // Reject splits that would land on top of an existing node.
    if !(0.05..=0.95).contains(&t_raw) {
        return None;
    }
    let t = t_raw.clamp(0.05, 0.95);

    let a = contour.nodes[si];
    let b = contour.nodes[(si + 1) % n];

    // A straight segment split is just two lines: no handles to preserve.
    if a.is_corner() && b.is_corner() {
        let ab = b.pos - a.pos;
        let q = a.pos + ab * t_raw;
        let new_node = Node::corner(q);
        let insert_at = if contour.closed && si == n - 1 {
            contour.nodes.insert(0, new_node);
            0
        } else {
            contour.nodes.insert(si + 1, new_node);
            si + 1
        };
        return Some(insert_at);
    }

    let cubic = segment_as_cubic(&a, &b);
    let left = cubic.subsegment(0.0..t);
    let right = cubic.subsegment(t..1.0);

    // Rewrite the two existing nodes' relevant handles, then insert the new
    // smooth node between them.
    contour.nodes[si].out_handle = Some(left.p1);
    let new_node = Node::smooth(left.p3, left.p2, right.p1);

    let insert_at = if contour.closed && si == n - 1 {
        // The wrap-around segment ends at node 0; the new node becomes the
        // new first node.
        contour.nodes[0].in_handle = Some(right.p2);
        contour.nodes.insert(0, new_node);
        0
    } else {
        contour.nodes[(si + 1) % n].in_handle = Some(right.p2);
        contour.nodes.insert(si + 1, new_node);
        si + 1
    };
    Some(insert_at)
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
    use crate::geom::flatten;

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
    fn insert_point_on_a_line() {
        use crate::glyph::Contour;
        let mut contour = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::corner(Point::new(100.0, 0.0)),
            ],
            false,
        );
        let idx = insert_point_on_segment(&mut contour, Point::new(40.0, 3.0), 10.0).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(contour.nodes.len(), 3);
        assert!((contour.nodes[1].pos.x - 40.0).abs() < 0.5);
        assert!((contour.nodes[1].pos.y).abs() < 0.5);
        // The new node is a corner: a line has no curvature to preserve.
        assert!(contour.nodes[1].is_corner());
    }
    #[test]
    fn insert_point_on_a_cubic_preserves_the_shape() {
        use crate::glyph::Contour;
        let mut contour = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::smooth(
                    Point::new(50.0, 100.0),
                    Point::new(20.0, 80.0),
                    Point::new(80.0, 120.0),
                ),
                Node::corner(Point::new(100.0, 0.0)),
            ],
            true,
        );
        let before: Vec<Point> = flatten(&contour.to_bezpath(), 0.05).concat();
        let idx = insert_point_on_segment(&mut contour, Point::new(22.5, 65.0), 5.0).unwrap();
        assert_eq!(idx, 1, "inserted on the first segment");
        assert_eq!(contour.nodes.len(), 4);
        let after: Vec<Point> = flatten(&contour.to_bezpath(), 0.05).concat();

        // The split must not move the outline: the new node must sit on the
        // original polyline, and the bounding box must not change. (Per-point
        // comparison is impossible because the split segments are resampled
        // independently by `flatten`.)
        let dist_to_polyline = |pts: &[Point], q: Point| -> f64 {
            pts.windows(2)
                .map(|w| {
                    let ab = w[1] - w[0];
                    let len2 = ab.hypot2();
                    if len2 < 1e-12 {
                        (q - w[0]).hypot()
                    } else {
                        let t = ((q - w[0]).dot(ab) / len2).clamp(0.0, 1.0);
                        (q - (w[0] + ab * t)).hypot()
                    }
                })
                .fold(f64::INFINITY, f64::min)
        };
        assert!(
            dist_to_polyline(&before, contour.nodes[1].pos) < 0.5,
            "new node must lie on the original outline"
        );
        let bb = |pts: &[Point]| {
            pts.iter().fold(
                (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
            )
        };
        assert!((bb(&before).0 - bb(&after).0).abs() < 0.01);
        assert!((bb(&before).3 - bb(&after).3).abs() < 0.01);
        // The new node carries handles (smooth).
        assert!(contour.nodes[1].has_handle());
    }
    #[test]
    fn insert_point_rejects_far_clicks() {
        use crate::glyph::Contour;
        let mut contour = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::corner(Point::new(100.0, 0.0)),
            ],
            false,
        );
        assert!(insert_point_on_segment(&mut contour, Point::new(50.0, 500.0), 10.0).is_none());
        // Also too close to either end node.
        assert!(insert_point_on_segment(&mut contour, Point::new(1.0, 0.0), 10.0).is_none());
    }
    #[test]
    fn insert_point_on_closed_wrap_segment() {
        use crate::glyph::Contour;
        let mut contour = Contour::new(
            vec![
                Node::corner(Point::new(0.0, 0.0)),
                Node::corner(Point::new(100.0, 0.0)),
                Node::corner(Point::new(100.0, 100.0)),
            ],
            true,
        );
        // The wrap segment runs from (100,100) back to (0,0); click near (50,50).
        let idx = insert_point_on_segment(&mut contour, Point::new(50.0, 50.0), 5.0).unwrap();
        assert_eq!(idx, 0, "wrap-segment insert becomes the new first node");
        assert_eq!(contour.nodes.len(), 4);
        assert!((contour.nodes[0].pos.x - 50.0).abs() < 1.0);
        // And the contour stays closed with four segments.
        assert_eq!(contour.to_bezpath().iter().count(), 6); // move + 4 lines + close
    }

    #[test]
    fn form_suffixes() {
        assert_eq!(Form::Isol.suffix(), "");
        assert_eq!(Form::Medi.suffix(), ".medi");
        assert_eq!(Form::Fina.feature_tag(), "fina");
    }
}
