//! Variable font support: axes, design locations and per-glyph deltas.
//!
//! The document stores a single outline per glyph plus a sparse set of deltas
//! per axis location. The exporter turns these into `fvar`/`gvar`, which keeps
//! the JSON format small and lets the user interpolate preview positions
//! continuously without storing whole masters.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One design axis, e.g. Weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    /// Four-character OpenType tag, e.g. `wght`.
    pub tag: String,
    /// Human-readable name.
    pub name: String,
    pub min: f64,
    pub default: f64,
    pub max: f64,
}

impl Axis {
    pub fn new(tag: &str, name: &str, min: f64, default: f64, max: f64) -> Self {
        Self { tag: tag.to_string(), name: name.to_string(), min, default, max }
    }

    pub fn is_valid(&self) -> bool {
        self.tag.len() == 4
            && self.min < self.default
            && self.default < self.max
            && self.tag.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
    }

    /// Normalised position of `value` in the axis range, clamped to 0..1.
    pub fn normalize(&self, value: f64) -> f64 {
        if (self.max - self.min).abs() < f64::EPSILON {
            0.0
        } else {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }
}

/// A point in design space: one user-defined value per axis.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Location {
    pub values: BTreeMap<String, f64>,
}

impl Location {
    pub fn get(&self, tag: &str) -> Option<f64> {
        self.values.get(tag).copied()
    }

    pub fn set(&mut self, tag: &str, value: f64) {
        self.values.insert(tag.to_string(), value);
    }

    /// The default value of every axis.
    pub fn defaults(axes: &[Axis]) -> Self {
        Self {
            values: axes.iter().map(|a| (a.tag.clone(), a.default)).collect(),
        }
    }

    /// The minimum value of every axis.
    pub fn min(axes: &[Axis]) -> Self {
        Self { values: axes.iter().map(|a| (a.tag.clone(), a.min)).collect() }
    }

    /// The maximum value of every axis.
    pub fn max(axes: &[Axis]) -> Self {
        Self { values: axes.iter().map(|a| (a.tag.clone(), a.max)).collect() }
    }

    /// Clamp every value into its axis range.
    pub fn clamp_to(&self, axes: &[Axis]) -> Self {
        let mut out = self.clone();
        for a in axes {
            if let Some(v) = out.values.get(&a.tag) {
                let v = v.clamp(a.min, a.max);
                out.values.insert(a.tag.clone(), v);
            }
        }
        out
    }
}

/// A sparse set of deltas for one glyph at one design location.
///
/// `x_deltas` and `y_deltas` are per-point and parallel to the glyph's flattened
/// node list. Empty vectors mean "no change at this location".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GlyphDeltas {
    pub location: Location,
    #[serde(default)]
    pub x_deltas: Vec<i16>,
    #[serde(default)]
    pub y_deltas: Vec<i16>,
    /// Advance width delta, applied to every contour's metrics.
    #[serde(default)]
    pub advance_delta: i16,
}

impl GlyphDeltas {
    pub fn is_empty(&self) -> bool {
        self.x_deltas.is_empty() && self.y_deltas.is_empty() && self.advance_delta == 0
    }
}

/// All deltas for one glyph, keyed by location string.
///
/// The location key is the canonical form produced by
/// [`LocationKey::of`] so ordering is deterministic in JSON.
pub type GlyphDeltaSet = BTreeMap<String, GlyphDeltas>;

/// Canonical, stable string form of a location, used as a map key.
pub struct LocationKey;

impl LocationKey {
    pub fn of(loc: &Location) -> String {
        let mut parts: Vec<String> = loc
            .values
            .iter()
            .map(|(tag, v)| format!("{tag}={v}"))
            .collect();
        parts.sort();
        parts.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_validation() {
        assert!(Axis::new("wght", "Weight", 100.0, 400.0, 900.0).is_valid());
        assert!(!Axis::new("wgh", "Weight", 100.0, 400.0, 900.0).is_valid());
        assert!(!Axis::new("wght", "Weight", 400.0, 400.0, 900.0).is_valid());
        assert!(!Axis::new("wght", "Weight", 100.0, 900.0, 900.0).is_valid());
    }

    #[test]
    fn normalize_clamps() {
        let a = Axis::new("wght", "Weight", 100.0, 400.0, 900.0);
        assert!((a.normalize(100.0) - 0.0).abs() < 1e-9);
        assert!((a.normalize(500.0) - 0.5).abs() < 1e-9);
        assert!((a.normalize(900.0) - 1.0).abs() < 1e-9);
        assert!((a.normalize(9999.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn location_key_is_order_independent() {
        let mut a = Location::default();
        a.set("wght", 700.0);
        a.set("wdth", 100.0);
        let mut b = Location::default();
        b.set("wdth", 100.0);
        b.set("wght", 700.0);
        assert_eq!(LocationKey::of(&a), LocationKey::of(&b));
    }

    #[test]
    fn location_clamp() {
        let axes = vec![Axis::new("wght", "Weight", 100.0, 400.0, 900.0)];
        let mut loc = Location::default();
        loc.set("wght", 5000.0);
        assert_eq!(loc.clamp_to(&axes).get("wght"), Some(900.0));
        assert_eq!(Location::defaults(&axes).get("wght"), Some(400.0));
        assert_eq!(Location::min(&axes).get("wght"), Some(100.0));
        assert_eq!(Location::max(&axes).get("wght"), Some(900.0));
    }
}