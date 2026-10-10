//! Application state, update logic and layout.

use fontmaker_core::document::{Document, Script};
use fontmaker_core::geom::Node;
use fontmaker_core::glyph::{Contour, Form, Glyph, JoiningType};
use fontmaker_core::history::History;
use fontmaker_core::stroke;
use fontmaker_export::{export, ExportFormat};
use fontmaker_script as scripts;

use iced::keyboard::{self, key, Key};
use iced::widget::{column, row};
use iced::{Element, Point, Subscription, Task};

use crate::canvas_view;
use crate::panels;

/// How far, in screen pixels, the cursor must be to grab a node or handle.
const HIT_RADIUS_PX: f32 = 10.0;
/// How close, in screen pixels, a point must be to a guide to snap onto it.
const SNAP_RADIUS_PX: f32 = 8.0;
/// Directory exported fonts are written to.
const OUT_DIR: &str = "out";
/// File extension of the project JSON.
const PROJECT_EXT: &str = "fontmaker.json";

/// The editing tools, mirroring an Illustrator toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pen,
    Rectangle,
    Ellipse,
    Pan,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Select,
        Tool::Pen,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Pan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select (V)",
            Tool::Pen => "Pen (P)",
            Tool::Rectangle => "Rectangle (R)",
            Tool::Ellipse => "Ellipse (E)",
            Tool::Pan => "Pan (H)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Pen => "P",
            Tool::Rectangle => "R",
            Tool::Ellipse => "E",
            Tool::Pan => "H",
        }
    }
}

/// Mapping between font units and screen pixels.
///
/// The origin is the screen position of font-space `(0, 0)`: the baseline at
/// the left sidebearing. Font space has y growing upwards, so screen y negates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

impl Camera {
    pub fn to_screen(self, p: kurbo::Point) -> Point {
        Point::new(
            self.origin_x + (p.x as f32) * self.scale,
            self.origin_y - (p.y as f32) * self.scale,
        )
    }

    pub fn to_font(self, p: Point) -> kurbo::Point {
        kurbo::Point::new(
            ((p.x - self.origin_x) / self.scale) as f64,
            ((self.origin_y - p.y) / self.scale) as f64,
        )
    }

    /// Zoom keeping the font-space point under `screen` fixed on screen.
    pub fn zoom_at(&mut self, screen: Point, factor: f32) {
        let before = self.to_font(screen);
        self.scale = (self.scale * factor).clamp(0.02, 40.0);
        let after = self.to_font(screen);
        self.origin_x += ((after.x - before.x) as f32) * self.scale;
        self.origin_y -= ((after.y - before.y) as f32) * self.scale;
    }

    /// Fit the current glyph's ink (or the guides when it is empty) into the view.
    pub fn fit(&mut self, doc: &Document, glyph_name: &str, width: f32, height: f32) {
        let glyph = doc.glyph(glyph_name);
        let (x0, y0, x1, y1) = match glyph.and_then(|g| g.bounds()) {
            Some(b) => (
                b.x0 - 100.0,
                (b.y0 - 100.0).min(doc.metrics.descender),
                b.x1 + 100.0,
                b.y1 + 100.0,
            ),
            None => (
                -100.0,
                doc.metrics.descender - 100.0,
                glyph.map(|g| g.advance).unwrap_or(600.0) + 100.0,
                doc.metrics.cap_height + 150.0,
            ),
        };
        let fw = (x1 - x0).max(1.0) as f32;
        let fh = (y1 - y0).max(1.0) as f32;
        self.scale = ((width / fw).min(height / fh) * 0.8).clamp(0.02, 40.0);
        self.origin_x = width / 2.0 - ((x0 + x1) / 2.0) as f32 * self.scale;
        self.origin_y = height / 2.0 + ((y0 + y1) / 2.0) as f32 * self.scale;
    }
}

/// What a press-drag-release gesture is currently doing.
#[derive(Debug, Clone)]
pub enum DragKind {
    Pan { start_screen: Point, origin: (f32, f32) },
    Node { contour: usize, node: usize },
    Handle { contour: usize, node: usize, is_in: bool },
    Marquee { start: kurbo::Point },
    Shape { start: kurbo::Point },
}

/// Messages the interface can send.
#[derive(Debug, Clone)]
pub enum Message {
    SelectGlyph(String),
    SelectScript(Script),
    SelectTool(Tool),
    Undo,
    Redo,
    DeleteSelection,
    FinishContour,
    Export(ExportFormat),
    SaveProject,
    LoadProject,
    PreviewChanged(String),
    NewGlyphName(String),
    AddGlyph,
    SetAdvance(f64),
    DeleteGlyph,
    ToggleSnap,
    FlipH,
    FlipV,
    SelectAllNodes,
    ConvertNodes,
    CopyOutline,
    PasteOutline,
    DuplicateGlyph,
    AutoAdvance,
    MetaChanged(MetaField, String),
    SelectTab(RightTab),
    SelectKernLeft(String),
    SelectKernRight(String),
    KernValueChanged(f64),
    SelectKernPair(usize),
    DeleteKernPair,
    FitView,
    Canvas(CanvasMsg),
}

/// Editable fields of the font metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaField {
    Family,
    Style,
    Designer,
    Version,
}

