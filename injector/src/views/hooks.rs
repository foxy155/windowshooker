//! Hooks view. Tab bar + Library / Edit Hook / New Hook.
//!
//! Editor panel is available both docked (inside the Hooks view) and
//! popped out as a real OS window via `app.rs`. Editor state is
//! shared via `Arc<Mutex<EditorState>>`.

use egui::Ui;
use crate::core::code_editor::CodeLanguage;
use crate::core::hooks::{
    self, Hook, HookParam, ParamKind, ParamValue,
};
use crate::state::{App, HooksTab};
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    if !app.hooks_loaded {
        app.refresh_hook_list();
    }

    let total_height = ui.available_height();
    let (floating, has_tabs) = {
        let e = app.editor();
        (e.floating, !e.tabs.is_empty())
    };
    let show_docked = has_tabs && !floating;
    let editor_height = if show_docked {
        (total_height * 0.45).clamp(220.0, 520.0)
    } else {
        0.0
    };
    let top_height = (total_height - editor_height - 20.0).max(200.0);

    egui::ScrollArea::vertical()
        .id_source("hooks_scroll")
        .auto_shrink([false, false])
        .max_height(top_height)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Hooks")
                    .size(theme::sz(20.0))
                    .strong()
                    .color(theme::text()),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "A library of DLL hooks. Import working ones, edit them, inject them.",
                )
                    .size(theme::sz(12.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text(
                    "Hooks live under %APPDATA%\\Sigil\\hooks. Each one is a folder with hook.json, an optional DLL, and optional source.",
                );

            ui.add_space(16.0);

            tab_bar(app, ui);

            ui.add_space(14.0);

            match app.hooks_tab {
                HooksTab::Library => library_tab(app, ui),
                HooksTab::EditHook => edit_hook_tab(app, ui),
                HooksTab::NewHook => new_hook_tab(app, ui),
            }
        });

    if show_docked {
        ui.add_space(10.0);
        editor_panel_docked(app, ui);
    }
}

// ============================================================
// EDITOR ENTRY POINTS
// ============================================================

pub fn draw_editor_only(app: &mut App, ui: &mut egui::Ui) {
    editor_tab_bar(app, ui);
    ui.add_space(6.0);
    editor_toolbar(app, ui);
    ui.add_space(6.0);
    editor_body(app, ui, 400.0);
    close_dirty_dialog(app, ui.ctx());
}

pub fn draw_editor_with_shared_state(
    ui: &mut egui::Ui,
    editor: std::sync::Arc<std::sync::Mutex<crate::core::code_editor::EditorState>>,
) {
    editor_tab_bar_shared(ui, &editor);
    ui.add_space(6.0);
    editor_toolbar_shared(ui, &editor);
    ui.add_space(6.0);
    editor_body_shared(ui, &editor, 400.0);
}

// ============================================================
// TAB BAR (main Hooks view)
// ============================================================

fn tab_bar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let tabs = [HooksTab::Library, HooksTab::EditHook, HooksTab::NewHook];

        for tab in tabs {
            let selected = app.hooks_tab == tab;
            let fill = if selected {
                theme::accent()
            } else {
                theme::panel()
            };
            let text_color = if selected {
                egui::Color32::WHITE
            } else {
                theme::text()
            };

            let btn = egui::Button::new(
                egui::RichText::new(tab.label())
                    .size(theme::sz(12.0))
                    .color(text_color),
            )
                .fill(fill)
                .rounding(egui::Rounding::same(8.0))
                .min_size(egui::vec2(140.0, 32.0));

            let resp = ui.add(btn);
            let clicked = resp.clicked();
            resp.on_hover_text(match tab {
                HooksTab::Library => {
                    "Browse and search the hook library. Enable, inject, delete."
                }
                HooksTab::EditHook => {
                    "Edit the selected hook. Name, params, target, DLL."
                }
                HooksTab::NewHook => {
                    "Create a new hook. Leaves any hook you were editing behind."
                }
            });

            if clicked {
                if matches!(tab, HooksTab::NewHook) {
                    app.deselect_hook_with_autosave();
                }
                app.hooks_tab = tab;
            }

            ui.add_space(6.0);
        }
    });
}

// ============================================================
// TAB: Library
// ============================================================

