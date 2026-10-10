//! Geometry helpers shared by the editor, the exporter and the renderer.
//!
//! FontMaker stores glyphs as editable nodes (see [`crate::glyph`]) and converts
//! them to [`kurbo::BezPath`] on demand for rendering, hit-testing and export.

use kurbo::{
    Affine, BezPath, CubicBez, ParamCurve, ParamCurveExtrema, PathEl, Point, Rect, Shape, Vec2,
};
use serde::{Deserialize, Serialize};

/// Flattening tolerance in font units. Chosen so that exported outlines stay
/// well under the 1/64 em grid rounding of the TrueType rasteriser.
pub const FLATTEN_TOLERANCE: f64 = 0.1;

/// Maximum number of line segments used when flattening one cubic segment.
const MAX_FLATTEN_STEPS: usize = 64;

/// A single editable node of a [`Contour`](crate::glyph::Contour).
///
/// A node with no handles is a corner. A node with both handles is a smooth
/// point whose handles stay collinear through [`Contour::smooth`](crate::glyph::Contour::smooth).
/// This mirrors the pen-tool model used by Illustrator and Inkscape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub pos: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_handle: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_handle: Option<Point>,
}

impl Node {
    pub fn corner(pos: Point) -> Self {
        Self { pos, in_handle: None, out_handle: None }
    }

    pub fn smooth(pos: Point, in_handle: Point, out_handle: Point) -> Self {
        Self { pos, in_handle: Some(in_handle), out_handle: Some(out_handle) }
    }

    pub fn is_corner(&self) -> bool {
        self.in_handle.is_none() && self.out_handle.is_none()
    }

    pub fn has_handle(&self) -> bool {
        self.in_handle.is_some() || self.out_handle.is_some()
    }

    /// Move the node and its handles by the same delta.
    pub fn move_to(&mut self, delta: Vec2) {
        self.pos += delta;
        if let Some(h) = &mut self.in_handle {
            *h += delta;
        }
        if let Some(h) = &mut self.out_handle {
            *h += delta;
        }
    }

    /// Move only the position, leaving handles in absolute place.
    pub fn move_pos_only(&mut self, pos: Point) {
        self.pos = pos;
    }
}

/// Convert a contour's nodes into a [`BezPath`].
pub fn contour_to_bezpath(nodes: &[Node], close: bool) -> BezPath {
    let mut path = BezPath::new();
    if nodes.is_empty() {
        return path;
    }
    path.move_to(nodes[0].pos);
    let n = nodes.len();
    let last = if close { n } else { n - 1 };
    for i in 0..last {
        let a = nodes[i];
        let b = nodes[(i + 1) % n];
        match (a.out_handle, b.in_handle) {
            (Some(o), Some(i_h)) => path.curve_to(o, i_h, b.pos),
            (Some(o), None) => path.quad_to(o, b.pos),
            (None, Some(i_h)) => path.quad_to(i_h, b.pos),
            (None, None) => path.line_to(b.pos),
        }
    }
    if close {
        path.close_path();
    }
    path
}

/// Merge runs of collinear line segments. Keeps exported outlines smaller and
/// helps downstream tools, without touching any curve.
pub fn simplify_path(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let elements: Vec<PathEl> = path.iter().collect();

    let mut idx = 0;
    while idx < elements.len() {
        match elements[idx] {
            PathEl::MoveTo(p) => {
                out.move_to(p);
                idx += 1;
            }
            PathEl::ClosePath => {
                out.close_path();
                idx += 1;
            }
            PathEl::LineTo(p) => {
                // Extend the line while the following points stay on it.
                let start = out.current_position().unwrap_or(p);
                let mut end = p;
                let mut j = idx + 1;
                while j < elements.len() {
                    match elements[j] {
                        PathEl::LineTo(q) if is_on_line(start, end, q) => {
                            end = q;
                            j += 1;
                        }
                        _ => break,
                    }
                }
                out.line_to(end);
                idx = j;
            }
            PathEl::QuadTo(c, p) => {
                out.quad_to(c, p);
                idx += 1;
            }
            PathEl::CurveTo(c1, c2, p) => {
                out.curve_to(c1, c2, p);
                idx += 1;
            }
        }
    }
    out
}

