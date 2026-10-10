//! The glyph editing canvas.
//!
//! The canvas owns no document state: it translates input events into
//! [`CanvasMsg`]s in font units and lets `App` decide what they mean. Hit
//! testing happens in `App` so the tools stay in one place.

use fontmaker_core::glyph::Contour;
use fontmaker_core::JoiningType;

use iced::widget::canvas::{
    self, Action, Canvas, Frame, Geometry, Path as CanvasPath, Program, Stroke,
};
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};

use crate::app::{App, CanvasMsg, Message, Tool};

/// Colors.
const BACKGROUND: Color = Color::from_rgb8(0x1e, 0x1e, 0x22);
const PREVIEW_BG: Color = Color::from_rgb8(0x17, 0x17, 0x1b);
const BASELINE: Color = Color::from_rgb8(0x3d, 0x6a, 0x7a);
const METRIC_LINE: Color = Color::from_rgb8(0x27, 0x3c, 0x46);
const INK: Color = Color::from_rgb8(0xdd, 0xdd, 0xdd);
const INK_PREVIEW: Color = Color::from_rgb8(0x55, 0x60, 0x66);
const NODE: Color = Color::from_rgb8(0x4f, 0xc3, 0xf7);
const NODE_SELECTED: Color = Color::from_rgb8(0xff, 0xeb, 0x3b);
const HANDLE: Color = Color::from_rgb8(0xff, 0x98, 0x00);
const MARQUEE_FILL: Color = Color::from_rgba8(0x4f, 0xc3, 0xf7, 0.12);
const MARQUEE_LINE: Color = Color::from_rgba8(0x4f, 0xc3, 0xf7, 0.6);
const SIDE_BEARING: Color = Color::from_rgb8(0x5a, 0x3a, 0x46);

/// A borrowed view of the application, rebuilt every frame.
pub struct GlyphCanvas<'a> {
    pub app: &'a App,
}

/// Transient canvas state: whether a button is currently held down, and the
/// last size we reported so resize messages are only sent on actual changes.
#[derive(Default)]
pub struct CanvasState {
    pressed: bool,
    reported_size: Option<(f32, f32)>,
}

/// The cursor position in *canvas-local* coordinates.
///
/// The runtime hands us window-space positions; the canvas draws in a frame
/// translated by `bounds.x/y`, so without this subtraction every point lands
/// offset by the toolbar and the glyph grid.
fn local_cursor(cursor: iced::mouse::Cursor, bounds: Rectangle) -> Option<Point> {
    cursor
        .position()
        .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
}

impl Program<Message, Theme, iced::Renderer> for GlyphCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<Action<Message>> {
        // Report the canvas size when it changes so the app can fit the view.
        // Redraws happen every frame, making this a cheap place to check.
        if let canvas::Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            let size = (bounds.width, bounds.height);
            if state.reported_size != Some(size) {
                state.reported_size = Some(size);
                return Some(Action::publish(Message::Canvas(CanvasMsg::Size {
                    width: size.0,
                    height: size.1,
                })));
            }
            return None;
        }

        let canvas::Event::Mouse(mouse_event) = event else {
            return None;
        };

        match mouse_event {
            iced::mouse::Event::ButtonPressed(button) => {
                let pos = local_cursor(cursor, bounds)?;
                state.pressed = true;
                let right = matches!(
                    button,
                    iced::mouse::Button::Right | iced::mouse::Button::Middle
                );
                Some(Action::publish(Message::Canvas(CanvasMsg::Pressed {
                    font: self.app.camera.to_font(pos),
                    screen: pos,
                    right_click: right,
                })))
            }
            iced::mouse::Event::ButtonReleased(_) => {
                if state.pressed {
                    state.pressed = false;
                    Some(Action::publish(Message::Canvas(CanvasMsg::Released)))
                } else {
                    None
                }
            }
            iced::mouse::Event::CursorMoved { .. } => {
                let pos = local_cursor(cursor, bounds)?;
                let font = self.app.camera.to_font(pos);
                let msg = if state.pressed {
                    Message::Canvas(CanvasMsg::Dragged { font, screen: pos })
                } else {
                    Message::Canvas(CanvasMsg::Moved { font })
                };
                Some(Action::publish(msg))
            }
            iced::mouse::Event::WheelScrolled { delta } => {
                let pos = local_cursor(cursor, bounds)?;
                let y = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => *y,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if y == 0.0 {
                    None
                } else {
                    Some(Action::publish(Message::Canvas(CanvasMsg::Scrolled {
                        screen: pos,
                        up: y > 0.0,
                    })))
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cam = &self.app.camera;

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), BACKGROUND);

        if let Some(glyph) = self.app.doc.glyph(&self.app.current) {
            draw_guides(&mut frame, cam, glyph.advance, &self.app.doc);
            draw_glyph(&mut frame, cam, glyph);
            draw_nodes(&mut frame, cam, glyph, &self.app.selection);
            draw_overlays(&mut frame, cam, self.app);
        } else {
            draw_guides(&mut frame, cam, 600.0, &self.app.doc);
        }

        draw_preview(&mut frame, cam, self.app);

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        match self.app.tool {
            Tool::Pan => iced::mouse::Interaction::Grabbing,
            Tool::Pen => iced::mouse::Interaction::Crosshair,
            _ => iced::mouse::Interaction::default(),
        }
    }
}