/// Which panel the right column shows under the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Inspector,
    Info,
    Kerning,
}

/// Events forwarded from the canvas widget.
#[derive(Debug, Clone)]
pub enum CanvasMsg {
    /// A button went down. `font` is the position in font units, `screen` in
    /// canvas pixels (panning needs pixels, because panning changes the
    /// meaning of font-space positions under the cursor).
    Pressed { font: kurbo::Point, screen: Point, right_click: bool },
    Dragged { font: kurbo::Point, screen: Point },
    Released,
    Moved { font: kurbo::Point },
    Scrolled { screen: Point, up: bool },
    /// The canvas reported its size so the view can be fitted. Sent when the
    /// size actually changes, including the first frame after startup.
    Size { width: f32, height: f32 },
}

pub struct App {
    pub doc: Document,
    pub history: History,
    /// Name of the glyph being edited.
    pub current: String,
    pub tool: Tool,
    pub camera: Camera,
    pub drag: Option<DragKind>,
    /// Selected nodes of the current glyph, as (contour, node) indices.
    pub selection: Vec<(usize, usize)>,
    /// Marquee rectangle in font units while it is being drawn.
    pub marquee: Option<(kurbo::Point, kurbo::Point)>,
    pub status: String,
    pub preview: String,
    pub new_glyph_name: String,
    pub fit_pending: bool,
    pub last_size: (f32, f32),
    pub cursor_font: kurbo::Point,
    /// Snap new and dragged points onto guides, the advance edges and
    /// existing nodes.
    pub snap: bool,
    /// Last click, for double-click detection (insert point on segment).
    last_click: Option<(std::time::Instant, kurbo::Point)>,
    /// Set when a press-path mutation happened without starting a drag, so
    /// release() knows to keep it in the undo history.
    pending_change: bool,
    /// Copied outlines, pasted into whichever glyph is current.
    clipboard: Vec<Contour>,
    /// Which panel shows under the preview.
    pub right_tab: RightTab,
    /// Left glyph of the kerning pair being edited.
    pub kern_left: String,
    /// Right glyph of the kerning pair being edited.
    pub kern_right: String,
    /// Index into `doc.kerning` of the pair selected in the list, if any.
    pub kern_selected: Option<usize>,
}

