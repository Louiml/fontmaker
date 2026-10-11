//! The document: the in-memory model of a typeface, persisted as JSON.

use crate::glyph::{Contour, Form, Glyph, JoiningType};
use crate::variation::{Axis, Location};
use kurbo::Rect;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Which script a document targets. Drives the glyph grid and shaping rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
}

impl Script {
    pub fn all() -> [Script; 5] {
        [Script::Latin, Script::Cyrillic, Script::Greek, Script::Arabic, Script::Hebrew]
    }

    pub fn id(self) -> &'static str {
        match self {
            Script::Latin => "latn",
            Script::Cyrillic => "cyrl",
            Script::Greek => "grek",
            Script::Arabic => "arab",
            Script::Hebrew => "hebr",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Script::Latin => "Latin",
            Script::Cyrillic => "Cyrillic",
            Script::Greek => "Greek",
            Script::Arabic => "Arabic",
            Script::Hebrew => "Hebrew",
        }
    }

    /// Whether this script needs positional forms and joining behaviour.
    pub fn uses_forms(self) -> bool {
        matches!(self, Script::Arabic)
    }

    /// Whether the script is written right-to-left.
    pub fn is_rtl(self) -> bool {
        matches!(self, Script::Arabic | Script::Hebrew)
    }

    /// Which positional forms are drawn.
    ///
    /// Arabic needs all four. Hebrew does not: its final forms (ך ם ן ף ץ) are
    /// separate codepoints, so one glyph per codepoint is enough.
    pub fn forms(self) -> &'static [Form] {
        match self {
            Script::Arabic => &Form::ALL,
            _ => &[Form::Isol],
        }
    }
}

impl std::fmt::Display for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Default vertical metrics for a script, in font units at 1000 upem.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VerticalMetrics {
    pub upem: f64,
    pub ascender: f64,
    pub descender: f64,
    pub line_gap: f64,
    /// Cap height, used as a guide and for OS/2 sCapHeight.
    pub cap_height: f64,
    /// Lowercase x-height.
    pub x_height: f64,
    /// Overshoot of round glyphs.
    pub overshoot: f64,
}

impl Default for VerticalMetrics {
    fn default() -> Self {
        Self {
            upem: 1000.0,
            ascender: 750.0,
            descender: -250.0,
            line_gap: 0.0,
            cap_height: 700.0,
            x_height: 500.0,
            overshoot: 10.0,
        }
    }
}

impl VerticalMetrics {
    /// Horizontal guide lines shown on the canvas, highest first.
    pub fn guides(self) -> Vec<(&'static str, f64)> {
        vec![
            ("ascender", self.ascender),
            ("cap", self.cap_height + self.overshoot),
            ("cap", self.cap_height),
            ("x", self.x_height + self.overshoot),
            ("x", self.x_height),
            ("baseline", 0.0),
            ("descender", self.descender),
        ]
    }

    pub fn line_height(self) -> f64 {
        self.ascender - self.descender + self.line_gap
    }
}

/// Static font identity. Variable-only fields live in [`Axis`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontMeta {
    pub family_name: String,
    pub style_name: String,
    pub version: String,
    pub designer: String,
    pub manufacturer: String,
    pub license: String,
    pub description: String,
    /// Preferred panose digits, ten values. Kept as a list so it round-trips
    /// through JSON without inventing a newtype.
    #[serde(default = "default_panose")]
    pub panose: [u8; 10],
    /// Typographic family used to group styles in font menus.
    pub typographic_family: String,
}

fn default_panose() -> [u8; 10] {
    [2, 0, 5, 0, 0, 0, 0, 0, 0, 0]
}

impl Default for FontMeta {
    fn default() -> Self {
        Self {
            family_name: "Untitled".to_string(),
            style_name: "Regular".to_string(),
            version: "0.001".to_string(),
            designer: String::new(),
            manufacturer: String::new(),
            license: String::new(),
            description: String::new(),
            panose: default_panose(),
            typographic_family: String::new(),
        }
    }
}

