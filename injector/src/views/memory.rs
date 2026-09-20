use crate::state::App;
use crate::views::placeholder;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    placeholder::draw(
        app,
        ui,
        "Memory",
        "Read, write, and scan the target process's address space.",
        &[
            "Region browser with protections",
            "Value scans (int32, float, string)",
            "Pointer chain finder",
            "Freeze and modify",
        ],
    );
}