impl App {
    pub fn new() -> Self {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Latin);
        let current = doc
            .glyph_order
            .iter()
            .find(|n| n.as_str() != ".notdef")
            .cloned()
            .unwrap_or_else(|| ".notdef".to_string());
        let kern_right = doc
            .glyph_order
            .iter()
            .skip_while(|n| n.as_str() != current)
            .nth(1)
            .cloned()
            .unwrap_or_else(|| current.clone());
        Self {
            doc,
            history: History::default(),
            current: current.clone(),
            tool: Tool::Select,
            camera: Camera { scale: 0.4, origin_x: 300.0, origin_y: 500.0 },
            drag: None,
            selection: Vec::new(),
            marquee: None,
            status: "Ready. Pick a letter on the left, draw with the pen tool (P).".to_string(),
            preview: scripts::preview_text(Script::Latin).to_string(),
            new_glyph_name: String::new(),
            fit_pending: true,
            last_size: (800.0, 600.0),
            cursor_font: kurbo::Point::ZERO,
            snap: true,
            last_click: None,
            pending_change: false,
            clipboard: Vec::new(),
            right_tab: RightTab::Inspector,
            kern_left: current.clone(),
            kern_right,
            kern_selected: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectGlyph(name) => {
                if self.doc.glyphs.contains_key(&name) {
                    self.current = name;
                    self.selection.clear();
                    self.marquee = None;
                    self.fit_pending = true;
                    self.refit();
                    self.status = format!("Editing {}", self.current);
                }
                Task::none()
            }
            Message::SelectScript(script) => {
                scripts::apply_script(&mut self.doc, script);
                self.preview = scripts::preview_text(script).to_string();
                if !self.doc.glyphs.contains_key(&self.current) {
                    self.current = self
                        .doc
                        .glyph_order
                        .first()
                        .cloned()
                        .unwrap_or_else(|| ".notdef".to_string());
                }
                self.selection.clear();
                self.fit_pending = true;
                self.refit();
                self.status = format!(
                    "Switched to {}. {} glyphs in the set.",
                    script.display_name(),
                    self.doc.charset.len()
                );
                Task::none()
            }
            Message::SelectTool(tool) => {
                self.tool = tool;
                self.status = format!("Tool: {}", tool.label());
                Task::none()
            }
            Message::Undo => {
                if self.history.undo(&mut self.doc) {
                    self.drop_stale_selection();
                    self.status = "Undone.".to_string();
                } else {
                    self.status = "Nothing to undo.".to_string();
                }
                Task::none()
            }
            Message::Redo => {
                if self.history.redo(&mut self.doc) {
                    self.drop_stale_selection();
                    self.status = "Redone.".to_string();
                } else {
                    self.status = "Nothing to redo.".to_string();
                }
                Task::none()
            }
            Message::DeleteSelection => {
                self.delete_selection();
                Task::none()
            }
            Message::FinishContour => {
                self.finish_open_contour();
                Task::none()
            }
            Message::Export(format) => {
                self.export(format);
                Task::none()
            }
            Message::SaveProject => {
                self.save_project();
                Task::none()
            }
            Message::LoadProject => {
                self.load_project();
                Task::none()
            }
            Message::PreviewChanged(text) => {
                self.preview = text;
                Task::none()
            }
            Message::NewGlyphName(name) => {
                self.new_glyph_name = name;
                Task::none()
            }
            Message::AddGlyph => {
                let name = self.new_glyph_name.trim().to_string();
                if name.is_empty() {
                    self.status = "Type a glyph name first.".to_string();
                } else if self.doc.add_glyph(Glyph::new(name.clone())) {
                    self.status = format!("Added {name}.");
                    self.new_glyph_name.clear();
                } else {
                    self.status = format!("'{name}' already exists.");
                }
                Task::none()
            }
            Message::SetAdvance(value) => {
                if let Some(g) = self.doc.glyph_mut(&self.current) {
                    g.advance = value.clamp(0.0, 60000.0);
                }
                Task::none()
            }
            Message::DeleteGlyph => {
                let name = self.current.clone();
                match self.doc.remove_glyph(&name) {
                    Some(_) => {
                        self.current = self
                            .doc
                            .glyph_order
                            .first()
                            .cloned()
                            .unwrap_or_else(|| ".notdef".to_string());
                        self.selection.clear();
                        self.status = format!("Deleted {name}.");
                    }
                    None => self.status = format!("'{name}' cannot be deleted."),
                }
                Task::none()
            }
            Message::ToggleSnap => {
                self.snap = !self.snap;
                self.status = format!("Snapping {}.", if self.snap { "on" } else { "off" });
                Task::none()
            }
            Message::FlipH => {
                self.history.begin(&self.doc);
                self.history.touch(&self.doc, &self.current);
                let axis = self.doc.glyph(&self.current).map(|g| g.advance / 2.0).unwrap_or(0.0);
                if let Some(g) = self.doc.glyph_mut(&self.current) {
                    for c in &mut g.contours {
                        fontmaker_core::mirror_x(&mut c.nodes, axis);
                    }
                }
                self.history.commit(&self.doc);
                self.status = "Flipped horizontally.".to_string();
                Task::none()
            }
            Message::FlipV => {
                self.history.begin(&self.doc);
                self.history.touch(&self.doc, &self.current);
                // Flip about the middle of the ink, or the midpoint between
                // baseline and cap height for an empty glyph.
                let axis = self
                    .doc
                    .glyph(&self.current)
                    .and_then(|g| g.bounds())
                    .map(|b| (b.y0 + b.y1) / 2.0)
                    .unwrap_or(self.doc.metrics.cap_height / 2.0);
                if let Some(g) = self.doc.glyph_mut(&self.current) {
                    for c in &mut g.contours {
                        fontmaker_core::mirror_y(&mut c.nodes, axis);
                    }
                }
                self.history.commit(&self.doc);
                self.status = "Flipped vertically.".to_string();
                Task::none()
            }
            Message::SelectAllNodes => {
                self.selection.clear();
                if let Some(g) = self.doc.glyph(&self.current) {
                    for (ci, c) in g.contours.iter().enumerate() {
                        for ni in 0..c.nodes.len() {
                            self.selection.push((ci, ni));
                        }
                    }
                }
                self.status = format!("Selected {} nodes.", self.selection.len());
                Task::none()
            }
            Message::ConvertNodes => {
                if self.selection.is_empty() {
                    self.status = "Select nodes to convert (corner ↔ smooth).".to_string();
                    return Task::none();
                }
                self.history.begin(&self.doc);
                self.history.touch(&self.doc, &self.current);
                let count = self.convert_selected_nodes();
                self.history.commit(&self.doc);
                self.status = format!(
                    "Converted {} node(s): smooth ↔ corner.",
                    count
                );
                Task::none()
            }
            Message::CopyOutline => {
                self.clipboard = self
                    .doc
                    .glyph(&self.current)
                    .map(|g| g.contours.clone())
                    .unwrap_or_default();
                self.status = if self.clipboard.is_empty() {
                    format!("'{}' has no outlines to copy.", self.current)
                } else {
                    format!(
                        "Copied {} contour(s) from '{}'. Select another glyph and paste.",
                        self.clipboard.len(),
                        self.current
                    )
                };
                Task::none()
            }
            Message::PasteOutline => {
                if self.clipboard.is_empty() {
                    self.status = "Outline clipboard is empty.".to_string();
                    return Task::none();
                }
                self.history.begin(&self.doc);
                self.history.touch(&self.doc, &self.current);
                let count = self.clipboard.len();
                if let Some(g) = self.doc.glyph_mut(&self.current) {
                    g.contours.extend(self.clipboard.iter().cloned());
                }
                self.history.commit(&self.doc);
                self.status = format!("Pasted {count} contour(s) into '{}'.", self.current);
                Task::none()
            }
            Message::DuplicateGlyph => {
                let Some(source) = self.doc.glyph(&self.current).cloned() else {
                    return Task::none();
                };
                // First free `name.copy`, `name.copy2`, ...
                let mut n = 1;
                let name = loop {
                    let candidate = if n == 1 {
                        format!("{}.copy", source.name)
                    } else {
                        format!("{}.copy{n}", source.name)
                    };
                    if !self.doc.glyphs.contains_key(&candidate) {
                        break candidate;
                    }
                    n += 1;
                };
                let mut copy = Glyph::new(name.clone());
                copy.contours = source.contours.clone();
                copy.advance = source.advance;
                copy.lsb = source.lsb;
                self.doc.add_glyph(copy);
                self.current = name.clone();
                self.selection.clear();
                self.status = format!("Duplicated '{}' as '{name}'.", source.name);
                Task::none()
            }
            Message::AutoAdvance => {
                let Some(bounds) = self.doc.glyph(&self.current).and_then(|g| g.bounds()) else {
                    self.status = "Nothing drawn to measure.".to_string();
                    return Task::none();
                };
                self.history.begin(&self.doc);
                self.history.touch(&self.doc, &self.current);
                let margin = self.doc.metrics.upem * 0.05;
                if let Some(g) = self.doc.glyph_mut(&self.current) {
                    g.lsb = bounds.x0;
                    g.advance = (bounds.x1 + margin).max(1.0);
                }
                self.history.commit(&self.doc);
                self.status = "Advance set from outline with a right margin.".to_string();
                Task::none()
            }
            Message::MetaChanged(field, value) => {
                match field {
                    MetaField::Family => self.doc.meta.family_name = value,
                    MetaField::Style => self.doc.meta.style_name = value,
                    MetaField::Designer => self.doc.meta.designer = value,
                    MetaField::Version => self.doc.meta.version = value,
                }
                Task::none()
            }
            Message::SelectTab(tab) => {
                self.right_tab = tab;
                Task::none()
            }
            Message::SelectKernLeft(name) => {
                if self.doc.glyphs.contains_key(&name) {
                    self.kern_left = name;
                    self.kern_selected = None;
                }
                Task::none()
            }
            Message::SelectKernRight(name) => {
                if self.doc.glyphs.contains_key(&name) {
                    self.kern_right = name;
                    self.kern_selected = None;
                }
                Task::none()
            }
            Message::KernValueChanged(value) => {
                let value = value.clamp(-2000.0, 2000.0);
                self.doc
                    .set_kern(&self.kern_left.clone(), &self.kern_right.clone(), value);
                self.kern_selected = None;
                Task::none()
            }
            Message::SelectKernPair(index) => {
                if let Some((l, r, _)) = self.doc.kerning.get(index) {
                    self.kern_left = l.clone();
                    self.kern_right = r.clone();
                    self.kern_selected = Some(index);
                    self.status = format!("Editing kerning pair {l} {r}.");
                }
                Task::none()
            }
            Message::DeleteKernPair => {
                if let Some(index) = self.kern_selected {
                    if index < self.doc.kerning.len() {
                        let (l, r, _) = self.doc.kerning.remove(index);
                        self.kern_selected = None;
                        self.status = format!("Deleted kerning pair {l} {r}.");
                    }
                } else {
                    self.status = "Select a kerning pair first.".to_string();
                }
                Task::none()
            }
            Message::FitView => {
                self.refit();
                Task::none()
            }
            Message::Canvas(msg) => self.canvas_message(msg),
        }
    }

    fn canvas_message(&mut self, msg: CanvasMsg) -> Task<Message> {
        match msg {
            CanvasMsg::Size { width, height } => {
                self.last_size = (width, height);
                if self.fit_pending {
                    self.refit();
                }
                Task::none()
            }
            CanvasMsg::Scrolled { screen, up } => {
                let factor = if up { 1.15 } else { 1.0 / 1.15 };
                self.camera.zoom_at(screen, factor);
                Task::none()
            }
            CanvasMsg::Moved { font } => {
                self.cursor_font = font;
                Task::none()
            }
            CanvasMsg::Pressed { font, screen, right_click } => {
                if right_click || self.tool == Tool::Pan {
                    self.drag = Some(DragKind::Pan {
                        start_screen: screen,
                        origin: (self.camera.origin_x, self.camera.origin_y),
                    });
                    return Task::none();
                }
                self.history.begin(&self.doc);
                self.press(font);
                Task::none()
            }
            CanvasMsg::Dragged { font, screen } => {
                self.drag_move(font, screen);
                Task::none()
            }
            CanvasMsg::Released => {
                self.release();
                Task::none()
            }
        }
    }

    // ---- tool behaviour -------------------------------------------------

    fn hit_radius_font(&self) -> f64 {
        (HIT_RADIUS_PX / self.camera.scale) as f64
    }

    /// Snap `p` onto metric guides, the advance edges and existing nodes.
    ///
    /// Each axis snaps independently, which is how drawing programs behave:
    /// a point near the baseline but mid-glyph snaps its y only.
    pub fn snap_point(&self, p: kurbo::Point) -> kurbo::Point {
        if !self.snap {
            return p;
        }
        let threshold = (SNAP_RADIUS_PX / self.camera.scale) as f64;

        let metrics = &self.doc.metrics;
        let mut y_targets = vec![0.0, metrics.ascender, metrics.descender, metrics.cap_height, metrics.x_height];
        let mut x_targets = vec![0.0, self.doc.glyph(&self.current).map(|g| g.advance).unwrap_or(0.0)];

        // Existing nodes are targets too, so contours can be joined exactly.
        if let Some(glyph) = self.doc.glyph(&self.current) {
            for contour in &glyph.contours {
                for node in &contour.nodes {
                    x_targets.push(node.pos.x);
                    y_targets.push(node.pos.y);
                }
            }
        }

        let snapped = |value: f64, targets: &[f64]| -> f64 {
            targets
                .iter()
                .copied()
                .min_by(|a, b| {
                    (value - a).abs()
                        .partial_cmp(&(value - b).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .filter(|t| (value - t).abs() <= threshold)
                .unwrap_or(value)
        };

        kurbo::Point::new(snapped(p.x, &x_targets), snapped(p.y, &y_targets))
    }

    fn hit_node(&self, p: kurbo::Point) -> Option<(usize, usize)> {
        let glyph = self.doc.glyph(&self.current)?;
        let r = self.hit_radius_font();
        for (ci, contour) in glyph.contours.iter().enumerate() {
            for (ni, node) in contour.nodes.iter().enumerate() {
                if (node.pos - p).hypot() <= r {
                    return Some((ci, ni));
                }
            }
        }
        None
    }

    fn hit_handle(&self, p: kurbo::Point) -> Option<(usize, usize, bool)> {
        let glyph = self.doc.glyph(&self.current)?;
        let r = self.hit_radius_font();
        for (ci, contour) in glyph.contours.iter().enumerate() {
            for (ni, node) in contour.nodes.iter().enumerate() {
                if let Some(h) = node.in_handle {
                    if (h - p).hypot() <= r {
                        return Some((ci, ni, true));
                    }
                }
                if let Some(h) = node.out_handle {
                    if (h - p).hypot() <= r {
                        return Some((ci, ni, false));
                    }
                }
            }
        }
        None
    }

    fn press(&mut self, p: kurbo::Point) {
        // Double-click inserts a point on the nearest segment.
        let now = std::time::Instant::now();
        let is_double = self
            .last_click
            .take()
            .map(|(t, q)| {
                t.elapsed().as_millis() < 400
                    && (q - p).hypot() <= (8.0 / self.camera.scale) as f64
            })
            .unwrap_or(false);
        if is_double && matches!(self.tool, Tool::Select | Tool::Pen) {
            self.last_click = None; // avoid triple-click re-triggering
            if self.insert_point_at(p) {
                return;
            }
        }
        self.last_click = Some((now, p));

        match self.tool {
            Tool::Pan => {}
            Tool::Select => {
                if let Some((c, n, is_in)) = self.hit_handle(p) {
                    self.selection = vec![(c, n)];
                    self.drag = Some(DragKind::Handle { contour: c, node: n, is_in });
                } else if let Some((c, n)) = self.hit_node(p) {
                    self.selection = vec![(c, n)];
                    self.drag = Some(DragKind::Node { contour: c, node: n });
                } else {
                    self.selection.clear();
                    self.marquee = Some((p, p));
                    self.drag = Some(DragKind::Marquee { start: p });
                }
            }
            Tool::Pen => {
                // Clicking the first node of the contour being drawn closes it;
                // clicking any other node drags it; otherwise we add a point.
                let open_first = self.open_trailing_contour().and_then(|(ci, _)| {
                    self.hit_node(p).filter(|(c, n)| *c == ci && *n == 0)
                });
                if let Some((ci, _)) = open_first {
                    self.close_contour(ci);
                } else if let Some((c, n)) = self.hit_node(p) {
                    self.selection = vec![(c, n)];
                    self.drag = Some(DragKind::Node { contour: c, node: n });
                } else {
                    self.pen_add_point(p);
                }
            }
            Tool::Rectangle | Tool::Ellipse => {
                self.marquee = Some((p, p));
                self.drag = Some(DragKind::Shape { start: p });
            }
        }
    }

    /// The trailing contour if it is open and non-empty, as `(index, node count)`.
    fn open_trailing_contour(&self) -> Option<(usize, usize)> {
        let glyph = self.doc.glyph(&self.current)?;
        let ci = glyph.contours.len().checked_sub(1)?;
        let contour = glyph.contours.get(ci)?;
        (!contour.closed && !contour.nodes.is_empty()).then_some((ci, contour.nodes.len()))
    }

    fn close_contour(&mut self, ci: usize) {
        self.history.touch(&self.doc, &self.current);
        if let Some(contour) = self
            .doc
            .glyph_mut(&self.current)
            .and_then(|g| g.contours.get_mut(ci))
        {
            contour.closed = true;
        }
        self.history.commit(&self.doc);
        self.selection.clear();
        self.status = "Contour closed.".to_string();
    }

    /// Double-click: split the nearest segment under `p` and insert a smooth
    /// node there. Returns whether a point was inserted.
    fn insert_point_at(&mut self, p: kurbo::Point) -> bool {
        let radius = self.hit_radius_font();
        let mut inserted: Option<(usize, usize)> = None;
        {
            let Some(glyph) = self.doc.glyph_mut(&self.current) else { return false };
            'outer: for (ci, contour) in glyph.contours.iter_mut().enumerate() {
                // Prefer the contour whose segment is genuinely closest: try
                // every contour and keep the best hit.
                if let Some(ni) = fontmaker_core::insert_point_on_segment(contour, p, radius) {
                    inserted = Some((ci, ni));
                    break 'outer;
                }
            }
        }
        match inserted {
            Some((ci, ni)) => {
                self.selection = vec![(ci, ni)];
                self.history.touch(&self.doc, &self.current);
                self.pending_change = true;
                self.status = "Point inserted on segment.".to_string();
                true
            }
            None => false,
        }
    }

    /// Pen tool: append a point to the open contour, or start a new one.
    ///
    /// The new point becomes the drag target, so it follows the cursor for as
    /// long as the button is held down and lands exactly where it was clicked
    /// when it is not.
    fn pen_add_point(&mut self, p: kurbo::Point) {
        let p = self.snap_point(p);
        let target = {
            let Some(glyph) = self.doc.glyph_mut(&self.current) else { return };
            let open = glyph
                .contours
                .last()
                .map(|c| !c.closed && !c.is_empty())
                .unwrap_or(false);
            if open {
                glyph.contours.last_mut().unwrap().nodes.push(Node::corner(p));
            } else {
                glyph.contours.push(Contour::new(vec![Node::corner(p)], false));
                self.status = "Drawing: click to add points, click the first point or press Escape to close.".to_string();
            }
            let ci = glyph.contours.len() - 1;
            let ni = glyph.contours[ci].nodes.len() - 1;
            (ci, ni)
        };
        self.history.touch(&self.doc, &self.current);
        self.selection = vec![target];
        self.drag = Some(DragKind::Node { contour: target.0, node: target.1 });
    }

    fn drag_move(&mut self, font: kurbo::Point, screen: Point) {
        match self.drag.clone() {
            Some(DragKind::Pan { start_screen, origin }) => {
                self.camera.origin_x = origin.0 + (screen.x - start_screen.x);
                self.camera.origin_y = origin.1 + (screen.y - start_screen.y);
            }
            Some(DragKind::Node { contour, node }) => {
                self.history.touch(&self.doc, &self.current);
                let snapped = self.snap_point(font);
                if let Some(old) = self.node_pos(contour, node) {
                    // Dragging one node of a multi-selection moves them all,
                    // like Illustrator.
                    let dragging_selection = self.selection.len() > 1
                        && self.selection.contains(&(contour, node));
                    let targets: Vec<(usize, usize)> = if dragging_selection {
                        self.selection.clone()
                    } else {
                        vec![(contour, node)]
                    };
                    let delta = snapped - old;
                    self.move_nodes(&targets, delta);
                }
            }
            Some(DragKind::Handle { contour, node, is_in }) => {
                self.history.touch(&self.doc, &self.current);
                if let Some(n) = self
                    .doc
                    .glyph_mut(&self.current)
                    .and_then(|g| g.contours.get_mut(contour))
                    .and_then(|c| c.nodes.get_mut(node))
                {
                    // Dragging one handle mirrors the other through the node to
                    // keep the tangent continuous, the way smooth points work.
                    if is_in {
                        n.in_handle = Some(font);
                        n.out_handle = Some(n.pos - (font - n.pos));
                    } else {
                        n.out_handle = Some(font);
                        n.in_handle = Some(n.pos - (font - n.pos));
                    }
                }
            }
            Some(DragKind::Marquee { start }) => {
                self.marquee = Some((start, font));
            }
            Some(DragKind::Shape { .. }) => {
                self.cursor_font = font;
                if let Some((start, _)) = self.marquee {
                    self.marquee = Some((start, font));
                }
            }
            None => {}
        }
    }

    fn release(&mut self) {
        match self.drag.take() {
            Some(DragKind::Marquee { start }) => {
                let end = self
                    .marquee
                    .map(|(_, e)| e)
                    .unwrap_or(start);
                self.marquee = None;
                self.select_in_rect(start, end);
                self.history.commit(&self.doc);
            }
            Some(DragKind::Shape { start }) => {
                let end = self
                    .marquee
                    .map(|(_, e)| e)
                    .unwrap_or(start);
                self.marquee = None;
                if let Some(contour) = self.build_shape(start, end) {
                    if let Some(g) = self.doc.glyph_mut(&self.current) {
                        g.contours.push(contour);
                    }
                    self.history.commit(&self.doc);
                    self.status = "Shape added.".to_string();
                } else {
                    self.history.abort();
                }
            }
            Some(_) => {
                self.history.commit(&self.doc);
            }
            None => {
                // A click that mutated without dragging (double-click insert)
                // still deserves an undo entry.
                if self.pending_change {
                    self.history.commit(&self.doc);
                    self.pending_change = false;
                } else {
                    self.history.abort();
                }
            }
        }
    }

    fn build_shape(&self, start: kurbo::Point, end: kurbo::Point) -> Option<Contour> {
        if (end - start).hypot() < 4.0 {
            return None;
        }
        let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
        let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
        Some(match self.tool {
            Tool::Rectangle => stroke::rectangle(x0, y0, x1, y1),
            Tool::Ellipse => stroke::ellipse(
                kurbo::Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                (x1 - x0) / 2.0,
                (y1 - y0) / 2.0,
            ),
            _ => return None,
        })
    }

    fn select_in_rect(&mut self, a: kurbo::Point, b: kurbo::Point) {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        self.selection.clear();
        if let Some(g) = self.doc.glyph(&self.current) {
            for (ci, contour) in g.contours.iter().enumerate() {
                for (ni, node) in contour.nodes.iter().enumerate() {
                    let p = node.pos;
                    if p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1 {
                        self.selection.push((ci, ni));
                    }
                }
            }
        }
        if self.selection.is_empty() {
            self.status = "Marquee: nothing inside.".to_string();
        }
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            self.status = "Nothing selected to delete.".to_string();
            return;
        }
        self.history.begin(&self.doc);
        self.history.touch(&self.doc, &self.current);
        let mut selected = self.selection.clone();
        selected.sort_unstable_by(|a, b| b.cmp(a)); // backwards keeps indices valid
        selected.dedup();
        if let Some(g) = self.doc.glyph_mut(&self.current) {
            for (ci, ni) in selected {
                if let Some(cont) = g.contours.get_mut(ci) {
                    if cont.nodes.len() > 2 {
                        cont.nodes.remove(ni);
                    }
                }
            }
            g.contours.retain(|c| c.nodes.len() >= 2);
        }
        self.history.commit(&self.doc);
        self.selection.clear();
        self.status = "Deleted selection.".to_string();
    }

    fn finish_open_contour(&mut self) {
        self.history.begin(&self.doc);
        self.history.touch(&self.doc, &self.current);
        if let Some(g) = self.doc.glyph_mut(&self.current) {
            if let Some(last) = g.contours.last_mut() {
                if !last.closed {
                    if last.nodes.len() < 2 {
                        g.contours.pop();
                        self.status = "Contour discarded (too few points).".to_string();
                    } else {
                        last.closed = true;
                        self.status = "Contour closed.".to_string();
                    }
                } else {
                    self.status = "No open contour.".to_string();
                }
            }
        }
        self.history.commit(&self.doc);
    }

    fn drop_stale_selection(&mut self) {
        let len = self
            .doc
            .glyph(&self.current)
            .map(|g| g.contours.len())
            .unwrap_or(0);
        self.selection.retain(|(c, _)| *c < len);
    }

    /// Position of a node, if it exists.
    fn node_pos(&self, contour: usize, node: usize) -> Option<kurbo::Point> {
        self.doc
            .glyph(&self.current)?
            .contours
            .get(contour)?
            .nodes
            .get(node)
            .map(|n| n.pos)
    }

    /// Move a set of nodes (and their handles) by `delta`.
    fn move_nodes(&mut self, targets: &[(usize, usize)], delta: kurbo::Vec2) {
        let Some(glyph) = self.doc.glyph_mut(&self.current) else { return };
        for (ci, ni) in targets {
            if let Some(n) = glyph
                .contours
                .get_mut(*ci)
                .and_then(|c| c.nodes.get_mut(*ni))
            {
                n.move_to(delta);
            }
        }
    }

    /// Toggle every selected node between corner and smooth.
    ///
    /// A corner becomes smooth by adding handles along the tangent through the
    /// node; a smooth point becomes a corner by dropping its handles. Returns
    /// the number of nodes touched.
    fn convert_selected_nodes(&mut self) -> usize {
        let selected = self.selection.clone();
        let mut count = 0;
        for (ci, ni) in &selected {
            let Some(glyph) = self.doc.glyph(&self.current) else { return count };
            let Some(contour) = glyph.contours.get(*ci) else { continue };
            let n = contour.nodes.len();
            if n < 2 {
                continue;
            }
            let Some(node) = contour.nodes.get(*ni) else { continue };

            if node.is_corner() {
                // Tangent through the node: from the previous node towards the
                // next, wrapping on closed contours.
                let prev = if *ni == 0 {
                    if contour.closed { n - 1 } else { 0 }
                } else {
                    ni - 1
                };
                let next = if *ni + 1 == n {
                    if contour.closed { 0 } else { n - 1 }
                } else {
                    *ni + 1
                };
                let (pp, np) = (contour.nodes[prev].pos, contour.nodes[next].pos);
                let tangent = np - pp;
                let len = tangent.hypot();
                if len < 1e-9 {
                    continue;
                }
                let unit = tangent / len;
                let handle_len = (node.pos - pp).hypot().min((np - node.pos).hypot()) / 3.0;
                let in_h = node.pos - unit * handle_len;
                let out_h = node.pos + unit * handle_len;
                if let Some(target) = self
                    .doc
                    .glyph_mut(&self.current)
                    .and_then(|g| g.contours.get_mut(*ci))
                    .and_then(|c| c.nodes.get_mut(*ni))
                {
                    *target = Node::smooth(target.pos, in_h, out_h);
                    count += 1;
                }
            } else {
                // Smooth → corner: drop the handles.
                if let Some(target) = self
                    .doc
                    .glyph_mut(&self.current)
                    .and_then(|g| g.contours.get_mut(*ci))
                    .and_then(|c| c.nodes.get_mut(*ni))
                {
                    *target = Node::corner(target.pos);
                    count += 1;
                }
            }
        }
        count
    }

    // ---- persistence -----------------------------------------------------

    fn project_path(&self) -> String {
        format!("{}.{}", self.doc.meta.postscript_name(), PROJECT_EXT)
    }

    fn save_project(&mut self) {
        let path = self.project_path();
        let result = fontmaker_core::project::to_json(&self.doc)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
        self.status = match result {
            Ok(()) => format!("Saved {path}"),
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn load_project(&mut self) {
        let path = self.project_path();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| fontmaker_core::project::from_json(&s));
        match result {
            Ok(doc) => {
                self.doc = doc;
                self.history.clear();
                self.selection.clear();
                if !self.doc.glyphs.contains_key(&self.current) {
                    self.current = self
                        .doc
                        .glyph_order
                        .first()
                        .cloned()
                        .unwrap_or_else(|| ".notdef".to_string());
                }
                let script = self.doc.script;
                scripts::apply_script(&mut self.doc, script);
                self.fit_pending = true;
                self.refit();
                self.status = format!("Loaded {path}");
            }
            Err(e) => self.status = format!("Load failed: {e}"),
        }
    }

    fn export(&mut self, format: ExportFormat) {
        match export(&self.doc, format) {
            Ok(bytes) => {
                let _ = std::fs::create_dir_all(OUT_DIR);
                let path = std::path::Path::new(OUT_DIR).join(format.file_name(&self.doc));
                self.status = match std::fs::write(&path, &bytes) {
                    Ok(()) => format!("Wrote {} ({} bytes)", path.display(), bytes.len()),
                    Err(e) => format!("Write failed: {e}"),
                };
            }
            Err(e) => self.status = format!("Export failed: {e}"),
        }
    }

    fn refit(&mut self) {
        let (w, h) = self.last_size;
        self.camera.fit(&self.doc, &self.current, w, h);
        self.fit_pending = false;
    }

    // ---- subscription ----------------------------------------------------

    pub fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(key_message)
    }

    // ---- view ------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let bar = panels::toolbar(self);
        let grid = panels::glyph_grid(self);
        let status = panels::status_bar(self);

        // Right column: the large clean preview of the letter, with tabbed
        // panels (inspector / font info) underneath it.
        let right = panels::right_column(self);

        let canvas_area = column![
            canvas_view::canvas_widget(self),
            panels::preview_bar(self),
        ]
        .width(iced::Length::Fill)
        .height(iced::Length::Fill);

        column![
            bar,
            row![grid, canvas_area, right]
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .spacing(4),
            status,
        ]
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .spacing(4)
        .into()
    }
}

