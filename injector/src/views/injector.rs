use crate::core::scanning::{AppGroup, LaunchableApp};
use crate::shell::sidebar::draw_log_panel;
use crate::state::{App, ScanStatus, TargetMode};
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let total_height = ui.available_height();
    let log_height = (total_height * 0.40).clamp(160.0, 320.0);
    let top_height = (total_height - log_height - 20.0).max(200.0);

    egui::ScrollArea::vertical()
        .id_source("injector_top_scroll")
        .auto_shrink([false, false])
        .max_height(top_height)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Inject a hook DLL into a running process.")
                    .size(theme::sz(12.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text("Pick an app, launch it, and let the injector load the DLL once the target connects.");

            ui.add_space(16.0);

            draw_launch_card(app, ui);
            ui.add_space(14.0);
            draw_manual_target_card(app, ui);
        });

    ui.add_space(14.0);

    draw_log_panel(app, ui, log_height);
}

fn draw_launch_card(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("LAUNCH TARGET")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Installed apps discovered from the Windows registry.");

                ui.add_space(6.0);

                let (status_text, status_color, status_tip) = match &app.scan_status {
                    ScanStatus::Idle => (
                        format!("{} groups", app.installed_apps.len()),
                        theme::text_faint(),
                        "The app list has not been refreshed yet this session.",
                    ),
                    ScanStatus::Scanning => (
                        "scanning...".to_string(),
                        theme::warn(),
                        "Reading the Windows registry and building the app list.",
                    ),
                    ScanStatus::Ready { count } => (
                        format!("{} groups", count),
                        theme::ok(),
                        "Scan complete. The list below is up to date.",
                    ),
                    ScanStatus::Failed(msg) => {
                        let short: String = msg.chars().take(40).collect();
                        (
                            format!("scan failed: {}", short),
                            theme::error(),
                            "The scan hit an error. Check the log panel below.",
                        )
                    }
                };

                egui::Frame::none()
                    .fill(theme::bg())
                    .rounding(egui::Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(status_text)
                                .size(theme::sz(10.0))
                                .color(status_color),
                        )
                            .on_hover_text(status_tip);
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_rescan = app.scan_status != ScanStatus::Scanning;
                    let resp = ui.add_enabled(
                        can_rescan,
                        egui::Button::new(
                            egui::RichText::new("rescan")
                                .size(theme::sz(11.0))
                                .color(if can_rescan {
                                    theme::text_dim()
                                } else {
                                    theme::text_faint()
                                }),
                        )
                            .fill(egui::Color32::TRANSPARENT)
                            .rounding(egui::Rounding::same(6.0)),
                    );
                    resp.clone().on_hover_text(
                        "Re-read the registry and rebuild the list. Cache is discarded.",
                    );
                    if resp.clicked() {
                        app.start_rescan();
                    }
                });
            });

            ui.add_space(8.0);

            let search_resp = ui.add(
                egui::TextEdit::singleline(&mut app.app_search)
                    .hint_text("search apps...")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(10.0, 8.0)),
            );
            search_resp
                .clone()
                .on_hover_text("Filter the list by substring in the app name or path.");
            if search_resp.changed() {
                ui.ctx().request_repaint();
            }

            ui.add_space(8.0);

            let raw_search = app.app_search.trim().to_lowercase();
            let groups: Vec<AppGroup> = if raw_search.is_empty() {
                app.installed_apps.clone()
            } else {
                app.installed_apps
                    .iter()
                    .filter(|g| {
                        g.name.to_lowercase().contains(&raw_search)
                            || g.entries.iter().any(|e| {
                            e.name.to_lowercase().contains(&raw_search)
                                || e.path.to_lowercase().contains(&raw_search)
                        })
                    })
                    .cloned()
                    .collect()
            };

            let match_color = if groups.is_empty() {
                theme::text_faint()
            } else {
                theme::text_dim()
            };
            let match_text = if raw_search.is_empty() {
                format!("{} app groups", groups.len())
            } else {
                format!(
                    "{} match{} for \"{}\"",
                    groups.len(),
                    if groups.len() == 1 { "" } else { "es" },
                    app.app_search.trim()
                )
            };

            ui.label(
                egui::RichText::new(match_text)
                    .size(theme::sz(10.0))
                    .color(match_color),
            );

            ui.add_space(6.0);

            #[derive(Clone)]
            enum Row {
                Group {
                    group_idx: usize,
                },
                Entry {
                    group_idx: usize,
                    entry_idx: usize,
                },
            }

            let mut rows: Vec<Row> = Vec::new();
            for (gi, g) in groups.iter().enumerate() {
                rows.push(Row::Group { group_idx: gi });
                let key = g.name.to_lowercase();
                if app.expanded.contains(&key) && g.entries.len() > 1 {
                    for ei in 0..g.entries.len() {
                        rows.push(Row::Entry {
                            group_idx: gi,
                            entry_idx: ei,
                        });
                    }
                }
            }

            let mut clicked_path: Option<String> = None;
            let mut toggled_group: Option<String> = None;

            egui::Frame::none()
                .fill(theme::bg())
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::same(4.0))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_source("app_tree_scroll")
                        .auto_shrink([false, false])
                        .max_height(280.0)
                        .show_rows(ui, 26.0, rows.len(), |ui, row_range| {
                            for row_i in row_range {
                                match &rows[row_i] {
                                    Row::Group { group_idx } => {
                                        let g: &AppGroup = &groups[*group_idx];
                                        let key = g.name.to_lowercase();
                                        let is_open = app.expanded.contains(&key);
                                        let any_selected = g
                                            .entries
                                            .iter()
                                            .any(|e| app.launch_path == e.path);

                                        let fill = if any_selected {
                                            theme::accent()
                                        } else {
                                            theme::panel()
                                        };
                                        let name_color = if any_selected {
                                            egui::Color32::WHITE
                                        } else {
                                            theme::text()
                                        };
                                        let source_color = if any_selected {
                                            egui::Color32::from_rgb(220, 210, 255)
                                        } else {
                                            theme::text_faint()
                                        };

                                        let chevron = if g.entries.len() > 1 {
                                            if is_open {
                                                "▾"
                                            } else {
                                                "▸"
                                            }
                                        } else {
                                            " "
                                        };

                                        let frame_resp = egui::Frame::none()
                                            .fill(fill)
                                            .rounding(egui::Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(chevron)
                                                            .size(theme::sz(12.0))
                                                            .color(name_color),
                                                    );
                                                    ui.add_space(4.0);
                                                    ui.label(
                                                        egui::RichText::new(&g.name)
                                                            .size(theme::sz(12.0))
                                                            .color(name_color),
                                                    );
                                                    if g.entries.len() > 1 {
                                                        ui.add_space(6.0);
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "({})",
                                                                g.entries.len()
                                                            ))
                                                                .size(theme::sz(10.0))
                                                                .color(source_color),
                                                        );
                                                    }
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            ui.label(
                                                                egui::RichText::new(&g.source)
                                                                    .size(theme::sz(10.0))
                                                                    .color(source_color),
                                                            )
                                                                .on_hover_text(
                                                                    "Where this entry came from: registry, per-user install, or a shortcut.",
                                                                );
                                                        },
                                                    );
                                                });
                                            })
                                            .response;

                                        let row_resp = ui
                                            .interact(
                                                frame_resp.rect,
                                                ui.id().with(("group_row", row_i)),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_text(
                                                "Click to expand and select the primary executable.",
                                            );

                                        if row_resp.clicked() {
                                            if g.entries.len() > 1 {
                                                toggled_group = Some(key);
                                            }
                                            if let Some(first) = g.entries.first() {
                                                clicked_path = Some(first.path.clone());
                                            }
                                        }
                                    }
                                    Row::Entry {
                                        group_idx,
                                        entry_idx,
                                    } => {
                                        let g: &AppGroup = &groups[*group_idx];
                                        let e: &LaunchableApp = &g.entries[*entry_idx];
                                        let selected = app.launch_path == e.path;
                                        let name_color = if selected {
                                            theme::accent()
                                        } else {
                                            theme::text_dim()
                                        };

                                        let frame_resp = egui::Frame::none()
                                            .fill(theme::bg())
                                            .inner_margin(egui::Margin {
                                                left: 34.0,
                                                right: 10.0,
                                                top: 4.0,
                                                bottom: 4.0,
                                            })
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new("•")
                                                            .size(theme::sz(12.0))
                                                            .color(name_color),
                                                    );
                                                    ui.add_space(4.0);
                                                    ui.label(
                                                        egui::RichText::new(&e.name)
                                                            .size(theme::sz(11.0))
                                                            .color(name_color),
                                                    );
                                                    if selected {
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                ui.label(
                                                                    egui::RichText::new(
                                                                        "selected",
                                                                    )
                                                                        .size(theme::sz(10.0))
                                                                        .color(theme::accent()),
                                                                );
                                                            },
                                                        );
                                                    }
                                                });
                                            })
                                            .response;

                                        let row_resp = ui
                                            .interact(
                                                frame_resp.rect,
                                                ui.id().with(("entry_row", row_i)),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_text(format!(
                                                "Full path: {}",
                                                e.path
                                            ));

                                        if row_resp.clicked() {
                                            clicked_path = Some(e.path.clone());
                                        }
                                    }
                                }
                            }
                        });
                });

            if let Some(key) = toggled_group {
                if app.expanded.contains(&key) {
                    app.expanded.remove(&key);
                } else {
                    app.expanded.insert(key);
                }
            }

            if let Some(p) = clicked_path {
                app.launch_path = p;
            }

            if app.installed_apps.is_empty() {
                ui.add_space(6.0);
                let msg = match &app.scan_status {
                    ScanStatus::Scanning => {
                        "scanning your start menu, this takes a moment..."
                    }
                    ScanStatus::Failed(_) => "scan failed — check the LOG panel below",
                    _ => "no apps found",
                };
                ui.label(
                    egui::RichText::new(msg)
                        .size(theme::sz(10.0))
                        .italics()
                        .color(theme::text_faint()),
                );
            }

            if !app.launch_path.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("selected")
                            .size(theme::sz(10.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("The executable that will be launched.");
                    ui.add_space(8.0);
                    let path_text = app.launch_path.clone();
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("copy")
                                    .size(theme::sz(10.0))
                                    .color(theme::text_dim()),
                            )
                                .fill(egui::Color32::TRANSPARENT)
                                .rounding(egui::Rounding::same(4.0)),
                        )
                        .on_hover_text("Copy the full path to the clipboard.")
                        .clicked()
                    {
                        ui.output_mut(|o| o.copied_text = path_text.clone());
                    }
                });
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&app.launch_path)
                            .size(theme::sz(11.0))
                            .monospace()
                            .color(theme::text()),
                    )
                        .sense(egui::Sense::click()),
                )
                    .on_hover_text(&app.launch_path);
            }

            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut app.auto_inject_after_launch,
                    "auto-inject after launch",
                )
                    .on_hover_text(
                        "Wait for the launched app to open the watch IP, then inject automatically.",
                    );

                ui.add_space(12.0);

                ui.label(
                    egui::RichText::new("watch IP")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Remote IP the injector waits for before injecting.");

                ui.add(
                    egui::TextEdit::singleline(&mut app.remote_ip)
                        .desired_width(140.0)
                        .margin(egui::Margin::symmetric(6.0, 4.0)),
                )
                    .on_hover_text("Only the target process holding a connection to this IP is injected.");
            });

            ui.add_space(12.0);

            let launch_btn = egui::Button::new(
                egui::RichText::new("Launch & Inject")
                    .size(theme::sz(13.0))
                    .strong()
                    .color(egui::Color32::WHITE),
            )
                .fill(theme::accent())
                .rounding(egui::Rounding::same(10.0))
                .min_size(egui::vec2(ui.available_width(), 38.0));

            if ui
                .add(launch_btn)
                .on_hover_text("Start the selected app and inject the DLL once the network connection appears.")
                .clicked()
            {
                app.do_launch_and_inject();
            }
        });
}

