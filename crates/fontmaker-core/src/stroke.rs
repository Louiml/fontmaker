//! Freehand drawing and stroke-to-outline expansion.
//!
//! Every primitive the user draws ends up as editable nodes, so the pen tool,
//! the brush and the shape tools all converge on the same representation.

use crate::geom::Node;
use crate::glyph::Contour;
use kurbo::{BezPath, Cap, Join, PathEl, Point, Stroke as KurboStroke, StrokeOpts, Vec2};

/// End cap style of an expanded stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeCap {
    Butt,
    #[default]
    Round,
    Square,
}

impl From<StrokeCap> for Cap {
    fn from(c: StrokeCap) -> Self {
        match c {
            StrokeCap::Butt => Cap::Butt,
            StrokeCap::Round => Cap::Round,
            StrokeCap::Square => Cap::Square,
        }
    }
}

/// Corner style of an expanded stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeJoin {
    Miter,
    #[default]
    Round,
    Bevel,
}

impl From<StrokeJoin> for Join {
    fn from(j: StrokeJoin) -> Self {
        match j {
            StrokeJoin::Miter => Join::Miter,
            StrokeJoin::Round => Join::Round,
            StrokeJoin::Bevel => Join::Bevel,
        }
    }
}

/// Width modulation along a stroke. A constant width keeps it simple; the
/// per-point list supports tapered strokes such as the entry of a `f`.
#[derive(Debug, Clone, PartialEq)]
pub enum WidthProfile {
    Constant(f64),
    /// One width per input centreline point.
    PerPoint(Vec<f64>),
}

impl WidthProfile {
    /// Width at input point `index`.
    ///
    /// Indices past the end clamp to the last value rather than falling back,
    /// so a profile with one entry per *input point* still gives the whole
    /// stroke that width when the path carries a few extra segments.
    pub fn at(&self, index: usize, fallback: f64) -> f64 {
        match self {
            WidthProfile::Constant(w) => *w,
            WidthProfile::PerPoint(v) => {
                if v.is_empty() {
                    fallback
                } else {
                    v[index.min(v.len() - 1)]
                }
            }
        }
    }
}

/// Drop points closer than `min_dist` to the previous kept point.
pub fn decimate(points: &[Point], min_dist: f64) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        match out.last() {
            Some(prev) if (*prev - *p).hypot() < min_dist => {}
            _ => out.push(*p),
        }
    }
    out
}

/// Convert a freehand point trail into a smooth cubic centreline.
///
/// Uses a Catmull-Rom spline converted to cubic Béziers, the standard way to
/// turn a mouse trail into type-friendly curves.
pub fn freehand_to_bezpath(points: &[Point], tension: f64) -> BezPath {
    let pts = decimate(points, 0.5);
    let mut path = BezPath::new();
    match pts.len() {
        0 => return path,
        1 => {
            path.move_to(pts[0]);
            return path;
        }
        2 => {
            path.move_to(pts[0]);
            path.line_to(pts[1]);
            return path;
        }
        _ => {}
    }

    path.move_to(pts[0]);
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
        let c1 = p1 + (p2 - p0) * (tension / 6.0);
        let c2 = p2 - (p3 - p1) * (tension / 6.0);
        path.curve_to(c1, c2, p2);
    }
    path
}

fn kurbo_stroke(width: f64, cap: StrokeCap, join: StrokeJoin, miter_limit: f64) -> KurboStroke {
    KurboStroke {
        width,
        join: join.into(),
        miter_limit,
        start_cap: cap.into(),
        end_cap: cap.into(),
        dash_pattern: Default::default(),
        dash_offset: 0.0,
    }
}

