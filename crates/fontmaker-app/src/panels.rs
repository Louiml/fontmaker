//! Side panels: toolbar, glyph grid, inspector, font info, preview bar and
//! status bar.

use fontmaker_core::document::Script;
use fontmaker_export::ExportFormat;

use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text, text_input,
};
use iced::{Element, Length, Padding};

use crate::app::{App, Message, MetaField, RightTab, Tool};

const PANEL_BG: iced::Color = iced::Color::from_rgb8(0x26, 0x26, 0x2c);

fn panel<'a>(content: impl Into<Element<'a, Message>>, height: Length) -> Element<'a, Message> {
    container(content)
        .width(Length::Shrink)
        .height(height)
        .padding(6)
        .style(|_| container::Style {
            background: Some(PANEL_BG.into()),
            ..Default::default()
        })
        .into()
}

/// Top bar: script selector, tools, file actions, export actions.
pub fn toolbar(app: &App) -> Element<'_, Message> {
    let script_pick = pick_list(Script::all(), Some(app.doc.script), Message::SelectScript)
        .width(120)
        .padding(4);

    let mut tools = row![].spacing(2);
    for tool in Tool::ALL {
        let selected = app.tool == tool;
        let label = if selected {
            format!("[{}]", tool.short())
        } else {
            format!(" {}", tool.short())
        };
        tools = tools.push(
            button(text(label).size(14))
                .on_press(Message::SelectTool(tool))
                .padding(Padding::from(6)),
        );
    }

    let file = row![
        button(text("Undo").size(13)).on_press_maybe(
            if app.history.can_undo() { Some(Message::Undo) } else { None }
        ).padding(Padding::from(6)),
        button(text("Redo").size(13)).on_press_maybe(
            if app.history.can_redo() { Some(Message::Redo) } else { None }
        ).padding(Padding::from(6)),
        button(text("Save").size(13)).on_press(Message::SaveProject).padding(Padding::from(6)),
        button(text("Load").size(13)).on_press(Message::LoadProject).padding(Padding::from(6)),
        button(text("Fit").size(13)).on_press(Message::FitView).padding(Padding::from(6)),
        button(text(if app.snap { "Snap: on" } else { "Snap: off" }).size(13))
            .on_press(Message::ToggleSnap)
            .padding(Padding::from(6)),
    ].spacing(2);

    let export = row![
        text("Export:").size(13),
        button(text(".ttf").size(13)).on_press(Message::Export(ExportFormat::Ttf)).padding(Padding::from(6)),
        button(text(".woff").size(13)).on_press(Message::Export(ExportFormat::Woff)).padding(Padding::from(6)),
        button(text(".woff2").size(13)).on_press(Message::Export(ExportFormat::Woff2)).padding(Padding::from(6)),
    ].spacing(2);

    row![
        text("FontMaker").size(16),
        script_pick,
        tools,
        file,
        export,
    ]
    .spacing(10)
    .padding(Padding { top: 6.0, bottom: 6.0, left: 8.0, right: 8.0 })
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// The glyph grid: every glyph in the document as a clickable tile.
pub fn glyph_grid(app: &App) -> Element<'_, Message> {
    let per_row = 5;
    let mut rows = column![].spacing(2);

    for chunk in app.doc.glyph_order.chunks(per_row) {
        let mut r = row![].spacing(2);
        for name in chunk {
            r = r.push(glyph_tile(app, name));
        }
        rows = rows.push(r);
    }

    let header = text("Glyphs").size(14);
    let add = row![
        text_input("name", &app.new_glyph_name)
            .on_input(Message::NewGlyphName)
            .on_submit(Message::AddGlyph)
            .width(110)
            .size(13),
        button(text("+").size(14)).on_press(Message::AddGlyph).padding(Padding::from(4)),
    ]
    .spacing(4);

    panel(
        column![
            header,
            add,
            scrollable(rows).height(Length::Fill),
        ]
        .spacing(6)
        .width(220),
        Length::Fill,
    )
}

