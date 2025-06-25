mod app;
mod completions_and_hints;
mod dict;
mod window;

use crate::app::SapfAsPlainText;
use eframe::egui::{self, Vec2, vec2};

const WINDOW_SIZE: Vec2 = vec2(680.0, 840.0);
const WINDOW_TITLE: &str = "sapf as plain* text";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(WINDOW_SIZE)
            .with_transparent(true),

        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|_| Ok(Box::new(SapfAsPlainText::new()))),
    )
}