/// Draw a contour into a canvas path, in screen coordinates.
pub(crate) fn contour_path(cam: &crate::app::Camera, contour: &Contour) -> CanvasPath {
    let mut builder = canvas::path::Builder::new();
    let Some(first) = contour.nodes.first() else {
        return builder.build();
    };
    builder.move_to(cam.to_screen(first.pos));
    let n = contour.nodes.len();
    let last = if contour.closed { n } else { n - 1 };
    for i in 0..last {
        let a = &contour.nodes[i];
        let b = &contour.nodes[(i + 1) % n];
        match (a.out_handle, b.in_handle) {
            (Some(o), Some(i_h)) => builder.bezier_curve_to(
                cam.to_screen(o),
                cam.to_screen(i_h),
                cam.to_screen(b.pos),
            ),
            (Some(o), None) => {
                builder.quadratic_curve_to(cam.to_screen(o), cam.to_screen(b.pos))
            }
            (None, Some(i_h)) => {
                builder.quadratic_curve_to(cam.to_screen(i_h), cam.to_screen(b.pos))
            }
            (None, None) => builder.line_to(cam.to_screen(b.pos)),
        }
    }
    if contour.closed {
        builder.close();
    }
    builder.build()
}

pub(crate) fn draw_guides(frame: &mut Frame, cam: &crate::app::Camera, advance: f64, doc: &fontmaker_core::Document) {
    // Vertical metrics, spanning a little beyond the advance width.
    let x0 = cam.to_screen(kurbo::Point::new(-150.0, 0.0)).x;
    let x1 = cam.to_screen(kurbo::Point::new(advance + 150.0, 0.0)).x;

    for (label, y) in doc.metrics.guides() {
        let is_baseline = label == "baseline";
        let screen_y = cam.to_screen(kurbo::Point::new(0.0, y)).y;
        let color = if is_baseline { BASELINE } else { METRIC_LINE };
        frame.stroke(
            &CanvasPath::line(Point::new(x0, screen_y), Point::new(x1, screen_y)),
            Stroke::default().with_color(color).with_width(1.0),
        );
    }

    // Advance width boundaries.
    for x in [0.0, advance] {
        let sx = cam.to_screen(kurbo::Point::new(x, 0.0)).x;
        let top = cam.to_screen(kurbo::Point::new(0.0, doc.metrics.ascender + 100.0)).y;
        let bottom = cam.to_screen(kurbo::Point::new(0.0, doc.metrics.descender - 100.0)).y;
        frame.stroke(
            &CanvasPath::line(Point::new(sx, top), Point::new(sx, bottom)),
            Stroke::default().with_color(SIDE_BEARING).with_width(1.0),
        );
    }
}

pub(crate) fn draw_glyph(frame: &mut Frame, cam: &crate::app::Camera, glyph: &fontmaker_core::Glyph) {
    // Even-odd so counters (holes) render no matter which order the
    // contours were drawn in.
    for contour in &glyph.contours {
        if contour.nodes.is_empty() {
            continue;
        }
        let path = contour_path(cam, contour);
        frame.fill(
            &path,
            canvas::Fill {
                style: canvas::Style::Solid(INK),
                rule: canvas::fill::Rule::EvenOdd,
            },
        );
    }
    // Show open contours as outlines while drawing.
    for contour in &glyph.contours {
        if !contour.closed {
            frame.stroke(
                &contour_path(cam, contour),
                Stroke::default().with_color(NODE).with_width(1.5),
            );
        }
    }
}

