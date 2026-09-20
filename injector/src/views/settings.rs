use crate::state::App;
use crate::theme::{self, Accent, Density, ThemeMode};

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_source("settings_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Settings")
                    .size(theme::sz(20.0))
                    .strong()
                    .color(theme::text()),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Preferences are saved to your user profile.")
                    .size(theme::sz(12.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text(
                    "Stored at %APPDATA%\\Sigil\\settings.txt. Edit that file directly or use the controls below.",
                );
            ui.add_space(20.0);

            appearance_card(app, ui);
            ui.add_space(14.0);
            paths_card(app, ui);
            ui.add_space(14.0);
            behavior_card(app, ui);
            ui.add_space(14.0);
            advanced_card(app, ui);
            ui.add_space(20.0);

            action_row(app, ui);
            ui.add_space(20.0);
        });
}

fn card<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            );
            ui.add_space(10.0);
            add_contents(ui)
        })
        .inner
}

fn label_dim(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::sz(11.0))
        .color(theme::text_dim())
}

fn apply(app: &App) {
    app.apply_settings_to_theme();
}

fn appearance_card(app: &mut App, ui: &mut egui::Ui) {
    card(ui, "APPEARANCE", |ui| {
        egui::Grid::new("appearance_grid")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .min_col_width(160.0)
            .show(ui, |ui| {
                // ---------- Theme row ----------
                ui.label(label_dim("Theme")).on_hover_text(
                    "Base color palette. All shades are dark variants tuned for long sessions.",
                );

                let theme_combo = egui::ComboBox::from_id_source("theme_mode")
                    .selected_text(app.settings.theme_mode.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for mode in ThemeMode::all() {
                            let resp = ui.selectable_value(
                                &mut app.settings.theme_mode,
                                mode,
                                mode.label(),
                            );
                            resp.on_hover_text(match mode {
                                ThemeMode::Dark => "Default neutral dark gray.",
                                ThemeMode::Darker => "Deep charcoal, minimal contrast.",
                                ThemeMode::Black => "Pure black, for OLED panels.",
                                ThemeMode::Midnight => "Blue-tinted near-black.",
                                ThemeMode::Slate => "Medium blue-gray, softer than pure dark.",
                                ThemeMode::Ash => "Light gray. Still dark-ish but airy.",
                                ThemeMode::Warm => "Brown-tinted dark. Easier on the eyes at night.",
                            });
                        }
                    });

                theme_combo
                    .response
                    .on_hover_text("Change the base color palette. Preview applies immediately.");

                if theme_combo.inner.is_some() {
                    apply(app);
                }
                ui.end_row();

                // ---------- Accent row ----------
                ui.label(label_dim("Accent")).on_hover_text(
                    "Highlight color used for selected items, links, and primary buttons.",
                );
                ui.horizontal(|ui| {
                    for accent in Accent::all() {
                        let (r, g, b) = accent.rgb();
                        let color = egui::Color32::from_rgb(r, g, b);
                        let selected = app.settings.accent == accent;

                        let button_size = egui::vec2(22.0, 22.0);
                        let (rect, response) =
                            ui.allocate_exact_size(button_size, egui::Sense::click());

                        let painter = ui.painter();
                        painter.circle_filled(rect.center(), 9.0, color);

                        if selected {
                            painter.circle_stroke(
                                rect.center(),
                                11.0,
                                egui::Stroke::new(2.0, theme::text()),
                            );
                        }

                        if response.clicked() {
                            app.settings.accent = accent;
                            apply(app);
                        }

                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }

                        response.on_hover_text(format!(
                            "Use {} as the accent color.",
                            accent.label()
                        ));
                    }
                });
                ui.end_row();

                // ---------- Density row ----------
                ui.label(label_dim("Density")).on_hover_text(
                    "Controls spacing between widgets, button padding, and panel margins.",
                );

                let density_combo = egui::ComboBox::from_id_source("density")
                    .selected_text(app.settings.density.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for d in Density::all() {
                            let resp =
                                ui.selectable_value(&mut app.settings.density, d, d.label());
                            resp.on_hover_text(match d {
                                Density::Comfortable => {
                                    "Default. Balanced padding for typical displays."
                                }
                                Density::Compact => {
                                    "Tighter padding. Fits more rows on screen."
                                }
                                Density::Spacious => {
                                    "Extra padding. Easier to hit targets on touch or HiDPI."
                                }
                            });
                        }
                    });

                density_combo
                    .response
                    .on_hover_text("Change widget spacing and padding. Preview applies immediately.");

                if density_combo.inner.is_some() {
                    apply(app);
                }
                ui.end_row();

                // ---------- Corner radius row ----------
                ui.label(label_dim("Corner radius"))
                    .on_hover_text("Rounding applied to cards, panels, and windows.");

                let corner_resp = ui.add(
                    egui::Slider::new(&mut app.settings.corner_radius, 0.0..=24.0)
                        .suffix(" px")
                        .fixed_decimals(0),
                );
                if corner_resp.changed() {
                    apply(app);
                }
                corner_resp.on_hover_text("0 = sharp corners · 24 = pill-shaped panels.");
                ui.end_row();

                // ---------- Font scale row ----------
                ui.label(label_dim("Font scale"))
                    .on_hover_text("Global multiplier applied to every text size in the app.");

                let mut pct = app.settings.font_scale * 100.0;
                let font_resp = ui.add(
                    egui::Slider::new(&mut pct, 85.0..=130.0)
                        .suffix(" %")
                        .fixed_decimals(0),
                );
                let font_changed = font_resp.changed();
                font_resp.on_hover_text(
                    "85% shrinks text · 130% enlarges it. Useful on HiDPI displays.",
                );
                if font_changed {
                    app.settings.font_scale = pct / 100.0;
                    apply(app);
                }
                ui.end_row();
            });
    });
}

