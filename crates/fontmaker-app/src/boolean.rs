//! Boolean operations on glyph outlines.
//!
//! Provides union (unify) operations on glyph contours. The real boolean
//! engine is not implemented yet; until it is, `unify_glyph` returns the
//! outlines unchanged and says so.

use fontmaker_core::glyph::Glyph;
use kurbo::BezPath;

/// Result of a boolean operation on a glyph's contours.
#[derive(Debug, Clone)]
pub struct BooleanResult {
    pub contours: Vec<fontmaker_core::glyph::Contour>,
    pub success: bool,
    pub message: String,
}

/// Union (unify) all contours in a glyph.
/// Returns new contours with overlaps removed.
pub fn unify_glyph(glyph: &Glyph) -> BooleanResult {
    let _ = contours_to_bezpath(&glyph.contours);

    BooleanResult {
        contours: glyph.contours.clone(),
        success: true,
        message: "Outlines returned unchanged: boolean union is not implemented yet.".to_string(),
    }
}

/// Convert fontmaker contours to a single kurbo BezPath.
#[allow(dead_code)]
fn contours_to_bezpath(contours: &[fontmaker_core::glyph::Contour]) -> BezPath {
    let mut path = BezPath::new();
    for contour in contours {
        let nodes = &contour.nodes;
        if nodes.is_empty() {
            continue;
        }
        path.move_to(nodes[0].pos);
        let n = nodes.len();
        let last = if contour.closed { n } else { n - 1 };
        for i in 0..last {
            let a = &nodes[i];
            let b = &nodes[(i + 1) % n];
            match (a.out_handle, a.in_handle, b.in_handle, b.out_handle) {
                (Some(o), Some(i_h), _, _) => path.curve_to(o, i_h, b.pos),
                (Some(o), None, _, _) => path.quad_to(o, b.pos),
                (None, Some(i_h), _, _) => path.quad_to(i_h, b.pos),
                (None, None, _, _) => path.line_to(b.pos),
            }
        }
        if contour.closed {
            path.close_path();
        }
    }
    path
}