fn draw_nodes(
    frame: &mut Frame,
    cam: &crate::app::Camera,
    glyph: &fontmaker_core::Glyph,
    selection: &[(usize, usize)],
) {
    let is_selected = |c: usize, n: usize| selection.contains(&(c, n));

    // Handles first, so nodes draw on top.
    for (ci, contour) in glyph.contours.iter().enumerate() {
        for (ni, node) in contour.nodes.iter().enumerate() {
            let pos = cam.to_screen(node.pos);
            for handle in [node.in_handle, node.out_handle].into_iter().flatten() {
                let hp = cam.to_screen(handle);
                frame.stroke(
                    &CanvasPath::line(pos, hp),
                    Stroke::default().with_color(HANDLE).with_width(1.0),
                );
                frame.fill(&CanvasPath::circle(hp, 3.0), HANDLE);
            }
            let _ = (ci, ni, is_selected);
        }
    }

    for (ci, contour) in glyph.contours.iter().enumerate() {
        for (ni, node) in contour.nodes.iter().enumerate() {
            let pos = cam.to_screen(node.pos);
            let color = if is_selected(ci, ni) { NODE_SELECTED } else { NODE };
            let size = if is_selected(ci, ni) { 8.0 } else { 6.0 };
            frame.fill_rectangle(
                Point::new(pos.x - size / 2.0, pos.y - size / 2.0),
                Size::new(size, size),
                color,
            );
        }
    }
}

fn draw_overlays(frame: &mut Frame, cam: &crate::app::Camera, app: &App) {
    // Marquee or in-progress shape.
    if let Some((a, b)) = app.marquee {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let tl = cam.to_screen(kurbo::Point::new(x0, y1));
        let size = Size::new(
            cam.to_screen(kurbo::Point::new(x1, y0)).x - tl.x,
            cam.to_screen(kurbo::Point::new(x1, y0)).y - tl.y,
        );
        if app.tool == Tool::Rectangle || app.tool == Tool::Ellipse {
            // Live shape preview.
            let contour = match app.tool {
                Tool::Rectangle => {
                    fontmaker_core::stroke::rectangle(x0, y0, x1, y1)
                }
                _ => fontmaker_core::stroke::ellipse(
                    kurbo::Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                    (x1 - x0) / 2.0,
                    (y1 - y0) / 2.0,
                ),
            };
            frame.stroke(
                &contour_path(cam, &contour),
                Stroke::default().with_color(NODE_SELECTED).with_width(1.5),
            );
        } else {
            frame.fill_rectangle(tl, size, MARQUEE_FILL);
            frame.stroke_rectangle(tl, size, Stroke::default().with_color(MARQUEE_LINE).with_width(1.0));
        }
    }

    // Pen tool: rubber band from the last node to the cursor.
    if app.tool == Tool::Pen {
        if let Some(glyph) = app.doc.glyph(&app.current) {
            if let Some(last) = glyph.contours.last() {
                if !last.closed && !last.nodes.is_empty() {
                    let from = cam.to_screen(last.nodes.last().unwrap().pos);
                    let to = cam.to_screen(app.cursor_font);
                    frame.stroke(
                        &CanvasPath::line(from, to),
                        Stroke::default().with_color(NODE).with_width(1.0),
                    );
                }
            }
        }
    }
}