fn draw_manual_target_card(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("MANUAL TARGET")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            )
                .on_hover_text("Inject into a process by PID or name without launching anything.");

            ui.add_space(8.0);

            egui::ComboBox::from_id_source("target_mode")
                .selected_text(app.mode.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.mode, TargetMode::Pid, TargetMode::Pid.label());
                    ui.selectable_value(
                        &mut app.mode,
                        TargetMode::Name,
                        TargetMode::Name.label(),
                    );
                });

            ui.add_space(10.0);

            match app.mode {
                TargetMode::Pid => {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.target_pid)
                            .hint_text("target PID (e.g. 15616)")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                        .on_hover_text("The PID of the process you want to inject into.");
                }
                TargetMode::Name => {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.target_name)
                            .hint_text("target process name")
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                        .on_hover_text("Injects into every process with this name. Use 'Scan' to preview the matches first.");
                }
            }

            ui.add_space(10.0);

            ui.add(
                egui::TextEdit::singleline(&mut app.dll_path)
                    .hint_text("path to hook_dll.dll")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(10.0, 8.0)),
            )
                .on_hover_text("Full path to the DLL that will be loaded into the target process.");

            ui.add_space(12.0);

            ui.horizontal(|ui| {
                let scan_w = ui.available_width() * 0.32;
                let preflight_btn = egui::Button::new(
                    egui::RichText::new("Scan").size(theme::sz(13.0)).color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(10.0))
                    .min_size(egui::vec2(scan_w, 38.0));

                if ui
                    .add(preflight_btn)
                    .on_hover_text("List every process that matches the target without injecting.")
                    .clicked()
                {
                    app.run_preflight();
                }

                let inject_btn = egui::Button::new(
                    egui::RichText::new("Inject")
                        .size(theme::sz(13.0))
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                    .fill(theme::accent())
                    .rounding(egui::Rounding::same(10.0))
                    .min_size(egui::vec2(ui.available_width(), 38.0));

                if ui
                    .add(inject_btn)
                    .on_hover_text("Inject the DLL into the target process now.")
                    .clicked()
                {
                    app.do_inject();
                }
            });

            if !app.preflight_results.is_empty() {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                if let Some(msg) = &app.preflight_message {
                    ui.label(
                        egui::RichText::new(msg)
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    );
                    ui.add_space(4.0);
                }

                egui::Grid::new("preflight_grid")
                    .striped(true)
                    .num_columns(3)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("PID")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.label(
                            egui::RichText::new("NAME")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.label(
                            egui::RichText::new("STATUS")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        )
                            .on_hover_text(
                                "injectable = can be opened with full rights · limited = partial access · denied = sandboxed or protected",
                            );
                        ui.end_row();

                        for entry in &app.preflight_results {
                            ui.label(
                                egui::RichText::new(format!("{}", entry.pid))
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(theme::text()),
                            );
                            ui.label(
                                egui::RichText::new(&entry.name)
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(theme::text()),
                            );
                            ui.label(
                                egui::RichText::new(entry.status.label())
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(entry.status.color()),
                            )
                                .on_hover_text(match entry.status {
                                    crate::core::processes::AccessStatus::Injectable => {
                                        "Full access. Injection will likely succeed."
                                    }
                                    crate::core::processes::AccessStatus::LimitedAccess => {
                                        "Partial access. Injection may fail."
                                    }
                                    crate::core::processes::AccessStatus::Denied => {
                                        "Access denied. Likely sandboxed or protected."
                                    }
                                });
                            ui.end_row();
                        }
                    });
            } else if let Some(msg) = &app.preflight_message {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(msg)
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                );
            }
        });
}