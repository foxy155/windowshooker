use crate::core::logging::LogEntry;
use crate::state::{App, ScanStatus, View};
use crate::theme;
use std::sync::atomic::Ordering;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Welcome to Sigil")
            .size(theme::sz(20.0))
            .strong()
            .color(theme::text()),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("A reverse engineering suite for Windows targets.")
            .size(theme::sz(12.0))
            .color(theme::text_dim()),
    )
        .on_hover_text("Sigil bundles process discovery, DLL injection, packet capture, and hook management into one tool.");

    ui.add_space(20.0);

    let injector_status = injector_status(app);
    let packets_status = packets_status(app);

    ui.columns(2, |cols| {
        status_card(
            &mut cols[0],
            "Injector",
            injector_status,
            "Status of the app scanner and the most recent injection attempt.",
        );
        status_card(
            &mut cols[1],
            "Packets",
            packets_status,
            "Whether the injected DLL is streaming captured packets to the tool.",
        );
    });

    ui.add_space(16.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("QUICK ACTIONS")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            );
            ui.add_space(10.0);

            ui.horizontal_wrapped(|ui| {
                if quick_button(ui, "Open Injector", "Go to the injector view to select a target and inject the DLL.")
                    .clicked()
                {
                    app.view = View::Injector;
                }
                if quick_button(ui, "View Packets", "Open the live packet capture view.")
                    .clicked()
                {
                    app.view = View::Packets;
                }
                if quick_button(ui, "Rescan installed apps", "Re-read the Windows registry and rebuild the app list.")
                    .clicked()
                {
                    app.start_rescan();
                }
                if quick_button(ui, "Open Settings", "Change theme, paths, and injector behavior.")
                    .clicked()
                {
                    app.view = View::Settings;
                }
            });
        });

    ui.add_space(16.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("RECENT ACTIVITY")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            )
                .on_hover_text("The last log lines emitted by the injector or the injected DLL.");
            ui.add_space(10.0);

            let entries: Vec<LogEntry> = match app.log_entries.lock() {
                Ok(guard) => guard.iter().rev().take(8).cloned().collect(),
                Err(_) => Vec::new(),
            };

            if entries.is_empty() {
                ui.label(
                    egui::RichText::new("nothing yet")
                        .size(theme::sz(12.0))
                        .italics()
                        .color(theme::text_faint()),
                );
            } else {
                for entry in &entries {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&entry.timestamp)
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::text_faint()),
                        );
                        ui.label(
                            egui::RichText::new(format!("[{}]", entry.target))
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::accent()),
                        )
                            .on_hover_text(format!("Logged from the {} subsystem.", entry.target));
                        ui.label(
                            egui::RichText::new(&entry.message)
                                .size(theme::sz(11.0))
                                .color(entry.color()),
                        );
                    });
                }
            }
        });
}

fn injector_status(app: &App) -> (&'static str, egui::Color32) {
    match &app.scan_status {
        ScanStatus::Idle => ("idle", theme::text_faint()),
        ScanStatus::Scanning => ("scanning installed apps", theme::warn()),
        ScanStatus::Ready { .. } => ("ready", theme::ok()),
        ScanStatus::Failed(_) => ("scan failed", theme::error()),
    }
}

fn packets_status(app: &App) -> (&'static str, egui::Color32) {
    if app.pipe_connected.load(Ordering::Relaxed) {
        ("live", theme::ok())
    } else {
        ("idle", theme::text_faint())
    }
}

fn status_card(
    ui: &mut egui::Ui,
    title: &str,
    status: (&'static str, egui::Color32),
    tooltip: &str,
) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_height(70.0);
            ui.label(
                egui::RichText::new(title)
                    .size(theme::sz(11.0))
                    .strong()
                    .color(theme::text_dim()),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(status.0)
                    .size(theme::sz(16.0))
                    .strong()
                    .color(status.1),
            )
                .on_hover_text(tooltip);
        });
}

fn quick_button(ui: &mut egui::Ui, label: &str, tooltip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::sz(12.0))
                .color(theme::text()),
        )
            .fill(theme::panel_hover())
            .rounding(egui::Rounding::same(8.0))
            .min_size(egui::vec2(0.0, 34.0)),
    )
        .on_hover_text(tooltip)
}