fn glyph_tile<'a>(app: &'a App, name: &str) -> Element<'a, Message> {
    let glyph = app.doc.glyph(name);
    let drawn = glyph.map(|g| g.is_drawn()).unwrap_or(false);
    let is_current = app.current == name;

    let label = match glyph.and_then(|g| g.unicode.first()) {
        Some(c) if *c != '\0' => {
            if name.contains('.') {
                // Positional form: show the suffix so forms are distinguishable.
                let suffix = name.split('.').nth(1).unwrap_or("");
                format!("{c}\n.{suffix}")
            } else {
                format!("{c}")
            }
        }
        _ => {
            let short: String = name.chars().take(8).collect();
            if name.chars().count() > 8 {
                format!("{short}..")
            } else {
                short
            }
        }
    };

    let marker = if is_current {
        "selected"
    } else if drawn {
        "drawn"
    } else {
        ""
    };

    button(
        column![
            text(label).size(15),
            text(marker).size(9),
        ]
        .align_x(iced::alignment::Horizontal::Center)
        .spacing(1),
    )
    .on_press(Message::SelectGlyph(name.to_string()))
    .width(42)
    .padding(Padding::from(3))
    .into()
}

/// The right column: large preview on top, tab buttons, then the active panel.
pub fn right_column(app: &App) -> Element<'_, Message> {
    let tabs = row![
        button(text("Glyph").size(13))
            .on_press(Message::SelectTab(RightTab::Inspector))
            .padding(Padding::from(5)),
        button(text("Kerning").size(13))
            .on_press(Message::SelectTab(RightTab::Kerning))
            .padding(Padding::from(5)),
        button(text("Font info").size(13))
            .on_press(Message::SelectTab(RightTab::Info))
            .padding(Padding::from(5)),
    ]
    .spacing(4);

    let content = match app.right_tab {
        RightTab::Inspector => inspector(app),
        RightTab::Info => font_info(app),
        RightTab::Kerning => kerning_panel(app),
    };

    column![
        crate::canvas_view::preview_canvas(app),
        tabs,
        content,
    ]
    .width(Length::Shrink)
    .height(Length::Fill)
    .spacing(4)
    .into()
}

/// The glyph inspector: metrics, editing actions, glyph management.
pub fn inspector(app: &App) -> Element<'_, Message> {
    let Some(glyph) = app.doc.glyph(&app.current) else {
        return panel(text("No glyph selected.").size(13), Length::Fill);
    };

    let char_info = glyph
        .unicode
        .first()
        .map(|c| format!("U+{:04X}  {}", *c as u32, c))
        .unwrap_or_else(|| "unencoded".to_string());

    let status = if glyph.is_drawn() { "drawn" } else { "empty" };

    // Sidebearings from the actual ink, not the stored lsb field.
    let sidebearings = match glyph.bounds() {
        Some(b) => format!("LSB {:.0}   RSB {:.0}", b.x0, glyph.advance - b.x1),
        None => "LSB -   RSB -".to_string(),
    };

    let advance_row = row![
        text("advance").size(13),
        slider(0.0..=2000.0, glyph.advance, Message::SetAdvance).width(110),
        text(format!("{:.0}", glyph.advance)).size(13),
    ]
    .spacing(6)
    .align_y(iced::alignment::Vertical::Center);

    let contours = text(format!(
        "{} contour(s), {} node(s)",
        glyph.contours.len(),
        glyph.contours.iter().map(|c| c.nodes.len()).sum::<usize>()
    ))
    .size(13);

    let forms = if app.doc.script.uses_forms() {
        let names = app.doc.form_names(&glyph.name);
        let drawn_forms: Vec<String> = names
            .iter()
            .filter(|(f, _n)| *f != fontmaker_core::Form::Isol)
            .map(|(f, n)| {
                let drawn = app.doc.glyph(n).map(|g| g.is_drawn()).unwrap_or(false);
                format!("{}: {}", f.feature_tag(), if drawn { "drawn" } else { "todo" })
            })
            .collect();
        text(format!("forms: {}", drawn_forms.join(", "))).size(12)
    } else {
        text(String::new()).size(12)
    };

    let metric_buttons = row![
        button(text("Auto advance").size(12))
            .on_press(Message::AutoAdvance)
            .padding(Padding::from(5)),
        button(text("Duplicate").size(12))
            .on_press(Message::DuplicateGlyph)
            .padding(Padding::from(5)),
    ]
    .spacing(4);

    let edit_buttons = row![
        button(text("Corner/Smooth").size(12))
            .on_press(Message::ConvertNodes)
            .padding(Padding::from(5)),
        button(text("Flip H").size(12)).on_press(Message::FlipH).padding(Padding::from(5)),
        button(text("Flip V").size(12)).on_press(Message::FlipV).padding(Padding::from(5)),
    ]
    .spacing(4);

    panel(
        column![
            text("Inspector").size(14),
            text(&glyph.name).size(16),
            text(char_info).size(13),
            text(status).size(13),
            text(sidebearings).size(13),
            advance_row,
            contours,
            forms,
            metric_buttons,
            edit_buttons,
            button(text("Delete glyph").size(12))
                .on_press(Message::DeleteGlyph)
                .padding(Padding::from(5)),
        ]
        .spacing(6)
        .width(210),
        Length::Fill,
    )
}