impl FontMeta {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.family_name, self.style_name).trim().to_string()
    }

    /// PostScript name: family and style without spaces, ASCII only.
    pub fn postscript_name(&self) -> String {
        let base = if self.typographic_family.is_empty() {
            self.family_name.as_str()
        } else {
            self.typographic_family.as_str()
        };
        let stem = sanitize_postscript(base);
        let style = sanitize_postscript(&self.style_name);
        if style.is_empty() {
            stem
        } else {
            format!("{stem}-{style}")
        }
    }
}

/// PostScript name: family and style joined by a dash, ASCII alphanumerics only.
///
/// The spec allows only these characters, and every mainstream tool expects the
/// style separated by a dash: `MyFancyFace-BoldItalic`.
fn sanitize_postscript(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// A kerning pair, in font units.
pub type KernPair = (String, String, f64);

/// How a codepoint maps to glyphs before shaping is applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharsetEntry {
    pub codepoint: u32,
    pub glyph: String,
    /// Present for joining scripts.
    #[serde(default)]
    pub joining: JoiningType,
    /// Whether this codepoint blocks joining across it (space, punctuation).
    #[serde(default)]
    pub transparent: bool,
}

/// The whole typeface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub meta: FontMeta,
    pub script: Script,
    pub metrics: VerticalMetrics,
    /// Ordered glyph names; export order follows this list.
    pub glyph_order: Vec<String>,
    pub glyphs: HashMap<String, Glyph>,
    #[serde(default)]
    pub charset: Vec<CharsetEntry>,
    #[serde(default)]
    pub kerning: Vec<KernPair>,
    /// Variable font axes. Empty means a static font.
    #[serde(default)]
    pub axes: Vec<Axis>,
    /// The design the user is editing. The default location sits at the
    /// default value of each axis.
    #[serde(default = "default_location")]
    pub location: Location,
    /// Named colour used by the canvas, purely cosmetic.
    #[serde(default = "default_guide_color")]
    pub guide_color: String,
    #[serde(default)]
    pub alternatives: BTreeMap<String, BTreeMap<String, String>>,
}

fn default_location() -> Location {
    Location::default()
}

fn default_guide_color() -> String {
    "#00b3ff".to_string()
}

impl Default for Document {
    fn default() -> Self {
        let mut doc = Self {
            meta: FontMeta::default(),
            script: Script::Latin,
            metrics: VerticalMetrics::default(),
            glyph_order: Vec::new(),
            glyphs: HashMap::new(),
            charset: Vec::new(),
            kerning: Vec::new(),
            axes: Vec::new(),
            location: Location::default(),
            guide_color: default_guide_color(),
            alternatives: BTreeMap::new(),
        };
        // Every font needs these three, even before any glyph is drawn.
        for name in [".notdef", "space"] {
            doc.add_glyph(Glyph::new(name));
        }
        doc
    }
}

impl Document {
    /// Add a glyph and append it to the glyph order.
    pub fn add_glyph(&mut self, glyph: Glyph) -> bool {
        if self.glyphs.contains_key(&glyph.name) {
            return false;
        }
        self.glyph_order.push(glyph.name.clone());
        self.glyphs.insert(glyph.name.clone(), glyph);
        true
    }

    pub fn remove_glyph(&mut self, name: &str) -> Option<Glyph> {
        if name == ".notdef" || name == "space" {
            return None;
        }
        self.glyph_order.retain(|n| n != name);
        self.charset.retain(|c| c.glyph != name);
        self.glyphs.remove(name)
    }

