use crate::state::{App, View};
use crate::views;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    match app.view {
        View::Dashboard => views::dashboard::draw(app, ui),
        View::Injector => views::injector::draw(app, ui),
        View::Packets => views::packets::draw(app, ui),
        View::Hooks => views::hooks::draw(app, ui),
        View::Memory => views::memory::draw(app, ui),
        View::Analyzer => views::analyzer::draw(app, ui),
        View::Sessions => views::session::draw(app, ui),
        View::Settings => views::settings::draw(app, ui),
    }
}