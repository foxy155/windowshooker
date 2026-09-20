use eframe::App as EframeApp;
use std::sync::{Arc, Mutex};

use crate::core::code_editor::EditorState;
use crate::shell::router;
use crate::shell::sidebar::draw_sidebar;
use crate::shell::topbar;
use crate::state::App;
use crate::theme;

fn editor_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("sigil_editor_viewport")
}

impl EframeApp for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.viewport_id() == editor_viewport_id() {
            draw_editor_viewport(self, ctx);
        } else {
            draw_main_viewport(self, ctx);
        }
    }
}

fn draw_main_viewport(app: &mut App, ctx: &egui::Context) {
    theme::apply_theme(ctx);

    app.pick_up_background_scan();

    draw_sidebar(app, ctx);

    egui::TopBottomPanel::top("topbar")
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            topbar::draw(app, ui);
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(theme::bg())
                .inner_margin(egui::Margin::same(20.0)),
        )
        .show(ctx, |ui| {
            router::draw(app, ui);
        });

    let (floating, has_tabs) = {
        let e = app.editor();
        (e.floating, !e.tabs.is_empty())
    };

    if floating && has_tabs {
        spawn_editor_viewport(app, ctx);
    }

    if ctx.input(|i| i.viewport().close_requested()) {
        app.shutdown_injected_dlls();
    }

    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

fn spawn_editor_viewport(app: &mut App, ctx: &egui::Context) {
    let builder = egui::ViewportBuilder::default()
        .with_title("Sigil — Editor")
        .with_inner_size([900.0, 600.0])
        .with_min_inner_size([400.0, 300.0]);

    let id = editor_viewport_id();

    // We only need to hand the editor Arc to the closure. App itself
    // stays inside the main update. When the OS window is closed we
    // set floating = false via the shared Arc.
    let editor: Arc<Mutex<EditorState>> = Arc::clone(&app.editor);

    let _ = ctx.show_viewport_deferred(id, builder, move |viewport_ctx, _class| {
        if viewport_ctx.input(|i| i.viewport().close_requested()) {
            if let Ok(mut e) = editor.lock() {
                e.floating = false;
            }
        }

        draw_editor_in_viewport(viewport_ctx, Arc::clone(&editor));
    });
}

/// Render the editor's OS window content. We can't access `App` here,
/// so we render a minimal shell that shares state via the editor Arc.
fn draw_editor_in_viewport(ctx: &egui::Context, editor: Arc<Mutex<EditorState>>) {
    theme::apply_theme(ctx);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(theme::bg())
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show(ctx, |ui| {
            crate::views::hooks::draw_editor_with_shared_state(ui, editor.clone());
        });

    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// Main viewport does nothing for the editor window; kept as a name
/// symmetric with `draw_main_viewport` for clarity.
fn draw_editor_viewport(_app: &mut App, _ctx: &egui::Context) {
    // Should never be called: the editor viewport is handled by the
    // closure in `spawn_editor_viewport`. This exists only so the
    // update dispatch has a matching arm.
}