fn library_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("HOOK LIBRARY")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Every hook in %APPDATA%\\Sigil\\hooks.");

                ui.add_space(8.0);

                ui.label(
                    egui::RichText::new(format!("{} hooks", app.hooks.len()))
                        .size(theme::sz(10.0))
                        .color(theme::text_faint()),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let inject_all_btn = egui::Button::new(
                        egui::RichText::new("inject all enabled")
                            .size(theme::sz(11.0))
                            .color(theme::text()),
                    )
                        .fill(theme::panel_hover())
                        .rounding(egui::Rounding::same(6.0));

                    let inject_all_resp = ui.add(inject_all_btn);
                    let inject_all_clicked = inject_all_resp.clicked();
                    inject_all_resp.on_hover_text(
                        "Inject every hook whose enabled checkbox is on, into the current target PID.",
                    );
                    if inject_all_clicked {
                        app.inject_all_enabled_hooks();
                    }

                    ui.add_space(6.0);

                    let refresh_btn = egui::Button::new(
                        egui::RichText::new("refresh")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));

                    let refresh_resp = ui.add(refresh_btn);
                    let refresh_clicked = refresh_resp.clicked();
                    refresh_resp.on_hover_text("Re-read the hooks folder from disk.");
                    if refresh_clicked {
                        app.refresh_hook_list();
                    }
                });
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let search_resp = ui.add(
                    egui::TextEdit::singleline(&mut app.hooks_search)
                        .hint_text("search hooks...")
                        .desired_width(300.0)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                );
                search_resp.on_hover_text("Filter by hook name or author.");

                ui.add_space(8.0);

                let import_btn = egui::Button::new(
                    egui::RichText::new("import folder")
                        .size(theme::sz(11.0))
                        .color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(6.0));

                let import_resp = ui.add(import_btn);
                let import_clicked = import_resp.clicked();
                import_resp.on_hover_text(
                    "Copy an external hook folder into the library. Nothing at the source is touched.",
                );
                if import_clicked {
                    if let Some(folder) = rfd_pick_folder() {
                        app.import_hook(&folder);
                    } else {
                        log::info!(target: "hooks", "import cancelled or no folder picker");
                    }
                }
            });

            ui.add_space(12.0);

            let needle = app.hooks_search.trim().to_lowercase();
            let filtered: Vec<_> = app
                .hooks
                .iter()
                .filter(|h| {
                    if needle.is_empty() {
                        return true;
                    }
                    h.name.to_lowercase().contains(&needle)
                        || h.author.to_lowercase().contains(&needle)
                })
                .cloned()
                .collect();

            if filtered.is_empty() {
                ui.label(
                    egui::RichText::new(if needle.is_empty() {
                        "no hooks in the library — create one from the New Hook tab"
                    } else {
                        "no hooks match the search"
                    })
                        .size(theme::sz(12.0))
                        .italics()
                        .color(theme::text_faint()),
                )
                    .on_hover_text("The hooks folder is empty or nothing matches.");
                return;
            }

            let mut open_folder: Option<std::path::PathBuf> = None;
            let mut delete_name: Option<String> = None;
            let mut toggle_enable: Option<(String, bool)> = None;
            let mut inject_name: Option<String> = None;
            let mut open_code_name: Option<String> = None;

            egui::ScrollArea::vertical()
                .id_source("hook_list_scroll")
                .auto_shrink([false, false])
                .max_height(520.0)
                .show(ui, |ui| {
                    for summary in &filtered {
                        egui::Frame::none()
                            .fill(theme::bg())
                            .rounding(egui::Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let mut enabled = summary.enabled;
                                    let cb = ui.checkbox(&mut enabled, "");
                                    let cb_changed = cb.changed();
                                    cb.on_hover_text(
                                        "Enable this hook for 'inject all enabled'.",
                                    );
                                    if cb_changed {
                                        toggle_enable =
                                            Some((summary.name.clone(), enabled));
                                    }

                                    ui.add_space(4.0);

                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(&summary.name)
                                                    .size(theme::sz(13.0))
                                                    .strong()
                                                    .color(theme::text()),
                                            );

                                            ui.add_space(6.0);

                                            if summary.dll_present {
                                                ui.label(
                                                    egui::RichText::new("dll")
                                                        .size(theme::sz(9.0))
                                                        .color(theme::ok()),
                                                )
                                                    .on_hover_text(
                                                        "A compiled DLL is present in this hook folder.",
                                                    );
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("no dll")
                                                        .size(theme::sz(9.0))
                                                        .color(theme::blocked()),
                                                )
                                                    .on_hover_text(
                                                        "No DLL found. Compile one and place it in the hook folder before injecting.",
                                                    );
                                            }

                                            if summary.source_present {
                                                ui.label(
                                                    egui::RichText::new("source")
                                                        .size(theme::sz(9.0))
                                                        .color(theme::accent()),
                                                )
                                                    .on_hover_text(
                                                        "A source file is present and can be opened in the editor.",
                                                    );
                                            }
                                        });

                                        if !summary.author.is_empty() {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "by {} · v{}",
                                                    summary.author, summary.version
                                                ))
                                                    .size(theme::sz(10.0))
                                                    .color(theme::text_faint()),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "v{}",
                                                    summary.version
                                                ))
                                                    .size(theme::sz(10.0))
                                                    .color(theme::text_faint()),
                                            );
                                        }
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let del_resp = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("delete")
                                                        .size(theme::sz(11.0))
                                                        .color(theme::blocked()),
                                                )
                                                    .fill(egui::Color32::TRANSPARENT)
                                                    .rounding(egui::Rounding::same(6.0)),
                                            );
                                            let del_clicked = del_resp.clicked();
                                            del_resp.on_hover_text(
                                                "Delete this hook folder and everything inside it.",
                                            );
                                            if del_clicked {
                                                delete_name = Some(summary.name.clone());
                                            }

                                            ui.add_space(4.0);

                                            let inject_resp = ui.add_enabled(
                                                summary.dll_present,
                                                egui::Button::new(
                                                    egui::RichText::new("inject")
                                                        .size(theme::sz(11.0))
                                                        .color(if summary.dll_present {
                                                            theme::text()
                                                        } else {
                                                            theme::text_faint()
                                                        }),
                                                )
                                                    .fill(theme::panel_hover())
                                                    .rounding(egui::Rounding::same(6.0)),
                                            );
                                            let inject_clicked = inject_resp.clicked();
                                            inject_resp.on_hover_text(if summary.dll_present {
                                                "Inject this hook into the current target PID."
                                            } else {
                                                "Cannot inject: this hook has no compiled DLL yet."
                                            });
                                            if inject_clicked {
                                                inject_name = Some(summary.name.clone());
                                            }

                                            ui.add_space(4.0);

                                            let open_code_btn = egui::Button::new(
                                                egui::RichText::new("open code")
                                                    .size(theme::sz(11.0))
                                                    .color(theme::accent()),
                                            )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .rounding(egui::Rounding::same(6.0));

                                            let open_code_resp = ui.add(open_code_btn);
                                            let open_code_clicked = open_code_resp.clicked();
                                            open_code_resp.on_hover_text(
                                                "Open this hook's source in the editor below. Creates a template if none exists.",
                                            );
                                            if open_code_clicked {
                                                open_code_name =
                                                    Some(summary.name.clone());
                                            }

                                            ui.add_space(4.0);

                                            let edit_resp = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("edit")
                                                        .size(theme::sz(11.0))
                                                        .color(theme::text()),
                                                )
                                                    .fill(theme::panel_hover())
                                                    .rounding(egui::Rounding::same(6.0)),
                                            );
                                            let edit_clicked = edit_resp.clicked();
                                            edit_resp.on_hover_text(
                                                "Select this hook and open the Edit Hook tab.",
                                            );
                                            if edit_clicked {
                                                open_folder = Some(summary.folder.clone());
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(6.0);
                    }
                });

            if let Some((name, enabled)) = toggle_enable {
                toggle_hook_enabled(app, &name, enabled);
            }

            if let Some(folder) = open_folder {
                app.open_hook(&folder);
            }

            if let Some(name) = inject_name {
                app.inject_hook_by_name(&name);
            }

            if let Some(name) = delete_name {
                delete_hook_by_name(app, &name);
            }

            if let Some(name) = open_code_name {
                open_hook_source(app, &name);
            }
        });
}