/// Expand a centreline stroke into a filled outline.
pub fn stroke_to_path(
    centerline: &BezPath,
    profile: &WidthProfile,
    cap: StrokeCap,
    join: StrokeJoin,
    miter_limit: f64,
) -> BezPath {
    if centerline.is_empty() {
        return BezPath::new();
    }

    match profile {
        WidthProfile::Constant(w) => {
            if *w <= 0.0 {
                return BezPath::new();
            }
            let style = kurbo_stroke(*w, cap, join, miter_limit);
            kurbo::stroke(centerline, &style, &StrokeOpts::default(), 0.05)
        }
        WidthProfile::PerPoint(_) => {
            // Variable width is produced by stroking one subpath per input
            // segment and merging them. This is approximate where the direction
            // changes sharply, which is why the brush keeps the input dense.
            let mut path = BezPath::new();
            let mut pts: Vec<Point> = Vec::new();

            let flush = |pts: &mut Vec<Point>,
                             width: f64,
                             path: &mut BezPath,
                             started: &mut bool| {
                if pts.len() < 2 || width <= 0.0 {
                    return;
                }
                let mut line = BezPath::new();
                line.move_to(pts[0]);
                for q in &pts[1..] {
                    line.line_to(*q);
                }
                let style = kurbo_stroke(width, StrokeCap::Round, StrokeJoin::Round, 4.0);
                let outline = kurbo::stroke(&line, &style, &StrokeOpts::default(), 0.05);
                if !*started {
                    if let Some(PathEl::MoveTo(p)) = outline.iter().next() {
                        path.move_to(p);
                        *started = true;
                    }
                }
                path.extend(outline);
            };

            let mut started = false;
            let mut index = 0usize;
            for el in centerline.iter() {
                match el {
                    PathEl::MoveTo(p) => {
                        flush(&mut pts, WidthProfile::at(profile, index, 0.0), &mut path, &mut started);
                        pts.clear();
                        pts.push(p);
                        index += 1;
                    }
                    PathEl::LineTo(p) => {
                        pts.push(p);
                        index += 1;
                    }
                    PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                        // Sample the curve so tapered widths follow it.
                        let start = pts.last().copied().unwrap_or(p);
                        for i in 1..=4 {
                            let t = i as f64 / 4.0;
                            pts.push(lerp(start, p, t));
                        }
                        index += 1;
                    }
                    PathEl::ClosePath => {
                        flush(&mut pts, WidthProfile::at(profile, index, 0.0), &mut path, &mut started);
                        pts.clear();
                    }
                }
            }
            flush(&mut pts, WidthProfile::at(profile, index, 0.0), &mut path, &mut started);
            path
        }
    }
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    a + (b - a) * t
}