    /// Rename a glyph and update every reference to it: the glyph order, the
    /// charset and all kerning pairs.
    ///
    /// Positional form glyphs (`A.init` etc.) are separate glyphs and are NOT
    /// renamed along with their base letter; rename them individually.
    pub fn rename_glyph(&mut self, old: &str, new: &str) -> Result<(), String> {
        if old == new {
            return Ok(());
        }
        let new = new.trim();
        if new.is_empty() {
            return Err("The new name is empty.".to_string());
        }
        if new.contains(|c: char| c.is_whitespace()) {
            return Err("Glyph names cannot contain whitespace.".to_string());
        }
        if new == ".notdef" || new == "space" {
            return Err(format!("'{new}' is reserved."));
        }
        if !self.glyphs.contains_key(old) {
            return Err(format!("No glyph named '{old}'."));
        }
        if self.glyphs.contains_key(new) {
            return Err(format!("'{new}' already exists."));
        }

        let mut glyph = self.glyphs.remove(old).expect("checked above");
        glyph.name = new.to_string();
        self.glyphs.insert(new.to_string(), glyph);

        for name in &mut self.glyph_order {
            if name == old {
                *name = new.to_string();
            }
        }
        for entry in &mut self.charset {
            if entry.glyph == old {
                entry.glyph = new.to_string();
            }
        }
        for (left, right, _) in &mut self.kerning {
            if left == old {
                *left = new.to_string();
            }
            if right == old {
                *right = new.to_string();
            }
        }
        Ok(())
    }

    pub fn glyph(&self, name: &str) -> Option<&Glyph> {
        self.glyphs.get(name)
    }

    pub fn glyph_mut(&mut self, name: &str) -> Option<&mut Glyph> {
        self.glyphs.get_mut(name)
    }

    /// All glyphs in export order, skipping names missing from the map.
    pub fn ordered_glyphs(&self) -> impl Iterator<Item = &Glyph> {
        self.glyph_order.iter().filter_map(move |n| self.glyphs.get(n))
    }

    pub fn drawable_glyphs(&self) -> Vec<&Glyph> {
        self.ordered_glyphs().filter(|g| g.is_drawn()).collect()
    }

    pub fn codepoint_for(&self, glyph_name: &str) -> Option<u32> {
        self.charset.iter().find(|c| c.glyph == glyph_name).map(|c| c.codepoint)
    }

    pub fn charset_entry(&self, codepoint: u32) -> Option<&CharsetEntry> {
        self.charset.iter().find(|c| c.codepoint == codepoint)
    }

    /// The four (or fewer) form glyph names for a base glyph.
    pub fn form_names(&self, base: &str) -> Vec<(Form, String)> {
        self.script
            .forms()
            .iter()
            .map(|f| (*f, format!("{base}{}", f.suffix())))
            .collect()
    }

    pub fn kern_value(&self, left: &str, right: &str) -> f64 {
        self.kerning
            .iter()
            .find(|(l, r, _)| l == left && r == right)
            .map(|(_, _, v)| *v)
            .unwrap_or(0.0)
    }

    pub fn set_kern(&mut self, left: &str, right: &str, value: f64) {
        if let Some(entry) = self
            .kerning
            .iter_mut()
            .find(|(l, r, _)| l == left && r == right)
        {
            entry.2 = value;
        } else if value != 0.0 {
            self.kerning.push((left.to_string(), right.to_string(), value));
        }
    }

    /// Union of every glyph's bounds, ignoring the origin. Used to zoom to fit.
    pub fn ink_bounds(&self) -> Option<Rect> {
        let mut b: Option<Rect> = None;
        for g in self.ordered_glyphs() {
            if let Some(gb) = g.bounds() {
                b = Some(match b {
                    Some(e) => e.union(gb),
                    None => gb,
                });
            }
        }
        b
    }

    /// The contours of a glyph with every component resolved recursively.
    ///
    /// Each component's outlines are scaled then offset per its transform.
    /// Cycles (a glyph ultimately referencing itself) resolve to nothing
    /// rather than recursing forever.
    pub fn resolve_glyph(&self, name: &str) -> Vec<Contour> {
        self.resolve_inner(name, &mut Vec::new())
    }

