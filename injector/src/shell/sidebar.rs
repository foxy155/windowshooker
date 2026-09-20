use crate::core::logging::LogEntry;
use crate::state::{App, LogLevelFilter, View};
use crate::theme;

const NAV: [(View, &str, &str); 8] = [
    (
        View::Dashboard,
        "Dashboard",
        "Overview of the injector state, recent activity, and quick actions.",
    ),
    (
        View::Injector,
        "Injector",
        "Find installed apps, launch them, and inject the hook DLL.",
    ),
    (
        View::Packets,
        "Packets",
        "Live capture of send/recv traffic from injected processes.",
    ),
    (
        View::Hooks,
        "Hooks",
        "Define and manage inline hooks. Planned.",
    ),
    (
        View::Memory,
        "Memory",
        "Browse and edit process memory. Planned.",
    ),
    (
        View::Analyzer,
        "Analyzer",
        "Disassemble functions and inspect IL2CPP/Mono. Planned.",
    ),
    (
        View::Sessions,
        "Sessions",
        "Save and reload working sessions. Planned.",
    ),
    (
        View::Settings,
        "Settings",
        "Theme, paths, and injector behavior.",
    ),
];

pub fn draw_sidebar(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::left("sidebar")
        .resizable(false)
        .exact_width(200.0)
        .frame(
            egui::Frame::none()
                .fill(theme::panel())
                .inner_margin(egui::Margin::same(14.0)),
        )
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Sigil")
                    .size(theme::sz(20.0))
                    .strong()
                    .color(theme::text()),
            );
            ui.label(
                egui::RichText::new("reverse engineering suite")
                    .size(theme::sz(10.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text("A tool for exploring, hooking, and analyzing Windows applications.");

            ui.add_space(24.0);

            for (view, label, tooltip) in NAV {
                let selected = app.view == view;
                if nav_button(ui, label, selected, tooltip).clicked() {
                    app.view = view;
                }
                ui.add_space(3.0);
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("v0.1.0")
                        .size(theme::sz(10.0))
                        .color(theme::text_faint()),
                )
                    .on_hover_text("Build version");
            });
        });
}

pub fn draw_log_panel(app: &mut App, ui: &mut egui::Ui, log_height: f32) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("LOG")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Messages from the injector and the injected DLL.");

                ui.add_space(10.0);

                egui::ComboBox::from_id_source("log_level")
                    .selected_text(app.log_level_filter.label())
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.log_level_filter,
                            LogLevelFilter::All,
                            "All",
                        )
                            .on_hover_text("Show every message, including debug and trace.");
                        ui.selectable_value(
                            &mut app.log_level_filter,
                            LogLevelFilter::Info,
                            "Info+",
                        )
                            .on_hover_text("Hide debug and trace messages.");
                        ui.selectable_value(
                            &mut app.log_level_filter,
                            LogLevelFilter::Warn,
                            "Warn+",
                        )
                            .on_hover_text("Show warnings and errors only.");
                        ui.selectable_value(
                            &mut app.log_level_filter,
                            LogLevelFilter::Error,
                            "Error",
                        )
                            .on_hover_text("Show errors only.");
                    });

                ui.add_space(8.0);

                let log_search_resp = ui.add(
                    egui::TextEdit::singleline(&mut app.log_search)
                        .hint_text("search")
                        .desired_width(160.0)
                        .margin(egui::Margin::symmetric(8.0, 4.0)),
                );
                log_search_resp
                    .clone()
                    .on_hover_text("Filter log lines by substring in the message or target tag.");
                if log_search_resp.changed() {
                    ui.ctx().request_repaint();
                }

                ui.add_space(8.0);

                ui.checkbox(&mut app.log_autoscroll, "autoscroll")
                    .on_hover_text("Scroll to the newest entry automatically.");

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("copy all")
                                        .size(theme::sz(11.0))
                                        .color(theme::text_dim()),
                                )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .rounding(egui::Rounding::same(8.0)),
                            )
                            .on_hover_text("Copy the full log to the clipboard.")
                            .clicked()
                        {
                            let joined = {
                                let guard = app.log_entries.lock().unwrap();
                                guard
                                    .iter()
                                    .map(|e| {
                                        format!(
                                            "{} {:5} [{}] {}",
                                            e.timestamp,
                                            e.level.as_str(),
                                            e.target,
                                            e.message
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            };
                            ui.output_mut(|o| o.copied_text = joined);
                        }

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("clear")
                                        .size(theme::sz(11.0))
                                        .color(theme::text_dim()),
                                )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .rounding(egui::Rounding::same(8.0)),
                            )
                            .on_hover_text("Clear every log entry.")
                            .clicked()
                        {
                            if let Ok(mut guard) = app.log_entries.lock() {
                                guard.clear();
                            }
                        }
                    },
                );
            });

            ui.add_space(8.0);

            let entries: Vec<LogEntry> = match app.log_entries.lock() {
                Ok(guard) => guard
                    .iter()
                    .filter(|e| app.log_level_filter.accepts(e.level))
                    .filter(|e| {
                        if app.log_search.is_empty() {
                            return true;
                        }
                        let needle = app.log_search.to_lowercase();
                        e.message.to_lowercase().contains(&needle)
                            || e.target.to_lowercase().contains(&needle)
                    })
                    .cloned()
                    .collect(),
                Err(_) => Vec::new(),
            };

            let total = app.log_entries.lock().map(|g| g.len()).unwrap_or(0);

            let mut scroll = egui::ScrollArea::vertical()
                .id_source("log_scroll")
                .auto_shrink([false, false])
                .max_height(log_height);

            if app.log_autoscroll {
                scroll = scroll.stick_to_bottom(true);
            }

            scroll.show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(
                        egui::RichText::new("no log entries match the current filter")
                            .size(theme::sz(12.0))
                            .italics()
                            .color(theme::text_faint()),
                    );
                } else {
                    for entry in &entries {
                        let line = format!(
                            "{} {:5} [{}] {}",
                            entry.timestamp,
                            entry.level.as_str(),
                            entry.target,
                            entry.message
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(line)
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(entry.color()),
                            )
                                .sense(egui::Sense::click())
                                .wrap(false),
                        );
                    }
                }
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("{} of {} entries", entries.len(), total))
                    .size(theme::sz(10.0))
                    .color(theme::text_faint()),
            )
                .on_hover_text("Number of entries shown after the current filter, out of the total kept in memory.");
        });
}

fn nav_button(ui: &mut egui::Ui, label: &str, selected: bool, tooltip: &str) -> egui::Response {
    let fill = if selected {
        theme::accent()
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if selected {
        egui::Color32::WHITE
    } else {
        theme::text()
    };
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::sz(13.0))
                .color(text_color),
        )
            .fill(fill)
            .rounding(egui::Rounding::same(8.0))
            .min_size(egui::vec2(ui.available_width(), 32.0)),
    )
        .on_hover_text(tooltip)
}