fn open_hook_source(app: &mut App, name: &str) {
    let folder = app
        .hooks
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.folder.clone());
    let Some(folder) = folder else { return };

    match hooks::load_hook(&folder) {
        Ok(mut h) => {
            let language = h
                .language
                .as_deref()
                .map(CodeLanguage::from_key)
                .unwrap_or(app.new_hook_language);

            match hooks::ensure_source_file(&mut h, language.key()) {
                Ok((path, _content)) => {
                    if let Err(e) = hooks::save_hook(&h) {
                        log::error!(target: "hooks", "{}", e);
                    }
                    let open_result = {
                        let mut e = app.editor();
                        e.open_or_focus(h.name.clone(), path.clone(), language)
                    };
                    match open_result {
                        Ok(_) => {
                            log::info!(
                                target: "hooks",
                                "opened source for '{}' at {}",
                                h.name,
                                path.display()
                            );
                        }
                        Err(e) => log::error!(target: "hooks", "{}", e),
                    }
                    app.selected_hook = Some(h);
                }
                Err(e) => log::error!(target: "hooks", "{}", e),
            }
        }
        Err(e) => log::error!(target: "hooks", "{}", e),
    }
}

fn toggle_hook_enabled(app: &mut App, name: &str, enabled: bool) {
    let folder = app
        .hooks
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.folder.clone());
    let Some(folder) = folder else { return };
    match hooks::load_hook(&folder) {
        Ok(mut h) => {
            h.enabled = enabled;
            if let Err(e) = hooks::save_hook(&h) {
                log::error!(target: "hooks", "{}", e);
            }
            app.refresh_hook_list();
        }
        Err(e) => log::error!(target: "hooks", "{}", e),
    }
}

fn delete_hook_by_name(app: &mut App, name: &str) {
    let folder = app
        .hooks
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.folder.clone());
    let Some(folder) = folder else { return };

    {
        let mut e = app.editor();
        e.close_tabs_for_hook(name);
    }

    match hooks::delete_hook(&folder) {
        Ok(()) => {
            log::info!(target: "hooks", "deleted hook '{}'", name);
            if let Some(h) = &app.selected_hook {
                if h.name == name {
                    app.selected_hook = None;
                }
            }
            app.refresh_hook_list();
        }
        Err(e) => log::error!(target: "hooks", "{}", e),
    }
}

// ============================================================
// TAB: Edit Hook
// ============================================================

fn edit_hook_tab(app: &mut App, ui: &mut egui::Ui) {
    let Some(hook) = app.selected_hook.clone() else {
        egui::Frame::none()
            .fill(theme::panel())
            .rounding(egui::Rounding::same(theme::corner()))
            .inner_margin(egui::Margin::same(24.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No hook selected")
                            .size(theme::sz(16.0))
                            .strong()
                            .color(theme::text_dim()),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Pick one from the Library tab, or create one in New Hook.",
                        )
                            .size(theme::sz(12.0))
                            .color(theme::text_faint()),
                    );
                    ui.add_space(20.0);
                });
            });
        return;
    };

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("EDIT HOOK")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Edit the selected hook's metadata, params, and injection.");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let done_btn = egui::Button::new(
                        egui::RichText::new("done editing")
                            .size(theme::sz(11.0))
                            .color(theme::accent()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));

                    let done_resp = ui.add(done_btn);
                    let done_clicked = done_resp.clicked();
                    done_resp.on_hover_text(
                        "Save this hook and go back to the Library.",
                    );
                    if done_clicked {
                        app.deselect_hook_with_autosave();
                        app.hooks_tab = HooksTab::Library;
                    }

                    ui.add_space(6.0);

                    let reveal_btn = egui::Button::new(
                        egui::RichText::new("reveal folder")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));

                    let reveal_resp = ui.add(reveal_btn);
                    let reveal_clicked = reveal_resp.clicked();
                    reveal_resp.on_hover_text("Open the hook folder in File Explorer.");
                    if reveal_clicked {
                        open_folder_in_explorer(&hook.folder);
                    }
                });
            });

            ui.add_space(12.0);

            egui::Grid::new("edit_hook_grid")
                .num_columns(2)
                .spacing([18.0, 10.0])
                .min_col_width(140.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Name")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("Display name. Also the folder name.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        ui.add(
                            egui::TextEdit::singleline(&mut h.name)
                                .desired_width(360.0)
                                .margin(egui::Margin::symmetric(8.0, 6.0)),
                        );
                    }
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Author")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("Who wrote the hook. Free text.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        ui.add(
                            egui::TextEdit::singleline(&mut h.author)
                                .hint_text("(unknown)")
                                .desired_width(360.0)
                                .margin(egui::Margin::symmetric(8.0, 6.0)),
                        );
                    }
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Version")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("Free-form version string.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        ui.add(
                            egui::TextEdit::singleline(&mut h.version)
                                .desired_width(160.0)
                                .margin(egui::Margin::symmetric(8.0, 6.0)),
                        );
                    }
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Target")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("The exe the hook is meant to run inside, e.g. 'game.exe'.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        ui.add(
                            egui::TextEdit::singleline(&mut h.target_module)
                                .hint_text("game.exe")
                                .desired_width(360.0)
                                .margin(egui::Margin::symmetric(8.0, 6.0)),
                        );
                    }
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("Method")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("How the DLL is loaded into the target.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        egui::ComboBox::from_id_source("hook_injection_method")
                            .selected_text(h.injection_method.label())
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                use crate::core::hooks::SerializedInjectionMethod as M;
                                ui.selectable_value(
                                    &mut h.injection_method,
                                    M::CreateRemoteThread,
                                    "CreateRemoteThread",
                                )
                                    .on_hover_text(
                                        "Classic LoadLibrary injection. Requires an existing process.",
                                    );
                                ui.selectable_value(
                                    &mut h.injection_method,
                                    M::QueueUserApc,
                                    "QueueUserAPC",
                                )
                                    .on_hover_text(
                                        "Queues LoadLibrary via APC on each thread. Needs a thread that enters alertable state.",
                                    );
                            });
                    }
                    ui.end_row();

                    ui.label(
                        egui::RichText::new("DLL")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text("Filename of the DLL inside the hook folder.");
                    if let Some(h) = app.selected_hook.as_mut() {
                        ui.add(
                            egui::TextEdit::singleline(&mut h.dll)
                                .desired_width(360.0)
                                .margin(egui::Margin::symmetric(8.0, 6.0)),
                        );
                    }
                    ui.end_row();
                });

            ui.add_space(10.0);

            ui.label(
                egui::RichText::new("Description")
                    .size(theme::sz(11.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text("What the hook does. Shown nowhere yet, but useful to document.");
            if let Some(h) = app.selected_hook.as_mut() {
                ui.add(
                    egui::TextEdit::multiline(&mut h.description)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                );
            }

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            params_section(app, ui);

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            action_row(app, ui);
        });
}