    fn resolve_inner(&self, name: &str, stack: &mut Vec<String>) -> Vec<Contour> {
        if stack.iter().any(|n| n == name) {
            return Vec::new(); // cycle: drop this branch
        }
        stack.push(name.to_string());
        let mut out = Vec::new();
        if let Some(glyph) = self.glyph(name) {
            out.extend(glyph.contours.iter().cloned());
            for component in &glyph.components {
                let mut sub = self.resolve_inner(&component.glyph, stack);
                for contour in &mut sub {
                    crate::geom::transform_nodes(&mut contour.nodes, component.affine());
                }
                out.extend(sub);
            }
        }
        stack.pop();
        out
    }

    /// Bounds of a glyph with its components resolved.
    pub fn resolved_bounds(&self, name: &str) -> Option<Rect> {
        let mut b: Option<Rect> = None;
        for contour in self.resolve_glyph(name) {
            if let Some(cb) = contour.bounds() {
                b = Some(match b {
                    Some(existing) => existing.union(cb),
                    None => cb,
                });
            }
        }
        b
    }

    /// The glyph's contours interpolated to `location` in design space.
    ///
    /// Starts from the default outline and adds the weighted deltas of every
    /// captured variation. The scalar of a variation is the product over its
    /// axes of how far `location` sits towards the variation's captured value
    /// (0..1, same side of the default only) — the standard gvar scalar model
    /// restricted to our peak-at-extreme tents.
    pub fn interpolated_contours(&self, name: &str, location: &Location) -> Vec<Contour> {
        let Some(glyph) = self.glyph(name) else { return Vec::new() };
        // Start from the component-resolved default so composites interpolate
        // against their full ink.
        let mut out = self.resolve_glyph(name);

        for variation in &glyph.variations {
            let scalar = self.variation_scalar(&variation.location, location);
            if scalar <= 1e-9 {
                continue;
            }
            // Only apply when the point structure matches the default outline.
            if !same_point_structure(&out, &variation.contours) {
                continue;
            }
            for (default_contour, var_contour) in out.iter_mut().zip(variation.contours.iter()) {
                for (node, var_node) in default_contour
                    .nodes
                    .iter_mut()
                    .zip(var_contour.nodes.iter())
                {
                    node.pos.x += scalar * (var_node.pos.x - node.pos.x);
                    node.pos.y += scalar * (var_node.pos.y - node.pos.y);
                    if let (Some(h), Some(vh)) = (&mut node.in_handle, &var_node.in_handle) {
                        h.x += scalar * (vh.x - h.x);
                        h.y += scalar * (vh.y - h.y);
                    }
                    if let (Some(h), Some(vh)) = (&mut node.out_handle, &var_node.out_handle) {
                        h.x += scalar * (vh.x - h.x);
                        h.y += scalar * (vh.y - h.y);
                    }
                }
            }
        }
        out
    }

    /// How strongly a variation captured at `captured` influences `location`.
    ///
    /// Per axis: 0 when `location` is on the other side of the default (or at
    /// it), otherwise the linear progress from the default towards the
    /// captured value, clamped to 0..1. Scalars multiply across axes.
    fn variation_scalar(&self, captured: &BTreeMap<String, f64>, location: &Location) -> f64 {
        let mut scalar = 1.0;
        for (tag, captured_value) in captured {
            let Some(axis) = self.axes.iter().find(|a| &a.tag == tag) else {
                return 0.0; // axis no longer exists: variation is inert
            };
            let Some(location_value) = location.get(tag) else {
                return 0.0;
            };
            if (captured_value - axis.default).abs() < 1e-9 {
                continue; // captured at the default: always full weight
            }
            let same_side =
                (location_value - axis.default) * (captured_value - axis.default) > 0.0;
            if !same_side {
                return 0.0;
            }
            let progress = (location_value - axis.default) / (captured_value - axis.default);
            scalar *= progress.clamp(0.0, 1.0);
            if scalar <= 1e-9 {
                return 0.0;
            }
        }
        scalar
    }