/// The kerning pair editor.
pub fn kerning_panel(app: &App) -> Element<'_, Message> {
    let glyphs: Vec<String> = app.doc.glyph_order.clone();

    let left_pick = pick_list(glyphs.clone(), Some(app.kern_left.clone()), Message::SelectKernLeft)
        .width(190)
        .padding(4);
    let right_pick = pick_list(glyphs, Some(app.kern_right.clone()), Message::SelectKernRight)
        .width(190)
        .padding(4);

    let value = app.doc.kern_value(&app.kern_left, &app.kern_right);
    let slider_row = row![
        text("kern").size(13),
        slider(-500.0..=500.0, value, Message::KernValueChanged).width(110),
        text(format!("{value:.0}")).size(13),
    ]
    .spacing(6)
    .align_y(iced::alignment::Vertical::Center);

    // Existing pairs, most recently added last. Clicking one loads it into
    // the editors above.
    let mut rows = column![].spacing(2);
    for (i, (l, r, v)) in app.doc.kerning.iter().enumerate() {
        let selected = app.kern_selected == Some(i);
        let label = if selected {
            format!("[{l} {r}]  {v:.0}")
        } else {
            format!(" {l} {r}   {v:.0}")
        };
        rows = rows.push(
            button(text(label).size(13))
                .on_press(Message::SelectKernPair(i))
                .width(190)
                .padding(Padding::from(3)),
        );
    }
    let pair_count = app.doc.kerning.len();

    let delete = button(text("Delete pair").size(12))
        .on_press_maybe(if app.kern_selected.is_some() { Some(Message::DeleteKernPair) } else { None })
        .padding(Padding::from(5));

    panel(
        column![
            text("Kerning").size(14),
            text("left").size(11),
            left_pick,
            text("right").size(11),
            right_pick,
            slider_row,
            crate::canvas_view::kern_canvas(app),
            delete,
            text(format!("{} pair(s)", pair_count)).size(12),
            scrollable(rows).height(Length::Fill),
        ]
        .spacing(5)
        .width(210),
        Length::Fill,
    )
}

/// The font metadata editor.
pub fn font_info(app: &App) -> Element<'_, Message> {
    let meta_input = |label: &'static str, value: &str, field: MetaField| {
        text_input(label, value)
            .on_input(move |s| Message::MetaChanged(field, s))
            .size(13)
            .width(190)
    };

    panel(
        column![
            text("Font Info").size(14),
            text("family").size(11),
            meta_input("Family name", &app.doc.meta.family_name, MetaField::Family),
            text("style").size(11),
            meta_input("Style name", &app.doc.meta.style_name, MetaField::Style),
            text("designer").size(11),
            meta_input("Designer", &app.doc.meta.designer, MetaField::Designer),
            text("version").size(11),
            meta_input("Version", &app.doc.meta.version, MetaField::Version),
            text(format!(
                "PostScript name:\n{}",
                app.doc.meta.postscript_name()
            ))
            .size(12),
            text("Used for exports and the project file name.").size(11),
        ]
        .spacing(4)
        .width(210),
        Length::Fill,
    )
}

/// The preview text input, below the canvas.
pub fn preview_bar(app: &App) -> Element<'_, Message> {
    row![
        text("Preview:").size(13),
        text_input("Type sample text", &app.preview)
            .on_input(Message::PreviewChanged)
            .width(Length::Fill)
            .size(14),
    ]
    .spacing(8)
    .padding(Padding { top: 4.0, bottom: 4.0, left: 8.0, right: 8.0 })
    .into()
}

/// The status line at the bottom.
pub fn status_bar(app: &App) -> Element<'_, Message> {
    let (done, total) = app.doc.completion();
    let pct = (done * 100).checked_div(total).unwrap_or(100);
    let zoom = format!("{:.0}%", app.camera.scale * 100.0);

    row![
        text(&app.status).size(13).width(Length::Fill),
        text(format!("{done}/{total} glyphs ({pct}%)")).size(13),
        text(format!("zoom {zoom}")).size(13),
    ]
    .spacing(16)
    .padding(Padding { top: 4.0, bottom: 4.0, left: 8.0, right: 8.0 })
    .into()
}