fn is_on_line(a: Point, b: Point, p: Point) -> bool {
    if (a - b).hypot() < 1e-9 {
        return false;
    }
    let d = b - a;
    let cross = d.x * (p.y - a.y) - d.y * (p.x - a.x);
    cross.abs() <= 1e-6 * d.hypot()
}

/// Convert every curve into line segments, one subpath per element group.
pub fn flatten(path: &BezPath, tolerance: f64) -> Vec<Vec<Point>> {
    let mut subpaths: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut start = Point::ZERO;

    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) => {
                if current.len() > 1 {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
                start = p;
                current.push(p);
            }
            PathEl::LineTo(p) => current.push(p),
            PathEl::QuadTo(c, p) => {
                let curve = CubicBez::new(start, c, c, p);
                push_cubic(&mut current, &curve, tolerance);
            }
            PathEl::CurveTo(c1, c2, p) => {
                let curve = CubicBez::new(start, c1, c2, p);
                push_cubic(&mut current, &curve, tolerance);
            }
            PathEl::ClosePath => {
                if current.len() > 1 {
                    current.push(start);
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    if current.len() > 1 {
        subpaths.push(current);
    }
    subpaths
}

fn push_cubic(out: &mut Vec<Point>, c: &CubicBez, tolerance: f64) {
    // Estimate the segment count from the control polygon deviation.
    let d1 = (c.p1 - c.p0).hypot() + (c.p2 - c.p1).hypot() + (c.p3 - c.p2).hypot();
    let chord = (c.p3 - c.p0).hypot();
    let dev = (d1 - chord).max(0.0);
    let steps = if dev < 1e-9 {
        1
    } else {
        (((dev / tolerance).sqrt() * 2.0).ceil() as usize).clamp(1, MAX_FLATTEN_STEPS)
    };
    for i in 1..=steps {
        out.push(c.eval(i as f64 / steps as f64));
    }
}

/// Bounds of a path, or `None` when the path is empty or degenerate.
pub fn bounds(path: &BezPath) -> Option<Rect> {
    if path.is_empty() {
        return None;
    }
    let b = path.bounding_box();
    if !b.is_finite() {
        None
    } else {
        Some(b)
    }
}

/// Shortest distance from `p` to the outline of `path`, measured on a flattened
/// copy. Used for hit-testing the pen and select tools.
pub fn distance_to_outline(path: &BezPath, p: Point) -> f64 {
    let polys = flatten(path, FLATTEN_TOLERANCE);
    let mut best = f64::INFINITY;
    for poly in &polys {
        for w in poly.windows(2) {
            let d = dist_point_segment(p, w[0], w[1]);
            if d < best {
                best = d;
            }
        }
    }
    best
}

pub fn dist_point_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.hypot2();
    if len2 < 1e-12 {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).hypot()
}

/// Even-odd point-in-path test.
///
/// Even-odd rather than non-zero winding on purpose: a counter drawn inside a
/// bowl is a hole regardless of the order the user drew the two contours in,
/// which is what a designer expects from a pen tool.
pub fn contains_point(path: &BezPath, p: Point) -> bool {
    let polys = flatten(path, FLATTEN_TOLERANCE);
    let mut inside = false;
    for poly in &polys {
        let closed = match (poly.first(), poly.last()) {
            (Some(f), Some(l)) if (*f - *l).hypot() < 1e-9 => &poly[..poly.len() - 1],
            _ => &poly[..],
        };
        for w in closed.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.y > p.y) != (b.y > p.y) {
                let t = (p.y - a.y) / (b.y - a.y);
                if p.x < a.x + t * (b.x - a.x) {
                    inside = !inside;
                }
            }
        }
    }
    inside
}

/// Whether `p` sits on one of the extrema of a curve segment. The pen tool uses
/// this to grab a curve anywhere along it rather than only at its nodes.
pub fn near_curve_extrema(p: Point, seg: &CubicBez, tolerance: f64) -> bool {
    seg.extrema().iter().any(|t| (seg.eval(*t) - p).hypot() <= tolerance)
}