/// Map a keyboard event to a message. Plain letter keys switch tools, so the
/// modifier check keeps Ctrl+C etc. out of the way.
fn key_message(event: keyboard::Event) -> Option<Message> {
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    let ctrl = modifiers.control();
    match key {
        Key::Named(key::Named::Delete) | Key::Named(key::Named::Backspace) => {
            Some(Message::DeleteSelection)
        }
        Key::Named(key::Named::Escape) => Some(Message::FinishContour),
        Key::Character(c) => {
            let lower = c.to_lowercase();
            match (ctrl, lower.as_str()) {
                (true, "z") => Some(if modifiers.shift() { Message::Redo } else { Message::Undo }),
                (true, "y") => Some(Message::Redo),
                (true, "s") => Some(Message::SaveProject),
                (true, "e") => Some(Message::Export(ExportFormat::Ttf)),
                (true, "0") => Some(Message::FitView),
                (true, "a") => Some(Message::SelectAllNodes),
                (true, "c") => Some(Message::CopyOutline),
                (true, "v") => Some(Message::PasteOutline),
                (true, "d") => Some(Message::DuplicateGlyph),
                (false, "v") => Some(Message::SelectTool(Tool::Select)),
                (false, "p") => Some(Message::SelectTool(Tool::Pen)),
                (false, "r") => Some(Message::SelectTool(Tool::Rectangle)),
                (false, "e") => Some(Message::SelectTool(Tool::Ellipse)),
                (false, "h") => Some(Message::SelectTool(Tool::Pan)),
                (false, "c") => Some(Message::ConvertNodes),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Which positional form should a character use, given its neighbours?
///
/// This is the naive joining pass the preview uses until the GSUB writer
/// lands: it applies the Arabic joining rule directly against the charset's
/// joining classes. Arabic only; other scripts are always isolated.
pub fn joining_form(
    prev: Option<JoiningType>,
    cur: JoiningType,
    next: Option<JoiningType>,
) -> Form {
    let prev_joins = matches!(
        prev,
        Some(JoiningType::DualJoining) | Some(JoiningType::LeftJoining)
    );
    let cur_joins_next = matches!(
        cur,
        JoiningType::DualJoining | JoiningType::LeftJoining | JoiningType::Joining
    );
    let next_joins = matches!(
        next,
        Some(JoiningType::DualJoining) | Some(JoiningType::RightJoining) | Some(JoiningType::Joining)
    );
    match (prev_joins, cur_joins_next && next_joins) {
        (false, false) => Form::Isol,
        (false, true) => Form::Init,
        (true, false) => Form::Fina,
        (true, true) => Form::Medi,
    }
}