fn paths_card(app: &mut App, ui: &mut egui::Ui) {
    card(ui, "PATHS", |ui| {
        egui::Grid::new("paths_grid")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .min_col_width(160.0)
            .show(ui, |ui| {
                path_row(
                    ui,
                    "Projects",
                    &mut app.settings.projects_dir,
                    "Where saved sessions and project files live.",
                );
                path_row(
                    ui,
                    "Dumps",
                    &mut app.settings.dumps_dir,
                    "Where memory dumps and packet captures are written.",
                );
                path_row(
                    ui,
                    "Logs",
                    &mut app.settings.logs_dir,
                    "Where log files are written when you export them.",
                );
                path_row(
                    ui,
                    "DLL search",
                    &mut app.settings.dll_search_dir,
                    "Extra folder searched for hook DLLs when you type a bare filename.",
                );
                path_row(
                    ui,
                    "Temp",
                    &mut app.settings.temp_dir,
                    "Scratch directory for extracted resources and caches.",
                );
            });
    });
}

fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, tooltip: &str) {
    ui.label(label_dim(label)).on_hover_text(tooltip);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(360.0)
            .margin(egui::Margin::symmetric(8.0, 6.0)),
    )
        .on_hover_text(tooltip);
    ui.end_row();
}

fn behavior_card(app: &mut App, ui: &mut egui::Ui) {
    card(ui, "BEHAVIOR", |ui| {
        egui::Grid::new("behavior_grid")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .min_col_width(160.0)
            .show(ui, |ui| {
                ui.label(label_dim("Confirmation"))
                    .on_hover_text("Ask before performing destructive or hard-to-reverse actions.");
                ui.vertical(|ui| {
                    let r1 = ui.checkbox(
                        &mut app.settings.confirm_inject,
                        "Confirm before injecting",
                    );
                    let r1_changed = r1.changed();
                    r1.on_hover_text(
                        "Show a modal dialog before loading the DLL into a target process.",
                    );
                    if r1_changed {
                        apply(app);
                    }

                    let r2 = ui.checkbox(
                        &mut app.settings.confirm_kill,
                        "Confirm before killing processes on exit",
                    );
                    let r2_changed = r2.changed();
                    r2.on_hover_text(
                        "Ask before taskkill-ing target processes when Sigil shuts down.",
                    );
                    if r2_changed {
                        apply(app);
                    }

                    let r3 = ui.checkbox(
                        &mut app.settings.restore_last_view,
                        "Restore last view on startup",
                    );
                    let r3_changed = r3.changed();
                    r3.on_hover_text("Reopen the view you were on when Sigil last closed.");
                    if r3_changed {
                        apply(app);
                    }
                });
                ui.end_row();

                ui.label(label_dim("Autosave"))
                    .on_hover_text("How often to write settings back to disk automatically.");
                ui.horizontal(|ui| {
                    let dv = ui.add(
                        egui::DragValue::new(&mut app.settings.autosave_minutes)
                            .speed(1.0)
                            .clamp_range(0..=120u32)
                            .suffix(" min"),
                    );
                    dv.on_hover_text("Minutes between automatic saves. 0 disables autosave.");
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("(0 = off)")
                            .size(theme::sz(10.0))
                            .color(theme::text_faint()),
                    )
                        .on_hover_text("Set to 0 to disable automatic saving entirely.");
                });
                ui.end_row();

                ui.label(label_dim("Startup log level"))
                    .on_hover_text("Minimum severity kept in the in-memory log when Sigil starts.");

                let log_combo = egui::ComboBox::from_id_source("startup_log_level")
                    .selected_text(app.settings.startup_log_level.clone())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for level in ["all", "info", "warn", "error"] {
                            let resp = ui.selectable_value(
                                &mut app.settings.startup_log_level,
                                level.to_string(),
                                level,
                            );
                            resp.on_hover_text(match level {
                                "all" => "Keep every message, including debug and trace.",
                                "info" => "Drop debug and trace. Keep info and above.",
                                "warn" => "Keep warnings and errors only.",
                                "error" => "Keep errors only.",
                                _ => "",
                            });
                        }
                    });

                log_combo
                    .response
                    .on_hover_text("Change the minimum log level kept at startup.");
                ui.end_row();
            });
    });
}