fn params_section(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("PARAMETERS")
                .size(theme::sz(10.0))
                .strong()
                .color(theme::text_dim()),
        )
            .on_hover_text(
                "Values sent to the DLL over the control pipe as JSON when you inject.",
            );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add_btn = egui::Button::new(
                egui::RichText::new("+ add param")
                    .size(theme::sz(11.0))
                    .color(theme::text()),
            )
                .fill(theme::panel_hover())
                .rounding(egui::Rounding::same(6.0));

            let add_resp = ui.add(add_btn);
            let add_clicked = add_resp.clicked();
            add_resp.on_hover_text("Add a new parameter to this hook.");
            if add_clicked {
                if let Some(h) = app.selected_hook.as_mut() {
                    h.params.push(HookParam {
                        key: format!("param{}", h.params.len() + 1),
                        label: "New parameter".into(),
                        kind: ParamKind::Float,
                        default: ParamValue::Float(0.0),
                        value: ParamValue::Float(0.0),
                        min: None,
                        max: None,
                    });
                }
            }
        });
    });

    ui.add_space(8.0);

    let params_len = app
        .selected_hook
        .as_ref()
        .map(|h| h.params.len())
        .unwrap_or(0);

    if params_len == 0 {
        ui.label(
            egui::RichText::new("no parameters — the DLL will get an empty config")
                .size(theme::sz(11.0))
                .italics()
                .color(theme::text_faint()),
        )
            .on_hover_text("Params are optional. Add one if your hook has tunable values.");
        return;
    }

    let mut remove_idx: Option<usize> = None;

    for i in 0..params_len {
        let Some(h) = app.selected_hook.as_mut() else { break };
        let Some(p) = h.params.get_mut(i) else { continue };

        egui::Frame::none()
            .fill(theme::bg())
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("key")
                            .size(theme::sz(10.0))
                            .color(theme::text_faint()),
                    )
                        .on_hover_text("Machine name. The DLL sees this as a JSON key.");
                    ui.add(
                        egui::TextEdit::singleline(&mut p.key)
                            .desired_width(120.0)
                            .margin(egui::Margin::symmetric(6.0, 4.0)),
                    );

                    ui.add_space(6.0);

                    ui.label(
                        egui::RichText::new("label")
                            .size(theme::sz(10.0))
                            .color(theme::text_faint()),
                    )
                        .on_hover_text("Human-readable label shown in the UI.");
                    ui.add(
                        egui::TextEdit::singleline(&mut p.label)
                            .desired_width(160.0)
                            .margin(egui::Margin::symmetric(6.0, 4.0)),
                    );

                    ui.add_space(6.0);

                    let prev_kind = p.kind;
                    egui::ComboBox::from_id_source(format!("param_kind_{}", i))
                        .selected_text(p.kind.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for k in ParamKind::all() {
                                ui.selectable_value(&mut p.kind, k, k.label())
                                    .on_hover_text(match k {
                                        ParamKind::Float => "Floating-point number.",
                                        ParamKind::Int => "Integer.",
                                        ParamKind::Bool => "Boolean toggle.",
                                        ParamKind::String => "Text value.",
                                    });
                            }
                        });
                    if p.kind != prev_kind {
                        p.value = ParamValue::default_for(p.kind);
                        p.default = ParamValue::default_for(p.kind);
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let remove_resp = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("×")
                                        .size(theme::sz(14.0))
                                        .color(theme::blocked()),
                                )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .rounding(egui::Rounding::same(4.0)),
                            );
                            let remove_clicked = remove_resp.clicked();
                            remove_resp.on_hover_text("Remove this parameter.");
                            if remove_clicked {
                                remove_idx = Some(i);
                            }
                        },
                    );
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("value")
                            .size(theme::sz(10.0))
                            .color(theme::text_faint()),
                    )
                        .on_hover_text("Current value sent to the DLL.");

                    match &mut p.value {
                        ParamValue::Float(v) => {
                            let mut f = *v as f32;
                            let min = p.min.unwrap_or(-1_000_000.0) as f32;
                            let max = p.max.unwrap_or(1_000_000.0) as f32;
                            let slider_resp = ui.add(
                                egui::Slider::new(&mut f, min..=max)
                                    .fixed_decimals(2),
                            );
                            slider_resp.on_hover_text("Drag to change the value.");
                            *v = f as f64;
                        }
                        ParamValue::Int(v) => {
                            let min = p.min.unwrap_or(-1_000_000.0) as i64;
                            let max = p.max.unwrap_or(1_000_000.0) as i64;
                            let mut iv = *v;
                            let dv_resp = ui.add(
                                egui::DragValue::new(&mut iv).clamp_range(min..=max),
                            );
                            dv_resp.on_hover_text("Drag or type to change the value.");
                            *v = iv;
                        }
                        ParamValue::Bool(v) => {
                            let cb_resp = ui.checkbox(v, "on");
                            cb_resp.on_hover_text("Toggle the boolean value.");
                        }
                        ParamValue::String(v) => {
                            let te_resp = ui.add(
                                egui::TextEdit::singleline(v)
                                    .desired_width(300.0)
                                    .margin(egui::Margin::symmetric(6.0, 4.0)),
                            );
                            te_resp.on_hover_text("Text value sent to the DLL.");
                        }
                    }
                });
            });

        ui.add_space(6.0);
    }

    if let Some(i) = remove_idx {
        if let Some(h) = app.selected_hook.as_mut() {
            if i < h.params.len() {
                h.params.remove(i);
            }
        }
    }
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
        save_resp.on_hover_text("Write hook.json back to disk.");
        if save_clicked {
            app.save_selected_hook();
        }

        ui.add_space(8.0);

        let has_dll = app
            .selected_hook
            .as_ref()
            .map(|h| hooks::hook_dll_path(h).exists())
            .unwrap_or(false);

        let inject_btn = egui::Button::new(
            egui::RichText::new("Inject now")
                .size(theme::sz(13.0))
                .strong()
                .color(if has_dll {
                    egui::Color32::WHITE
                } else {
                    theme::text_faint()
                }),
        )
            .fill(if has_dll {
                theme::accent()
            } else {
                theme::panel_hover()
            })
            .rounding(egui::Rounding::same(10.0))
            .min_size(egui::vec2(140.0, 36.0));

        let inject_resp = ui.add_enabled(has_dll, inject_btn);
        let inject_clicked = inject_resp.clicked();
        inject_resp.on_hover_text(if has_dll {
            "Inject this hook into the current target PID and send its params."
        } else {
            "No compiled DLL found in this hook's folder."
        });
        if inject_clicked {
            if let Some(h) = app.selected_hook.clone() {
                app.inject_hook_by_name(&h.name);
            }
        }

        ui.add_space(8.0);

        let delete_btn = egui::Button::new(
            egui::RichText::new("Delete hook")
                .size(theme::sz(13.0))
                .color(theme::blocked()),
        )
            .fill(theme::panel_hover())
            .rounding(egui::Rounding::same(10.0))
            .min_size(egui::vec2(140.0, 36.0));

        let delete_resp = ui.add(delete_btn);
        let delete_clicked = delete_resp.clicked();
        delete_resp.on_hover_text("Delete this hook folder. Cannot be undone.");
        if delete_clicked {
            app.delete_selected_hook();
        }
    });
}