/// Convert a Bézier path back into editable nodes.
///
/// Round joins leave small gaps where consecutive outlines meet, so the largest
/// subpath is taken and traced into a single editable contour.
pub fn path_to_contour(path: &BezPath) -> Option<Contour> {
    let mut subpaths: Vec<Vec<Node>> = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    let mut start = Point::ZERO;
    let mut pending_in: Option<Point> = None;

    for el in path.iter() {
        match el {
            PathEl::MoveTo(p) => {
                if current.len() > 1 {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
                start = p;
                pending_in = None;
                current.push(Node::corner(p));
            }
            PathEl::LineTo(p) => {
                pending_in = None;
                push_node(&mut current, Node::corner(p));
            }
            PathEl::QuadTo(c, p) => {
                if current.is_empty() {
                    current.push(Node::corner(start));
                }
                if let Some(last) = current.last_mut() {
                    if last.pos == start {
                        last.out_handle = Some(c);
                    }
                }
                push_node(&mut current, Node::corner(p));
                pending_in = Some(c);
            }
            PathEl::CurveTo(c1, c2, p) => {
                if current.is_empty() {
                    current.push(Node::corner(start));
                }
                if let Some(last) = current.last_mut() {
                    if last.pos == start {
                        last.out_handle = Some(c1);
                    }
                }
                push_node(&mut current, Node::corner(p));
                pending_in = Some(c2);
            }
            PathEl::ClosePath => {
                // A closed contour repeats its first point at the end. Drop that
                // duplicate so the node list does not carry a phantom node.
                if let (Some(first), Some(last)) = (current.first(), current.last()) {
                    if first.pos == last.pos {
                        current.pop();
                    }
                }
                // Attach the incoming handle of the closing segment to the first
                // node so the contour closes smoothly.
                if let (Some(h), Some(first)) = (pending_in, current.first_mut()) {
                    first.in_handle = Some(h);
                }
                if current.len() > 1 {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
                pending_in = None;
            }
        }
    }
    if current.len() > 1 {
        subpaths.push(current);
    }

    let main = subpaths.into_iter().max_by_key(|n| n.len())?;
    Some(Contour::new(main, true))
}

fn push_node(nodes: &mut Vec<Node>, node: Node) {
    match nodes.last_mut() {
        Some(last) if (last.pos - node.pos).hypot() < 1e-6 => {}
        _ => nodes.push(node),
    }
}

/// A closed rectangle as four editable corner nodes.
pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Contour {
    Contour::new(
        vec![
            Node::corner(Point::new(x0, y0)),
            Node::corner(Point::new(x1, y0)),
            Node::corner(Point::new(x1, y1)),
            Node::corner(Point::new(x0, y1)),
        ],
        true,
    )
}

/// An ellipse as four nodes with smooth handles, so it stays editable as a
/// curve rather than degenerating into a polygon.
pub fn ellipse(center: Point, rx: f64, ry: f64) -> Contour {
    const KAPPA: f64 = 0.552_284_749_830_793_3;
    let off = |dx: f64, dy: f64| center + Vec2::new(dx, dy);
    let (ox, oy) = (rx * KAPPA, ry * KAPPA);
    Contour::new(
        vec![
            Node::smooth(off(rx, 0.0), off(rx, oy), off(rx, -oy)),
            Node::smooth(off(0.0, ry), off(ox, ry), off(-ox, ry)),
            Node::smooth(off(-rx, 0.0), off(-rx, -oy), off(-rx, oy)),
            Node::smooth(off(0.0, -ry), off(-ox, -ry), off(ox, -ry)),
        ],
        true,
    )
}

/// A regular polygon with `sides` vertices.
pub fn polygon(center: Point, radius: f64, sides: usize, rotation: f64) -> Contour {
    let n = sides.max(3);
    let nodes = (0..n)
        .map(|i| {
            let a = rotation + (i as f64 / n as f64) * std::f64::consts::TAU;
            Node::corner(center + Vec2::new(radius * a.cos(), radius * a.sin()))
        })
        .collect();
    Contour::new(nodes, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn horizontal_line() -> BezPath {
        let mut line = BezPath::new();
        line.move_to(Point::new(0.0, 0.0));
        line.line_to(Point::new(100.0, 0.0));
        line
    }

    #[test]
    fn decimate_drops_close_points() {
        let pts = vec![
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.0),
            Point::new(5.0, 0.0),
        ];
        assert_eq!(decimate(&pts, 1.0).len(), 2);
    }

    #[test]
    fn freehand_produces_curves() {
        let pts: Vec<Point> = (0..20)
            .map(|i| Point::new(i as f64, (i as f64 * 0.3).sin() * 10.0))
            .collect();
        let path = freehand_to_bezpath(&pts, 1.0);
        let cubics = path
            .iter()
            .filter(|e| matches!(e, PathEl::CurveTo(_, _, _)))
            .count();
        assert_eq!(cubics, 19, "one cubic per input segment after the first");
    }

    #[test]
    fn freehand_degenerate_input() {
        // A single point has no length, so there is nothing to draw.
        assert!(freehand_to_bezpath(&[], 1.0).is_empty());
        assert!(freehand_to_bezpath(&[Point::new(1.0, 1.0)], 1.0).is_empty());
        assert!(!freehand_to_bezpath(
            &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            1.0
        )
        .is_empty());
    }

    #[test]
    fn stroke_expands_to_outline() {
        let outline = stroke_to_path(
            &horizontal_line(),
            &WidthProfile::Constant(20.0),
            StrokeCap::Butt,
            StrokeJoin::Round,
            4.0,
        );
        let b = outline.bounding_box();
        assert!((b.height() - 20.0).abs() < 0.5, "height was {}", b.height());
        assert!((b.width() - 100.0).abs() < 0.5, "width was {}", b.width());
    }

    #[test]
    fn round_cap_extends_the_ends() {
        let outline = stroke_to_path(
            &horizontal_line(),
            &WidthProfile::Constant(20.0),
            StrokeCap::Round,
            StrokeJoin::Round,
            4.0,
        );
        let b = outline.bounding_box();
        assert!(b.x0 < -9.0 && b.x1 > 109.0, "round cap should overhang: {b:?}");
    }

    #[test]
    fn variable_width_stroke_tapers() {
        let outline = stroke_to_path(
            &horizontal_line(),
            &WidthProfile::PerPoint(vec![2.0, 40.0]),
            StrokeCap::Round,
            StrokeJoin::Round,
            4.0,
        );
        assert!(!outline.is_empty());
        assert!(outline.bounding_box().height() > 20.0);
    }

    #[test]
    fn zero_width_stroke_produces_nothing() {
        let outline = stroke_to_path(
            &horizontal_line(),
            &WidthProfile::Constant(0.0),
            StrokeCap::Round,
            StrokeJoin::Round,
            4.0,
        );
        assert!(outline.is_empty());
    }

    #[test]
    fn ellipse_stays_a_four_node_curve() {
        let c = ellipse(Point::new(0.0, 0.0), 100.0, 50.0);
        assert_eq!(c.nodes.len(), 4);
        assert!(c.nodes.iter().all(|n| !n.is_corner()));
        let b = c.bounds().unwrap();
        assert!((b.width() - 200.0).abs() < 1e-6);
        assert!((b.height() - 100.0).abs() < 1e-6);
        // The handles must produce real curves, not four straight chords: a chord-only
        // ellipse would measure much less than its full 100-unit height.
        let path = c.to_bezpath();
        assert!(path.iter().any(|e| matches!(e, PathEl::CurveTo(_, _, _))));
        assert!((path.bounding_box().height() - 100.0).abs() < 0.01);
    }

    #[test]
    fn path_to_contour_recovers_nodes() {
        let src = rectangle(0.0, 0.0, 100.0, 200.0);
        let back = path_to_contour(&src.to_bezpath()).unwrap();
        assert_eq!(back.nodes.len(), 4);
        assert!((back.bounds().unwrap().width() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn stroke_outline_becomes_editable_contour() {
        let outline = stroke_to_path(
            &horizontal_line(),
            &WidthProfile::Constant(20.0),
            StrokeCap::Butt,
            StrokeJoin::Round,
            4.0,
        );
        let contour = path_to_contour(&outline).unwrap();
        assert!(contour.nodes.len() >= 4);
        let b = contour.bounds().unwrap();
        assert!((b.height() - 20.0).abs() < 1.0);
    }
}