    /// How many glyphs in the charset have been drawn. Drives the progress bar.
    pub fn completion(&self) -> (usize, usize) {
        let total = self.charset.len();
        let done = self
            .charset
            .iter()
            .filter(|c| self.glyphs.get(&c.glyph).map(|g| g.is_drawn()).unwrap_or(false))
            .count();
        (done, total)
    }
}

/// Whether two contour lists carry the same number of points per contour, so
/// point-wise deltas between them are well-defined.
fn same_point_structure(a: &[Contour], b: &[Contour]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.nodes.len() == y.nodes.len())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_document_has_required_glyphs() {
        let mut doc = Document::default();
        assert!(doc.glyph(".notdef").is_some());
        assert!(doc.glyph("space").is_some());
        assert_eq!(doc.remove_glyph("space"), None, "space must not be removable");
    }

    #[test]
    fn postscript_name_is_sanitised() {
        let meta = FontMeta {
            family_name: "My Fancy Face".into(),
            style_name: "Bold Italic".into(),
            ..Default::default()
        };
        assert_eq!(meta.postscript_name(), "MyFancyFace-BoldItalic");
    }

    #[test]
    fn glyph_order_drives_export() {
        let mut doc = Document::default();
        doc.add_glyph(Glyph::new("A").with_char('A'));
        doc.add_glyph(Glyph::new("B").with_char('B'));
        let names: Vec<&str> = doc.ordered_glyphs().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec![".notdef", "space", "A", "B"]);
    }

    #[test]
    fn arabic_has_four_forms() {
        let mut doc = Document {
            script: Script::Arabic,
            ..Default::default()
        };
        let names = doc.form_names("uni0627");
        assert_eq!(names.len(), 4);
        assert_eq!(names[3].1, "uni0627.fina");

        doc.script = Script::Hebrew;
        // Hebrew final forms are separate codepoints, not OpenType forms.
        assert_eq!(doc.form_names("uni05D0").len(), 1);
        assert!(!Script::Hebrew.uses_forms());
    }

    #[test]
    fn kern_lookup_and_write() {
        let mut doc = Document::default();
        doc.add_glyph(Glyph::new("A"));
        doc.add_glyph(Glyph::new("V"));
        assert_eq!(doc.kern_value("A", "V"), 0.0);
        doc.set_kern("A", "V", -80.0);
        assert_eq!(doc.kern_value("A", "V"), -80.0);
        doc.set_kern("A", "V", 0.0);
        assert_eq!(doc.kerning.len(), 1, "zeroed pairs are kept but reset");
    }

    #[test]
    fn rename_updates_every_reference() {
        let mut doc = Document::default();
        doc.add_glyph(Glyph::new("A").with_char('A'));
        doc.add_glyph(Glyph::new("V"));
        doc.charset.push(CharsetEntry {
            codepoint: 0x41,
            glyph: "A".into(),
            joining: Default::default(),
            transparent: false,
        });
        doc.set_kern("A", "V", -70.0);

        doc.rename_glyph("A", "Ay").unwrap();
        assert!(doc.glyph("Ay").is_some());
        assert!(doc.glyph("A").is_none());
        assert_eq!(doc.glyph("Ay").unwrap().name, "Ay");
        assert!(doc.glyph_order.contains(&"Ay".to_string()));
        assert_eq!(doc.charset[0].glyph, "Ay");
        assert_eq!(doc.kern_value("Ay", "V"), -70.0, "kerning pairs follow the rename");
        assert_eq!(doc.kern_value("A", "V"), 0.0);
    }

    #[test]
    fn rename_rejects_bad_names() {
        let mut doc = Document::default();
        doc.add_glyph(Glyph::new("A"));
        doc.add_glyph(Glyph::new("B"));

        assert!(doc.rename_glyph("A", "").is_err(), "empty name");
        assert!(doc.rename_glyph("A", "a b").is_err(), "whitespace");
        assert!(doc.rename_glyph("A", "B").is_err(), "existing name");
        assert!(doc.rename_glyph("A", "space").is_err(), "reserved");
        assert!(doc.rename_glyph("Z", "Q").is_err(), "unknown glyph");
        // Nothing was changed by the failed attempts.
        assert!(doc.glyph("A").is_some());
    }

    #[test]
    fn resolve_flattens_components() {
        use crate::glyph::Component;
        let mut doc = Document::default();
        let mut a = Glyph::new("A");
        a.contours.push(crate::stroke::rectangle(0.0, 0.0, 300.0, 700.0));
        doc.add_glyph(a);

        let mut marks = Glyph::new("dieresis");
        marks.contours.push(crate::stroke::rectangle(0.0, 750.0, 100.0, 850.0));
        doc.add_glyph(marks);

        let mut adieresis = Glyph::new("Adieresis");
        adieresis.components.push(Component::new("A", 0.0, 0.0));
        adieresis.components.push(Component::new("dieresis", 350.0, 0.0));
        doc.add_glyph(adieresis);

        let resolved = doc.resolve_glyph("Adieresis");
        assert_eq!(resolved.len(), 2, "A plus the dieresis");
        // The dieresis is offset by (350, 0), widening the bounds past A.
        let b = doc.resolved_bounds("Adieresis").unwrap();
        assert!((b.width() - 450.0).abs() < 1e-9, "width was {}", b.width());
        assert!((b.y1 - 850.0).abs() < 1e-9);
    }

    #[test]
    fn resolve_applies_scale_then_offset() {
        use crate::glyph::Component;
        let mut doc = Document::default();
        let mut a = Glyph::new("A");
        a.contours.push(crate::stroke::rectangle(0.0, 0.0, 100.0, 100.0));
        doc.add_glyph(a);

        let mut scaled = Glyph::new("half");
        scaled.components.push(Component {
            glyph: "A".into(),
            offset: (50.0, 200.0),
            scale: (0.5, 0.5),
        });
        doc.add_glyph(scaled);

        let resolved = doc.resolve_glyph("half");
        assert_eq!(resolved.len(), 1);
        // Point (100, 100) becomes (50, 50) scaled, then (100, 250) offset.
        assert_eq!(resolved[0].nodes[2].pos, kurbo::Point::new(100.0, 250.0));
    }

    #[test]
    fn resolve_survives_cycles() {
        use crate::glyph::Component;
        let mut doc = Document::default();
        let mut a = Glyph::new("A");
        a.contours.push(crate::stroke::rectangle(0.0, 0.0, 100.0, 100.0));
        a.components.push(Component::new("B", 0.0, 0.0));
        doc.add_glyph(a);
        let mut b = Glyph::new("B");
        b.components.push(Component::new("A", 0.0, 0.0));
        doc.add_glyph(b);

        // A -> B -> A: the cycle must terminate, keeping A's own contour.
        let resolved = doc.resolve_glyph("A");
        assert_eq!(resolved.len(), 1, "only A's own contour survives the cycle");
    }

    #[test]
    fn glyph_with_components_counts_as_drawn() {
        use crate::glyph::Component;
        let mut doc = Document::default();
        let mut adieresis = Glyph::new("Adieresis");
        adieresis.components.push(Component::new("A", 0.0, 0.0));
        doc.add_glyph(adieresis);
        assert!(doc.glyph("Adieresis").unwrap().is_drawn());
    }

    #[test]
    fn components_survive_json_roundtrip() {
        use crate::glyph::Component;
        let mut doc = Document::default();
        doc.add_glyph(Glyph::new("A"));
        let mut adieresis = Glyph::new("Adieresis");
        adieresis.components.push(Component {
            glyph: "A".into(),
            offset: (10.0, 20.0),
            scale: (0.9, 0.8),
        });
        doc.add_glyph(adieresis);

        let json = crate::project::to_json(&doc).unwrap();
        let back = crate::project::from_json(&json).unwrap();
        let comps = &back.glyph("Adieresis").unwrap().components;
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].offset, (10.0, 20.0));
        assert_eq!(comps[0].scale, (0.9, 0.8));
    }

    #[test]
    fn json_roundtrip() {
        let mut doc = Document {
            script: Script::Greek,
            ..Default::default()
        };
        doc.add_glyph(Glyph::new("alpha").with_char('\u{03B1}'));
        doc.axes.push(Axis::new("wght", "Weight", 100.0, 900.0, 400.0));
        let json = serde_json::to_string_pretty(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }
}