// ============================================================
// TAB: New Hook
// ============================================================

fn new_hook_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(24.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("CREATE A NEW HOOK")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            )
                .on_hover_text("Scaffolds a folder under %APPDATA%\\Sigil\\hooks.");

            ui.add_space(6.0);

            ui.label(
                egui::RichText::new(
                    "A hook is a folder with a hook.json, a compiled DLL, and optional source.",
                )
                    .size(theme::sz(12.0))
                    .color(theme::text_faint()),
            );

            ui.add_space(16.0);

            let name_resp = ui.add(
                egui::TextEdit::singleline(&mut app.new_hook_name)
                    .hint_text("hook name")
                    .desired_width(360.0)
                    .margin(egui::Margin::symmetric(10.0, 8.0)),
            );
            name_resp.on_hover_text("Short name. Used as the folder name.");

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Language")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text(
                        "Which template to use if you later open the source in the editor.",
                    );

                egui::ComboBox::from_id_source("new_hook_language")
                    .selected_text(app.new_hook_language.label())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for lang in crate::core::code_editor::CodeLanguage::all() {
                            ui.selectable_value(
                                &mut app.new_hook_language,
                                lang,
                                lang.label(),
                            )
                                .on_hover_text("Template language for the source file.");
                        }
                    });
            });

            ui.add_space(14.0);

            ui.horizontal(|ui| {
                let create_btn = egui::Button::new(
                    egui::RichText::new("Create")
                        .size(theme::sz(13.0))
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                    .fill(theme::accent())
                    .rounding(egui::Rounding::same(10.0))
                    .min_size(egui::vec2(140.0, 38.0));

                let create_resp = ui.add(create_btn);
                let create_clicked = create_resp.clicked();
                create_resp
                    .on_hover_text("Create the hook folder and switch to Edit Hook.");
                if create_clicked && !app.new_hook_name.trim().is_empty() {
                    let name = app.new_hook_name.clone();
                    app.new_hook(&name);
                    app.new_hook_name.clear();
                }

                ui.add_space(8.0);

                let clear_btn = egui::Button::new(
                    egui::RichText::new("Clear")
                        .size(theme::sz(12.0))
                        .color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(10.0))
                    .min_size(egui::vec2(100.0, 38.0));

                let clear_resp = ui.add(clear_btn);
                let clear_clicked = clear_resp.clicked();
                clear_resp.on_hover_text("Clear the name field.");
                if clear_clicked {
                    app.new_hook_name.clear();
                }
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label(
                egui::RichText::new("WHAT GETS CREATED")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            );

            ui.add_space(8.0);

            for line in [
                "hook.json — metadata, injection method, and params",
                "the folder itself lives under %APPDATA%\\Sigil\\hooks\\",
                "add your compiled hook.dll when you're ready to inject",
                "source files can be created and edited in the app later",
            ] {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("•")
                            .size(theme::sz(12.0))
                            .color(theme::accent()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(line)
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    );
                });
            }
        });
}

// ============================================================
// DOCKED EDITOR PANEL
// ============================================================

fn editor_panel_docked(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            draw_editor_only(app, ui);
        });
}

