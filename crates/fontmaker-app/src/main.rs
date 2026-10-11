//! FontMaker: a pure-Rust typeface editor with an Illustrator-style interface.

mod app;
mod boolean;
mod canvas_view;
mod panels;

use app::{App, Message};
use iced::{Element, Task};

fn boot() -> App {
    App::new()
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    app.update(message)
}

fn view(app: &App) -> Element<'_, Message> {
    app.view()
}

fn subscription(app: &App) -> iced::Subscription<Message> {
    app.subscription()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .subscription(subscription)
        .title("FontMaker")
        .theme(iced::Theme::Dark)
        .window(iced::window::Settings {
            size: iced::Size::new(1400.0, 900.0),
            min_size: Some(iced::Size::new(1000.0, 640.0)),
            ..Default::default()
        })
        .run()
}
