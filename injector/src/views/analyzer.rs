use crate::state::App;
use crate::views::placeholder;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    placeholder::draw(
        app,
        ui,
        "Analyzer",
        "Disassemble functions, trace calls, decompile IL2CPP and Mono.",
        &[
            "x86/x64 disassembly view",
            "Function call tracer",
            "IL2CPP metadata dump",
            "Mono assembly viewer",
        ],
    );
}