fn editor_tab_bar(app: &mut App, ui: &mut egui::Ui) {
    struct TabSnap {
        is_active: bool,
        dirty: bool,
        file_name: String,
    }

    let snap: Vec<TabSnap> = {
        let e = app.editor();
        e.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| TabSnap {
                is_active: e.active == Some(i),
                dirty: t.is_dirty(),
                file_name: t.file_name(),
            })
            .collect()
    };

    let mut click_activate: Option<usize> = None;
    let mut click_close: Option<usize> = None;
    let mut menu_action: Option<TabMenuAction> = None;

    egui::ScrollArea::horizontal()
        .id_source("editor_tab_strip")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, t) in snap.iter().enumerate() {
                    let fill = if t.is_active {
                        theme::accent_dim()
                    } else {
                        theme::bg()
                    };
                    let text_color = if t.is_active {
                        egui::Color32::WHITE
                    } else {
                        theme::text_dim()
                    };

                    let label = if t.dirty {
                        format!("● {}", t.file_name)
                    } else {
                        format!("  {}", t.file_name)
                    };

                    let tab_btn = egui::Button::new(
                        egui::RichText::new(label)
                            .size(theme::sz(11.0))
                            .color(text_color),
                    )
                        .fill(fill)
                        .rounding(egui::Rounding::same(6.0))
                        .min_size(egui::vec2(0.0, 26.0));

                    let resp = ui.add(tab_btn);

                    if resp.clicked() {
                        click_activate = Some(i);
                    }

                    resp.context_menu(|ui| {
                        if ui.button("Close").clicked() {
                            menu_action = Some(TabMenuAction::Close(i));
                            ui.close_menu();
                        }
                        if ui.button("Close others").clicked() {
                            menu_action = Some(TabMenuAction::CloseOthers(i));
                            ui.close_menu();
                        }
                        if ui.button("Close all").clicked() {
                            menu_action = Some(TabMenuAction::CloseAll);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Reveal in Explorer").clicked() {
                            menu_action = Some(TabMenuAction::Reveal(i));
                            ui.close_menu();
                        }
                        if ui.button("Open in external editor").clicked() {
                            menu_action = Some(TabMenuAction::External(i));
                            ui.close_menu();
                        }
                    });

                    let close_btn = egui::Button::new(
                        egui::RichText::new("×")
                            .size(theme::sz(12.0))
                            .color(theme::text_faint()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(4.0))
                        .min_size(egui::vec2(18.0, 18.0));

                    let close_resp = ui.add(close_btn);
                    let close_clicked = close_resp.clicked();
                    close_resp.on_hover_text("Close this tab.");
                    if close_clicked {
                        click_close = Some(i);
                    }

                    ui.add_space(4.0);
                }
            });
        });

    if let Some(i) = click_activate {
        app.editor().active = Some(i);
    }

    if let Some(i) = click_close {
        request_close_tab(app, i);
    }

    if let Some(action) = menu_action {
        apply_tab_menu_action(app, action);
    }
}

enum TabMenuAction {
    Close(usize),
    CloseOthers(usize),
    CloseAll,
    Reveal(usize),
    External(usize),
}

fn apply_tab_menu_action(app: &mut App, action: TabMenuAction) {
    match action {
        TabMenuAction::Close(i) => request_close_tab(app, i),
        TabMenuAction::CloseOthers(i) => {
            let dirty_others = {
                let e = app.editor();
                e.tabs
                    .iter()
                    .enumerate()
                    .any(|(idx, t)| idx != i && t.is_dirty())
            };
            if dirty_others {
                log::warn!(
                    target: "editor",
                    "close others skipped: some tabs have unsaved changes"
                );
            } else {
                app.editor().close_others(i);
            }
        }
        TabMenuAction::CloseAll => {
            let dirty = app.editor().has_dirty();
            if dirty {
                log::warn!(
                    target: "editor",
                    "close all skipped: some tabs have unsaved changes"
                );
            } else {
                app.editor().close_all();
            }
        }
        TabMenuAction::Reveal(i) => {
            let path = {
                let e = app.editor();
                e.tabs.get(i).map(|t| t.path.clone())
            };
            if let Some(path) = path {
                open_folder_in_explorer(path.parent().unwrap_or(&path));
            }
        }
        TabMenuAction::External(i) => {
            let path = {
                let e = app.editor();
                e.tabs.get(i).map(|t| t.path.clone())
            };
            if let Some(path) = path {
                app.open_path_with_picker(&path);
            }
        }
    }
}

fn request_close_tab(app: &mut App, index: usize) {
    let (len, dirty) = {
        let e = app.editor();
        (
            e.tabs.len(),
            e.tabs.get(index).map(|t| t.is_dirty()).unwrap_or(false),
        )
    };
    if index >= len {
        return;
    }
    if dirty {
        app.pending_close_tab = Some(index);
    } else {
        app.editor().close_at(index);
    }
}

fn editor_toolbar(app: &mut App, ui: &mut egui::Ui) {
    let snap = {
        let e = app.editor();
        e.active_buffer().map(|t| {
            (
                t.path.to_string_lossy().to_string(),
                t.file_name(),
                t.is_dirty(),
                t.language,
            )
        })
    };
    let Some((path_str, file_name, dirty, lang)) = snap else {
        return;
    };

    let floating = app.editor().floating;

    let ctrl_s = ui
        .ctx()
        .input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&file_name)
                .size(theme::sz(12.0))
                .strong()
                .color(theme::text()),
        )
            .on_hover_text(&path_str);

        if dirty {
            ui.label(
                egui::RichText::new("●")
                    .size(theme::sz(12.0))
                    .color(theme::warn()),
            )
                .on_hover_text("Unsaved changes.");
        }

        ui.add_space(10.0);

        ui.label(
            egui::RichText::new("Language")
                .size(theme::sz(10.0))
                .color(theme::text_dim()),
        )
            .on_hover_text("Syntax highlighting for this tab.");

        let mut new_lang = lang;
        egui::ComboBox::from_id_source("editor_language")
            .selected_text(lang.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for l in CodeLanguage::all() {
                    ui.selectable_value(&mut new_lang, l, l.label())
                        .on_hover_text("Change the highlighter for this tab.");
                }
            });

        if new_lang != lang {
            if let Some(tab) = app.editor().active_buffer_mut() {
                tab.language = new_lang;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close_active = egui::Button::new(
                egui::RichText::new("close tab")
                    .size(theme::sz(11.0))
                    .color(theme::text_dim()),
            )
                .fill(egui::Color32::TRANSPARENT)
                .rounding(egui::Rounding::same(6.0));

            let close_active_resp = ui.add(close_active);
            let close_active_clicked = close_active_resp.clicked();
            close_active_resp.on_hover_text("Close the active tab.");
            if close_active_clicked {
                let active = app.editor().active;
                if let Some(i) = active {
                    request_close_tab(app, i);
                }
            }

            ui.add_space(6.0);

            let ext_btn = egui::Button::new(
                egui::RichText::new("open with…")
                    .size(theme::sz(11.0))
                    .color(theme::text()),
            )
                .fill(theme::panel_hover())
                .rounding(egui::Rounding::same(6.0));

            let ext_resp = ui.add(ext_btn);
            let ext_clicked = ext_resp.clicked();
            ext_resp.on_hover_text(
                "Choose which app to open this file with. Windows remembers if you tick 'Always use this app'.",
            );
            if ext_clicked {
                app.open_active_with_picker();
            }

            ui.add_space(6.0);

            let (toggle_label, toggle_tip) = if floating {
                ("dock", "Close the editor window and go back to the docked panel.")
            } else {
                ("pop out", "Open the editor as a real OS window you can move to another monitor.")
            };

            let toggle_btn = egui::Button::new(
                egui::RichText::new(toggle_label)
                    .size(theme::sz(11.0))
                    .color(theme::accent()),
            )
                .fill(egui::Color32::TRANSPARENT)
                .rounding(egui::Rounding::same(6.0));

            let toggle_resp = ui.add(toggle_btn);
            let toggle_clicked = toggle_resp.clicked();
            toggle_resp.on_hover_text(toggle_tip);
            if toggle_clicked {
                let new = !app.editor().floating;
                app.editor().floating = new;
            }

            ui.add_space(6.0);

            let save_btn = egui::Button::new(
                egui::RichText::new(if dirty { "Save *" } else { "Save" })
                    .size(theme::sz(11.0))
                    .strong()
                    .color(egui::Color32::WHITE),
            )
                .fill(theme::accent())
                .rounding(egui::Rounding::same(6.0));

            let save_resp = ui.add(save_btn);
            let save_clicked = save_resp.clicked();
            save_resp.on_hover_text("Write the current buffer back to disk (Ctrl+S).");
            if save_clicked || ctrl_s {
                app.save_active_editor();
            }
        });
    });
}

