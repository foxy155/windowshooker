#![windows_subsystem = "windows"]

mod app;
mod core;
mod shell;
mod state;
mod theme;
mod views;
#[cfg(test)]
mod tests;
mod script_tests;

use state::App;
use theme::{install, Theme};

fn main() -> eframe::Result<()> {
    install(Theme::default());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 820.0])
            .with_min_inner_size([780.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sigil",
        options,
        Box::new(|_cc| Box::new(App::new()) as Box<dyn eframe::App>),
    )
}