fn advanced_card(app: &mut App, ui: &mut egui::Ui) {
    card(ui, "ADVANCED", |ui| {
        egui::Grid::new("advanced_grid")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .min_col_width(160.0)
            .show(ui, |ui| {
                ui.label(label_dim("Process scan"))
                    .on_hover_text("How thoroughly Sigil searches for installed applications.");

                let scan_combo = egui::ComboBox::from_id_source("scan_depth")
                    .selected_text(app.settings.scan_depth.clone())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for opt in ["registry", "registry + filesystem"] {
                            let resp = ui.selectable_value(
                                &mut app.settings.scan_depth,
                                opt.to_string(),
                                opt,
                            );
                            resp.on_hover_text(match opt {
                                "registry" => "Only read the Windows registry. Fast.",
                                "registry + filesystem" => {
                                    "Also walk common install folders. Slower but finds portable apps."
                                }
                                _ => "",
                            });
                        }
                    });

                scan_combo
                    .response
                    .on_hover_text("Choose how many sources to consult during the app scan.");
                ui.end_row();

                ui.label(label_dim("Pipe buffer")).on_hover_text(
                    "Size of the read buffer used when receiving frames from the DLL.",
                );
                let pv = ui.add(
                    egui::DragValue::new(&mut app.settings.pipe_buffer_kb)
                        .speed(1.0)
                        .clamp_range(16..=4096u32)
                        .suffix(" kb"),
                );
                pv.on_hover_text(
                    "Larger buffers handle bursty traffic better but use more memory. 16-4096 KB.",
                );
                ui.end_row();

                ui.label(label_dim("Inject timeout")).on_hover_text(
                    "How long to wait for the target to be ready before giving up on injection.",
                );
                let tv = ui.add(
                    egui::DragValue::new(&mut app.settings.inject_timeout_sec)
                        .speed(1.0)
                        .clamp_range(5..=600u32)
                        .suffix(" sec"),
                );
                tv.on_hover_text(
                    "After launching an app, Sigil waits this long for it to connect to the watch IP.",
                );
                ui.end_row();

                ui.label(label_dim("hooks.json"))
                    .on_hover_text("Path to the JSON file describing inline hook definitions.");
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings.hooks_json_path)
                        .hint_text("(default)")
                        .desired_width(360.0)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                )
                    .on_hover_text(
                        "Leave blank to use the default location. Ignored until the Hooks view is implemented.",
                    );
                ui.end_row();
            });
    });
}

fn action_row(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let save_btn = egui::Button::new(
            egui::RichText::new("Save")
                .size(theme::sz(13.0))
                .strong()
                .color(egui::Color32::WHITE),
        )
            .fill(theme::accent())
            .rounding(egui::Rounding::same(10.0))
            .min_size(egui::vec2(120.0, 36.0));

        let save_resp = ui.add(save_btn);
        let save_clicked = save_resp.clicked();
        save_resp.on_hover_text("Write the current settings to disk and re-snapshot them.");
        if save_clicked {
            app.save_settings();
        }

        ui.add_space(8.0);

        let revert_btn = egui::Button::new(
            egui::RichText::new("Revert")
                .size(theme::sz(13.0))
                .color(theme::text()),
        )
            .fill(theme::panel_hover())
            .rounding(egui::Rounding::same(10.0))
            .min_size(egui::vec2(100.0, 36.0));

        let revert_resp = ui.add(revert_btn);
        let revert_clicked = revert_resp.clicked();
        revert_resp.on_hover_text("Discard unsaved changes and restore the last saved settings.");
        if revert_clicked {
            app.revert_settings();
        }

        ui.add_space(8.0);

        let reset_btn = egui::Button::new(
            egui::RichText::new("Reset to defaults")
                .size(theme::sz(13.0))
                .color(theme::text()),
        )
            .fill(theme::panel_hover())
            .rounding(egui::Rounding::same(10.0))
            .min_size(egui::vec2(160.0, 36.0));

        let reset_resp = ui.add(reset_btn);
        let reset_clicked = reset_resp.clicked();
        reset_resp.on_hover_text(
            "Replace every setting with the built-in defaults. Does not save until you press Save.",
        );
        if reset_clicked {
            app.settings = crate::core::settings::Settings::default();
            app.apply_settings_to_theme();
        }
    });
}