fn editor_body(app: &mut App, ui: &mut egui::Ui, height: f32) {
    let (mut content, _lang) = {
        let e = app.editor();
        match e.active_buffer() {
            Some(tab) => (tab.content.clone(), tab.language),
            None => return,
        }
    };

    let h = height.max(120.0);

    egui::Frame::none()
        .fill(theme::bg())
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::same(6.0))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_source("hook_editor_scroll")
                .auto_shrink([false, false])
                .max_height(h)
                .show(ui, |ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), h),
                        egui::TextEdit::multiline(&mut content)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                });
        });

    let mut e = app.editor();
    if let Some(tab) = e.active_buffer_mut() {
        if tab.content != content {
            tab.content = content;
        }
    }
}


fn close_dirty_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(index) = app.pending_close_tab else {
        return;
    };

    let (len, file_name) = {
        let e = app.editor();
        (e.tabs.len(), e.tabs.get(index).map(|t| t.file_name()))
    };

    if index >= len {
        app.pending_close_tab = None;
        return;
    }
    let Some(file_name) = file_name else {
        app.pending_close_tab = None;
        return;
    };

    let mut still_open = true;
    let mut do_save = false;
    let mut do_discard = false;

    egui::Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "'{}' has unsaved changes. Save before closing?",
                    file_name
                ))
                    .size(theme::sz(12.0))
                    .color(theme::text()),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let save_btn = egui::Button::new(
                    egui::RichText::new("Save")
                        .size(theme::sz(12.0))
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                    .fill(theme::accent())
                    .rounding(egui::Rounding::same(8.0));

                let save_resp = ui.add(save_btn);
                let save_clicked = save_resp.clicked();
                save_resp.on_hover_text("Save the buffer, then close the tab.");
                if save_clicked {
                    do_save = true;
                    still_open = false;
                }

                ui.add_space(6.0);

                let discard_btn = egui::Button::new(
                    egui::RichText::new("Discard")
                        .size(theme::sz(12.0))
                        .color(theme::blocked()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(8.0));

                let discard_resp = ui.add(discard_btn);
                let discard_clicked = discard_resp.clicked();
                discard_resp.on_hover_text("Throw away changes and close the tab.");
                if discard_clicked {
                    do_discard = true;
                    still_open = false;
                }

                ui.add_space(6.0);

                let cancel_btn = egui::Button::new(
                    egui::RichText::new("Cancel")
                        .size(theme::sz(12.0))
                        .color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(8.0));

                let cancel_resp = ui.add(cancel_btn);
                let cancel_clicked = cancel_resp.clicked();
                cancel_resp.on_hover_text("Keep the tab open.");
                if cancel_clicked {
                    still_open = false;
                }
            });
        });

    if do_save {
        {
            let mut e = app.editor();
            if let Some(tab) = e.tabs.get_mut(index) {
                if let Err(err) = tab.save() {
                    log::error!(target: "editor", "{}", err);
                }
            }
            e.close_at(index);
        }
        app.pending_close_tab = None;
    } else if do_discard {
        app.editor().close_at(index);
        app.pending_close_tab = None;
    } else if !still_open {
        app.pending_close_tab = None;
    }
}

// ============================================================
// FLOATING WINDOW EDITOR (shared state)
// ============================================================

fn editor_tab_bar_shared(
    ui: &mut egui::Ui,
    editor: &std::sync::Arc<std::sync::Mutex<crate::core::code_editor::EditorState>>,
) {
    struct TabSnap {
        is_active: bool,
        dirty: bool,
        file_name: String,
    }

    let snap: Vec<TabSnap> = {
        let e = editor.lock().unwrap();
        e.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| TabSnap {
                is_active: e.active == Some(i),
                dirty: t.is_dirty(),
                file_name: t.file_name(),
            })
            .collect()
    };

    let mut click_activate: Option<usize> = None;
    let mut click_close: Option<usize> = None;

    egui::ScrollArea::horizontal()
        .id_source("editor_tab_strip_float")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, t) in snap.iter().enumerate() {
                    let fill = if t.is_active {
                        theme::accent_dim()
                    } else {
                        theme::bg()
                    };
                    let text_color = if t.is_active {
                        egui::Color32::WHITE
                    } else {
                        theme::text_dim()
                    };

                    let label = if t.dirty {
                        format!("● {}", t.file_name)
                    } else {
                        format!("  {}", t.file_name)
                    };

                    let tab_btn = egui::Button::new(
                        egui::RichText::new(label)
                            .size(theme::sz(11.0))
                            .color(text_color),
                    )
                        .fill(fill)
                        .rounding(egui::Rounding::same(6.0))
                        .min_size(egui::vec2(0.0, 26.0));

                    let resp = ui.add(tab_btn);
                    if resp.clicked() {
                        click_activate = Some(i);
                    }

                    let close_btn = egui::Button::new(
                        egui::RichText::new("×")
                            .size(theme::sz(12.0))
                            .color(theme::text_faint()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(4.0))
                        .min_size(egui::vec2(18.0, 18.0));

                    let close_resp = ui.add(close_btn);
                    let close_clicked = close_resp.clicked();
                    close_resp.on_hover_text("Close this tab.");
                    if close_clicked {
                        click_close = Some(i);
                    }

                    ui.add_space(4.0);
                }
            });
        });

    if let Some(i) = click_activate {
        editor.lock().unwrap().active = Some(i);
    }
    if let Some(i) = click_close {
        let dirty = editor
            .lock()
            .unwrap()
            .tabs
            .get(i)
            .map(|t| t.is_dirty())
            .unwrap_or(false);
        if !dirty {
            editor.lock().unwrap().close_at(i);
        }
    }
}