/// Transform every node of a contour. Handles are transformed as absolute
/// points, so a non-uniform scale skews them exactly as Illustrator does.
pub fn transform_nodes(nodes: &mut [Node], m: Affine) {
    for n in nodes.iter_mut() {
        n.pos = m * n.pos;
        if let Some(h) = &mut n.in_handle {
            *h = m * *h;
        }
        if let Some(h) = &mut n.out_handle {
            *h = m * *h;
        }
    }
}

/// Mirror points across the vertical line `x = axis_x`.
pub fn mirror_x(nodes: &mut [Node], axis_x: f64) {
    for n in nodes.iter_mut() {
        n.pos = Point::new(2.0 * axis_x - n.pos.x, n.pos.y);
        if let Some(h) = &mut n.in_handle {
            *h = Point::new(2.0 * axis_x - h.x, h.y);
        }
        if let Some(h) = &mut n.out_handle {
            *h = Point::new(2.0 * axis_x - h.x, h.y);
        }
    }
}

/// Round to the nearest 1/64 unit, which is the grid TrueType stores on.
pub fn round_to_f26dot6(v: f64) -> i32 {
    (v * 64.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Node> {
        vec![
            Node::corner(Point::new(0.0, 0.0)),
            Node::corner(Point::new(100.0, 0.0)),
            Node::corner(Point::new(100.0, 100.0)),
            Node::corner(Point::new(0.0, 100.0)),
        ]
    }

    #[test]
    fn square_contour_roundtrips() {
        let path = contour_to_bezpath(&square(), true);
        let b = bounds(&path).unwrap();
        assert!((b.width() - 100.0).abs() < 1e-9);
        assert!((b.height() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn hit_test_inside_and_outside() {
        let path = contour_to_bezpath(&square(), true);
        assert!(contains_point(&path, Point::new(50.0, 50.0)));
        assert!(!contains_point(&path, Point::new(150.0, 50.0)));
        assert!(distance_to_outline(&path, Point::new(50.0, 0.0)) < 0.01);
        assert!(distance_to_outline(&path, Point::new(50.0, 50.0)) > 49.0);
    }

    #[test]
    fn hole_is_detected_even_when_winding_matches() {
        let mut path = contour_to_bezpath(&square(), true);
        let inner = vec![
            Node::corner(Point::new(25.0, 25.0)),
            Node::corner(Point::new(75.0, 25.0)),
            Node::corner(Point::new(75.0, 75.0)),
            Node::corner(Point::new(25.0, 75.0)),
        ];
        path.extend(contour_to_bezpath(&inner, true));
        // Both contours are wound the same way, yet even-odd still finds a hole.
        assert!(!contains_point(&path, Point::new(50.0, 50.0)));
        assert!(contains_point(&path, Point::new(10.0, 10.0)));
    }

    #[test]
    fn simplify_merges_collinear_lines_only() {
        let mut path = BezPath::new();
        path.move_to(Point::ZERO);
        path.line_to(Point::new(10.0, 0.0));
        path.line_to(Point::new(20.0, 0.0));
        path.line_to(Point::new(30.0, 5.0));
        let simple = simplify_path(&path);
        assert_eq!(simple.iter().filter(|e| matches!(e, PathEl::LineTo(_))).count(), 2);
        let original = flatten(&path, 0.01).len();
        assert_eq!(flatten(&simple, 0.01).len(), original, "geometry must not change");
    }

    #[test]
    fn mirror_keeps_handle_offsets() {
        let mut nodes = vec![Node::smooth(
            Point::new(10.0, 0.0),
            Point::new(5.0, 10.0),
            Point::new(15.0, -10.0),
        )];
        mirror_x(&mut nodes, 20.0);
        assert_eq!(nodes[0].pos, Point::new(30.0, 0.0));
        assert_eq!(nodes[0].in_handle, Some(Point::new(35.0, 10.0)));
        assert_eq!(nodes[0].out_handle, Some(Point::new(25.0, -10.0)));
    }

    #[test]
    fn curve_extrema_are_found() {
        let seg = CubicBez::new(
            Point::new(0.0, 0.0),
            Point::new(0.0, 100.0),
            Point::new(100.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert!(near_curve_extrema(Point::new(50.0, 75.0), &seg, 1.0));
        assert!(!near_curve_extrema(Point::new(50.0, 5.0), &seg, 1.0));
    }
}