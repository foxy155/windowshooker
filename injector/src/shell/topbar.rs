use crate::state::App;
use crate::theme;
use std::sync::atomic::Ordering;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .inner_margin(egui::Margin::symmetric(16.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(app.view.title())
                        .size(theme::sz(15.0))
                        .strong()
                        .color(theme::text()),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Target PID pill
                    let target_text = if app.target_pid.trim().is_empty() {
                        "no target".to_string()
                    } else {
                        format!("pid {}", app.target_pid.trim())
                    };
                    let target_color = if app.target_pid.trim().is_empty() {
                        theme::text_faint()
                    } else {
                        theme::accent()
                    };
                    pill(
                        ui,
                        &target_text,
                        target_color,
                        "The last PID the injector identified as holding the watch IP connection.",
                    );

                    ui.add_space(6.0);

                    // Pipe pill
                    let pipe_on = app.pipe_connected.load(Ordering::Relaxed);
                    let (pipe_text, pipe_color, pipe_tip) = if pipe_on {
                        (
                            "pipe live",
                            theme::ok(),
                            "The injected DLL is connected and streaming captured data.",
                        )
                    } else {
                        (
                            "pipe idle",
                            theme::text_faint(),
                            "No DLL is connected to the packet pipe right now.",
                        )
                    };
                    pill(ui, pipe_text, pipe_color, pipe_tip);

                    ui.add_space(6.0);

                    // Elevation pill
                    let elevated = app.is_elevated.load(Ordering::Relaxed);
                    let (elev_text, elev_color, elev_tip) = if elevated {
                        (
                            "elevated",
                            theme::ok(),
                            "SeDebugPrivilege is enabled. The injector can open protected processes.",
                        )
                    } else {
                        (
                            "standard user",
                            theme::warn(),
                            "Running without SeDebugPrivilege. Some processes will reject injection.",
                        )
                    };
                    pill(ui, elev_text, elev_color, elev_tip);

                    ui.add_space(6.0);

                    // Project pill
                    let (proj_text, proj_color, proj_tip) = match &app.current_project {
                        None => (
                            "no project".to_string(),
                            theme::text_faint(),
                            "No project is open. Create one in the Sessions view.",
                        ),
                        Some(p) => {
                            if app.project_dirty {
                                if app.settings.project_autosave {
                                    (
                                        format!("project: {} · saving…", p.name),
                                        theme::warn(),
                                        "Changes pending. Autosave will write them this frame.",
                                    )
                                } else {
                                    (
                                        format!("project: {} · unsaved", p.name),
                                        theme::warn(),
                                        "Changes pending. Autosave is off — click Save in Sessions.",
                                    )
                                }
                            } else {
                                (
                                    format!("project: {}", p.name),
                                    theme::accent(),
                                    "Current project is saved to disk.",
                                )
                            }
                        }
                    };
                    pill(ui, &proj_text, proj_color, proj_tip);
                });
            });
        });
}

fn pill(ui: &mut egui::Ui, text: &str, color: egui::Color32, tooltip: &str) {
    egui::Frame::none()
        .fill(theme::bg())
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(theme::sz(11.0))
                    .color(color),
            )
        })
        .response
        .on_hover_text(tooltip);
}