fn editor_toolbar_shared(
    ui: &mut egui::Ui,
    editor: &std::sync::Arc<std::sync::Mutex<crate::core::code_editor::EditorState>>,
) {
    let snap = {
        let e = editor.lock().unwrap();
        e.active_buffer().map(|t| {
            (
                t.path.to_string_lossy().to_string(),
                t.file_name(),
                t.is_dirty(),
                t.language,
            )
        })
    };
    let Some((path_str, file_name, dirty, lang)) = snap else {
        return;
    };

    let ctrl_s = ui
        .ctx()
        .input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&file_name)
                .size(theme::sz(12.0))
                .strong()
                .color(theme::text()),
        )
            .on_hover_text(&path_str);

        if dirty {
            ui.label(
                egui::RichText::new("●")
                    .size(theme::sz(12.0))
                    .color(theme::warn()),
            )
                .on_hover_text("Unsaved changes.");
        }

        ui.add_space(10.0);

        ui.label(
            egui::RichText::new("Language")
                .size(theme::sz(10.0))
                .color(theme::text_dim()),
        )
            .on_hover_text("Syntax highlighting for this tab.");

        let mut new_lang = lang;
        egui::ComboBox::from_id_source("editor_language_float")
            .selected_text(lang.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for l in CodeLanguage::all() {
                    ui.selectable_value(&mut new_lang, l, l.label());
                }
            });
        if new_lang != lang {
            if let Some(tab) = editor.lock().unwrap().active_buffer_mut() {
                tab.language = new_lang;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close_active = egui::Button::new(
                egui::RichText::new("close tab")
                    .size(theme::sz(11.0))
                    .color(theme::text_dim()),
            )
                .fill(egui::Color32::TRANSPARENT)
                .rounding(egui::Rounding::same(6.0));

            let close_active_resp = ui.add(close_active);
            let close_active_clicked = close_active_resp.clicked();
            close_active_resp.on_hover_text("Close the active tab.");
            if close_active_clicked {
                let active = editor.lock().unwrap().active;
                if let Some(i) = active {
                    let dirty = editor
                        .lock()
                        .unwrap()
                        .tabs
                        .get(i)
                        .map(|t| t.is_dirty())
                        .unwrap_or(false);
                    if !dirty {
                        editor.lock().unwrap().close_at(i);
                    }
                }
            }

            ui.add_space(6.0);

            let dock_btn = egui::Button::new(
                egui::RichText::new("dock")
                    .size(theme::sz(11.0))
                    .color(theme::accent()),
            )
                .fill(egui::Color32::TRANSPARENT)
                .rounding(egui::Rounding::same(6.0));

            let dock_resp = ui.add(dock_btn);
            let dock_clicked = dock_resp.clicked();
            dock_resp.on_hover_text("Close this window and go back to the docked panel.");
            if dock_clicked {
                editor.lock().unwrap().floating = false;
            }

            ui.add_space(6.0);

            let save_btn = egui::Button::new(
                egui::RichText::new(if dirty { "Save *" } else { "Save" })
                    .size(theme::sz(11.0))
                    .strong()
                    .color(egui::Color32::WHITE),
            )
                .fill(theme::accent())
                .rounding(egui::Rounding::same(6.0));

            let save_resp = ui.add(save_btn);
            let save_clicked = save_resp.clicked();
            save_resp.on_hover_text("Write the current buffer back to disk (Ctrl+S).");
            if save_clicked || ctrl_s {
                let mut e = editor.lock().unwrap();
                if let Some(tab) = e.active_buffer_mut() {
                    if let Err(err) = tab.save() {
                        log::error!(target: "editor", "{}", err);
                    }
                }
            }
        });
    });
}

fn editor_body_shared(
    ui: &mut egui::Ui,
    editor: &std::sync::Arc<std::sync::Mutex<crate::core::code_editor::EditorState>>,
    height: f32,
) {
    let (mut content, _lang) = {
        let e = editor.lock().unwrap();
        match e.active_buffer() {
            Some(tab) => (tab.content.clone(), tab.language),
            None => return,
        }
    };

    let h = height.max(120.0);

    egui::Frame::none()
        .fill(theme::bg())
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::same(6.0))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_source("hook_editor_scroll_float")
                .auto_shrink([false, false])
                .max_height(h)
                .show(ui, |ui| {
                    ui.add_sized(
                        egui::vec2(ui.available_width(), h),
                        egui::TextEdit::multiline(&mut content)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                });
        });

    let mut e = editor.lock().unwrap();
    if let Some(tab) = e.active_buffer_mut() {
        if tab.content != content {
            tab.content = content;
        }
    }
}

// ============================================================
// HELPERS
// ============================================================

fn rfd_pick_folder() -> Option<std::path::PathBuf> {
    use std::process::Command;

    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.FolderBrowserDialog
$dlg.Description = "Select a folder to import as a hook"
if ($dlg.ShowDialog() -eq 'OK') { Write-Output $dlg.SelectedPath }
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", script])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(text))
}

fn open_folder_in_explorer(path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::process::Command;
        let _ = Command::new("explorer").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}