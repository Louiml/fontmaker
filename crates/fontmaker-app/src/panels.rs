//! Side panels: toolbar, glyph grid, inspector, preview bar and status bar.

use fontmaker_core::document::Script;
use fontmaker_export::ExportFormat;

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, text_input};
use iced::{Element, Length, Padding};

use crate::app::{App, Message, Tool};

const PANEL_BG: iced::Color = iced::Color::from_rgb8(0x26, 0x26, 0x2c);

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Shrink)
        .height(Length::Fill)
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
    ].spacing(2);

    let export = row![
        text("Export:").size(13),
        button(text(".ttf").size(13)).on_press(Message::Export(ExportFormat::Ttf)).padding(Padding::from(6)),
        button(text(".woff").size(13)).on_press(Message::Export(ExportFormat::Woff)).padding(Padding::from(6)),
        button(text(".woff2").size(13)).on_press(Message::Export(ExportFormat::Woff2)).padding(Padding::from(6)),
    ].spacing(2).push(text("").size(4));

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
        .width(220)
        .height(Length::Fill),
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
                format!("{short}׳³ֲ³׳’ג‚¬ג„¢׳³ג€™׳’ג‚¬ֲײ²ֲ¬׳²ֲ²ײ²ֲ¦")
            } else {
                short
            }
        }
    };

    let marker = if is_current { "׳³ֲ³׳’ג‚¬ג„¢׳³ג€™׳’ג€ֲ¬׳’ג‚¬ֲ׳²ֲ²ײ²ֲ¶" } else if drawn { "׳³ֲ³׳’ג‚¬ג„¢׳³ג€™׳’ג€ֲ¬׳’ג‚¬ֲ׳²ֲ²ײ²ֲ" } else { " " };

    button(
        column![
            text(label).size(15),
            text(marker).size(10),
        ]
        .align_x(iced::alignment::Horizontal::Center)
        .spacing(1),
    )
    .on_press(Message::SelectGlyph(name.to_string()))
    .width(40)
    .padding(Padding::from(3))
    .into()
}

/// Right-hand inspector: metrics for the current glyph plus glyph management.
pub fn inspector(app: &App) -> Element<'_, Message> {
    let Some(glyph) = app.doc.glyph(&app.current) else {
        return panel(text("No glyph selected.").size(13));
    };

    let char_info = glyph
        .unicode
        .first()
        .map(|c| format!("U+{:04X}  {}", *c as u32, c))
        .unwrap_or_else(|| "unencoded".to_string());

    let status = if glyph.is_drawn() { "drawn" } else { "empty" };

    let advance_row = row![
        text("advance").size(13),
        slider(0.0..=2000.0, glyph.advance, Message::SetAdvance)
            .width(120),
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
                format!("{}{}", f.feature_tag(), if drawn { " ׳³ֲ³׳’ג‚¬ג„¢׳²ֲ²ײ²ֲ׳³ג€™׳’ג€ֲ¬ײ²ֲ" } else { " ׳³ֲ³׳’ג‚¬ג„¢׳²ֲ²ײ²ֲ׳³ג€™׳’ג€ֲ¬׳’ג‚¬ֲ" })
            })
            .collect();
        text(format!("forms: {}", drawn_forms.join("  "))).size(13)
    } else {
        text(String::new()).size(13)
    };

    panel(
        column![
            text("Inspector").size(14),
            text(&glyph.name).size(16),
            text(char_info).size(13),
            text(status).size(13),
            advance_row,
            contours,
            forms,
            iced::widget::Space::new().height(Length::Fill),
            button(text("Delete glyph").size(13))
                .on_press(Message::DeleteGlyph)
                .padding(Padding::from(6)),
        ]
        .spacing(6)
        .width(200)
        .height(Length::Fill),
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