/// Draw the sample sentence with the outlines actually in the document.
///
/// This is shaping-lite: Latin renders left to right, right-to-left scripts
/// reverse their characters, and Arabic picks init/medi/fina/isol forms with
/// the joining rule. It is not HarfBuzz, but it shows whether letters join.
fn draw_preview(frame: &mut Frame, cam: &crate::app::Camera, app: &App) {
    if app.preview.trim().is_empty() {
        return;
    }

    let scale = 0.35;
    let baseline_font_y = app.doc.metrics.descender - 120.0;
    let chars: Vec<char> = if app.doc.script.is_rtl() {
        app.preview.chars().rev().collect()
    } else {
        app.preview.chars().collect()
    };

    let mut pen_x = 50.0;
    let mut prev_name: Option<String> = None;
    let mut prev_joining: Option<JoiningType> = None;

    for (i, ch) in chars.iter().enumerate() {
        let Some(entry) = app
            .doc
            .charset
            .iter()
            .find(|e| e.codepoint == *ch as u32)
            .cloned()
        else {
            prev_name = None;
            prev_joining = None;
            continue;
        };

        // Pick the positional form for joining scripts.
        let next_joining = chars
            .get(i + 1)
            .and_then(|next| {
                app.doc
                    .charset
                    .iter()
                    .find(|e| e.codepoint == *next as u32)
                    .map(|e| e.joining)
            });
        let form = if app.doc.script.uses_forms() {
            crate::app::joining_form(prev_joining, entry.joining, next_joining)
        } else {
            fontmaker_core::Form::Isol
        };

        let mut name = entry.glyph.clone();
        if form != fontmaker_core::Form::Isol {
            let with_form = format!("{name}{}", form.suffix());
            if app.doc.glyphs.contains_key(&with_form) {
                name = with_form;
            }
        }

        if let Some(glyph) = app.doc.glyph(&name) {
            for contour in &glyph.contours {
                if contour.nodes.is_empty() {
                    continue;
                }
                // Transform into preview space: scale down and lay out on the
                // preview baseline.
                let preview_contour = {
                    let mut nodes = contour.nodes.clone();
                    for node in &mut nodes {
                        let p = kurbo::Point::new(
                            pen_x + node.pos.x * scale,
                            baseline_font_y + node.pos.y * scale,
                        );
                        node.pos = p;
                        if let Some(h) = &mut node.in_handle {
                            h.x = pen_x + h.x * scale;
                            h.y = baseline_font_y + h.y * scale;
                        }
                        if let Some(h) = &mut node.out_handle {
                            h.x = pen_x + h.x * scale;
                            h.y = baseline_font_y + h.y * scale;
                        }
                    }
                    Contour::new(nodes, contour.closed)
                };
                frame.fill(
                    &contour_path(cam, &preview_contour),
                    canvas::Fill {
                        style: canvas::Style::Solid(INK_PREVIEW),
                        rule: canvas::fill::Rule::EvenOdd,
                    },
                );
            }
        }

        let kern = prev_name
            .as_deref()
            .map(|p| app.doc.kern_value(p, &entry.glyph))
            .unwrap_or(0.0);
        pen_x += (glyph_advance(app, &entry.glyph) + kern) * scale;
        prev_name = Some(entry.glyph.clone());
        prev_joining = Some(entry.joining);
    }
}

fn glyph_advance(app: &App, name: &str) -> f64 {
    app.doc.glyph(name).map(|g| g.advance).unwrap_or(500.0)
}

/// The canvas element sized to fill its container.
pub fn canvas_widget(app: &App) -> Element<'_, Message> {
    Canvas::new(GlyphCanvas { app })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
/// A large, clean render of the current glyph: fill only, no nodes or handles.
///
/// Read-only, so it keeps no state and publishes no messages: the fit is
/// recomputed from the panel's own bounds every frame.
pub struct PreviewCanvas<'a> {
    pub app: &'a App,
}

impl Program<Message, Theme, iced::Renderer> for PreviewCanvas<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Option<Action<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), PREVIEW_BG);

        let doc = &self.app.doc;
        let glyph = doc.glyph(&self.app.current);
        let ink = glyph.and_then(|g| g.bounds());

        // Frame the ink, but always include the baseline and cap height so the
        // letter is seen in the context of its metrics.
        let (x0, y0, x1, y1) = match ink {
            Some(b) => (
                b.x0.min(-20.0),
                b.y0.min(0.0),
                b.x1.max(glyph.map(|g| g.advance).unwrap_or(0.0)),
                b.y1.max(doc.metrics.cap_height),
            ),
            None => (
                -50.0,
                doc.metrics.descender,
                glyph.map(|g| g.advance).unwrap_or(550.0),
                doc.metrics.cap_height,
            ),
        };
        let pad = 28.0;
        let fw = ((x1 - x0).max(100.0)) as f32;
        let fh = ((y1 - y0).max(100.0)) as f32;
        let scale = ((bounds.width - 2.0 * pad) / fw)
            .min((bounds.height - 2.0 * pad) / fh)
            .clamp(0.01, 10.0);
        let cam = crate::app::Camera {
            scale,
            origin_x: bounds.width / 2.0 - ((x0 + x1) / 2.0) as f32 * scale,
            origin_y: bounds.height / 2.0 + ((y0 + y1) / 2.0) as f32 * scale,
        };

        draw_guides(&mut frame, &cam, glyph.map(|g| g.advance).unwrap_or(600.0), doc);
        if let Some(g) = glyph {
            draw_glyph(&mut frame, &cam, g);
        }

        vec![frame.into_geometry()]
    }
}

/// The preview element, sized by its container.
pub fn preview_canvas(app: &App) -> Element<'_, Message> {
    Canvas::new(PreviewCanvas { app })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}