#[cfg(test)]
mod variation_tests {
    use super::*;
    use crate::glyph::GlyphVariation;
    use crate::stroke::rectangle;

    fn doc_with_axis() -> Document {
        let mut doc = Document::default();
        let mut a = Glyph::new("A");
        a.contours.push(rectangle(0.0, 0.0, 300.0, 700.0));
        doc.add_glyph(a);
        doc.axes.push(crate::Axis::new("wght", "Weight", 100.0, 400.0, 900.0));
        doc
    }

    fn capture_at(doc: &mut Document, value: f64, dx: f64) {
        // Capture A's current outline shifted by (dx, 0) as a variation.
        let mut contours = doc.glyph("A").unwrap().contours.clone();
        for c in &mut contours {
            for n in &mut c.nodes {
                n.pos.x += dx;
            }
        }
        let mut location = BTreeMap::new();
        location.insert("wght".to_string(), value);
        let glyph = doc.glyph_mut("A").unwrap();
        glyph.variations.push(GlyphVariation { location, contours });
    }

    #[test]
    fn interpolation_at_captured_location_matches_capture() {
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);

        let mut location = Location::default();
        location.set("wght", 900.0);
        let out = doc.interpolated_contours("A", &location);
        // Fully at the captured location: x shifted by exactly 50.
        assert_eq!(out[0].nodes[0].pos.x, 50.0);
        assert_eq!(out[0].nodes[0].pos.y, 0.0);
    }

    #[test]
    fn interpolation_at_default_is_the_default() {
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);

        let location = Location::defaults(&doc.axes);
        let out = doc.interpolated_contours("A", &location);
        assert_eq!(out[0].nodes[0].pos.x, 0.0, "no delta at the default");
    }

    #[test]
    fn interpolation_scales_linearly() {
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);

        let mut location = Location::default();
        location.set("wght", 650.0); // halfway between 400 and 900
        let out = doc.interpolated_contours("A", &location);
        assert!((out[0].nodes[0].pos.x - 25.0).abs() < 1e-9, "halfway: got {}", out[0].nodes[0].pos.x);
    }

    #[test]
    fn interpolation_ignores_wrong_side_of_default() {
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);

        let mut location = Location::default();
        location.set("wght", 100.0); // below the default, opposite side
        let out = doc.interpolated_contours("A", &location);
        assert_eq!(out[0].nodes[0].pos.x, 0.0);
    }

    #[test]
    fn variation_survives_json_roundtrip() {
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);
        let json = crate::project::to_json(&doc).unwrap();
        let back = crate::project::from_json(&json).unwrap();
        assert_eq!(back.glyph("A").unwrap().variations.len(), 1);
    }

    #[test]
    fn variations_with_mismatched_points_are_skipped() {
        // A captured variation whose point structure no longer matches the
        // default outline must be ignored, never corrupt the outline.
        let mut doc = doc_with_axis();
        capture_at(&mut doc, 900.0, 50.0);
        // Now add a second node to the default outline: structure diverged.
        if let Some(g) = doc.glyph_mut("A") {
            g.contours[0]
                .nodes
                .push(crate::geom::Node::corner(kurbo::Point::new(150.0, 350.0)));
        }

        let mut location = Location::default();
        location.set("wght", 900.0);
        let out = doc.interpolated_contours("A", &location);
        // No deltas applied, but the extra node is there.
        assert_eq!(out[0].nodes.len(), 5);
        assert_eq!(out[0].nodes[0].pos.x, 0.0);
    }
}
