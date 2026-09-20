use crate::core::session::{self, ChangelogKind};
use crate::state::{App, SessionsTab};
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    if !app.project_list_loaded {
        app.refresh_project_list();
    }

    egui::ScrollArea::vertical()
        .id_source("sessions_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Projects")
                    .size(theme::sz(20.0))
                    .strong()
                    .color(theme::text()),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Every session is a project. Renames, patches, and settings are logged.",
                )
                    .size(theme::sz(12.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text(
                    "Projects live under your Documents\\Sigil\\Projects folder. Each one stores its own config, appearance snapshot, and changelog.",
                );

            ui.add_space(16.0);

            // ---- TAB BAR ----
            ui.horizontal(|ui| {
                let tabs = [
                    SessionsTab::OpenProject,
                    SessionsTab::AllProjects,
                    SessionsTab::NewProject,
                ];

                for tab in tabs {
                    let selected = app.sessions_tab == tab;
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
                        SessionsTab::OpenProject => {
                            "Show the project you have open. Name, notes, and changelog."
                        }
                        SessionsTab::AllProjects => {
                            "Browse every project on disk. Open or delete them from here."
                        }
                        SessionsTab::NewProject => "Create a new project folder.",
                    });

                    if clicked {
                        app.sessions_tab = tab;
                    }

                    ui.add_space(6.0);
                }
            });

            ui.add_space(14.0);

            // ---- TAB CONTENT ----
            match app.sessions_tab {
                SessionsTab::OpenProject => open_project_tab(app, ui),
                SessionsTab::AllProjects => all_projects_tab(app, ui),
                SessionsTab::NewProject => new_project_tab(app, ui),
            }
        });
}

// ============================================================
// TAB: Open Project
// ============================================================

fn open_project_tab(app: &mut App, ui: &mut egui::Ui) {
    let Some(project) = app.current_project.clone() else {
        egui::Frame::none()
            .fill(theme::panel())
            .rounding(egui::Rounding::same(theme::corner()))
            .inner_margin(egui::Margin::same(24.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("No project open")
                            .size(theme::sz(16.0))
                            .strong()
                            .color(theme::text_dim()),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Head to the New Project tab to create one, or use All Projects to open an existing one.",
                        )
                            .size(theme::sz(12.0))
                            .color(theme::text_faint()),
                    );
                    ui.add_space(20.0);
                });
            });
        return;
    };

    let (status_text, status_color, status_tip) = if app.project_dirty {
        if app.settings.project_autosave {
            (
                "saving...",
                theme::warn(),
                "Changes are pending and will be written this frame.",
            )
        } else {
            (
                "unsaved",
                theme::warn(),
                "Autosave is off. Click Save to write changes to disk.",
            )
        }
    } else {
        ("saved", theme::ok(), "Everything is written to disk.")
    };

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            // ---- Header row ----
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&project.name)
                        .size(theme::sz(16.0))
                        .strong()
                        .color(theme::text()),
                )
                    .on_hover_text("The open project.");

                ui.add_space(10.0);

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
                    let close_btn = egui::Button::new(
                        egui::RichText::new("close")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));

                    let close_resp = ui.add(close_btn);
                    let close_clicked = close_resp.clicked();
                    close_resp.on_hover_text("Close the current project. It stays on disk.");
                    if close_clicked {
                        app.close_project();
                    }

                    ui.add_space(6.0);

                    let save_btn = egui::Button::new(
                        egui::RichText::new("save")
                            .size(theme::sz(11.0))
                            .color(theme::text()),
                    )
                        .fill(theme::panel_hover())
                        .rounding(egui::Rounding::same(6.0));

                    let save_resp = ui.add(save_btn);
                    let save_clicked = save_resp.clicked();
                    save_resp.on_hover_text("Force a write to disk even if autosave is off.");
                    if save_clicked {
                        app.save_project();
                    }
                });
            });

            ui.add_space(14.0);

            // ---- Rename row ----
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Name")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Project name. Editable.");

                let name_resp = ui.add(
                    egui::TextEdit::singleline(&mut app.rename_buffer)
                        .desired_width(300.0)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                );
                name_resp.on_hover_text("Edit the name and press Rename.");

                ui.add_space(6.0);

                let rename_btn = egui::Button::new(
                    egui::RichText::new("Rename")
                        .size(theme::sz(11.0))
                        .color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(6.0));

                let rename_resp = ui.add(rename_btn);
                let rename_clicked = rename_resp.clicked();
                rename_resp.on_hover_text("Update the project name. Does not rename the folder.");
                if rename_clicked && !app.rename_buffer.trim().is_empty() {
                    let new_name = app.rename_buffer.clone();
                    app.rename_current_project(&new_name);
                }
            });

            ui.add_space(10.0);

            // ---- Folder row ----
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Folder")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Where this project lives on disk.");

                let folder_str = project.folder.to_string_lossy().to_string();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&folder_str)
                            .size(theme::sz(11.0))
                            .monospace()
                            .color(theme::text()),
                    )
                        .sense(egui::Sense::click()),
                )
                    .on_hover_text(&folder_str);

                let copy_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new("copy")
                            .size(theme::sz(10.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(4.0)),
                );
                let copy_clicked = copy_resp.clicked();
                copy_resp.on_hover_text("Copy the folder path to the clipboard.");
                if copy_clicked {
                    ui.output_mut(|o| o.copied_text = folder_str);
                }
            });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Created")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("When this project was first created.");
                ui.label(
                    egui::RichText::new(&project.created)
                        .size(theme::sz(11.0))
                        .monospace()
                        .color(theme::text()),
                );

                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new("Modified")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Last time the project file was written.");
                ui.label(
                    egui::RichText::new(&project.modified)
                        .size(theme::sz(11.0))
                        .monospace()
                        .color(theme::text()),
                );
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            // ---- Notes ----
            ui.label(
                egui::RichText::new("NOTES")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            )
                .on_hover_text("Free-form notes saved with the project.");

            ui.add_space(6.0);

            let mut notes = app
                .current_project
                .as_ref()
                .map(|p| p.notes.clone())
                .unwrap_or_default();
            let notes_resp = ui.add(
                egui::TextEdit::multiline(&mut notes)
                    .desired_width(f32::INFINITY)
                    .desired_rows(5)
                    .margin(egui::Margin::symmetric(8.0, 6.0)),
            );
            let notes_changed = notes_resp.changed();
            notes_resp.on_hover_text("Changes are autosaved when you edit here.");
            if notes_changed {
                if let Some(p) = app.current_project.as_mut() {
                    p.notes = notes;
                }
                app.touch_project();
            }

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            changelog_section(ui, &project.folder);
        });
}

fn changelog_section(ui: &mut egui::Ui, folder: &std::path::Path) {
    let entries = session::read_changelog(folder);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("CHANGELOG")
                .size(theme::sz(10.0))
                .strong()
                .color(theme::text_dim()),
        )
            .on_hover_text("Every rename, patch, and settings change is logged here.");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} entries", entries.len()))
                    .size(theme::sz(10.0))
                    .color(theme::text_faint()),
            )
                .on_hover_text("Total logged events for this project.");
        });
    });

    ui.add_space(6.0);

    if entries.is_empty() {
        ui.label(
            egui::RichText::new("nothing logged yet")
                .size(theme::sz(11.0))
                .italics()
                .color(theme::text_faint()),
        )
            .on_hover_text("Actions you take in this project will appear here.");
        return;
    }

    egui::Frame::none()
        .fill(theme::bg())
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_source("changelog_scroll")
                .auto_shrink([false, false])
                .max_height(240.0)
                .show(ui, |ui| {
                    for entry in entries.iter().rev() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&entry.timestamp)
                                    .size(theme::sz(10.0))
                                    .monospace()
                                    .color(theme::text_faint()),
                            );

                            let (kind_color, kind_tip) = changelog_kind_style(&entry.kind);
                            ui.label(
                                egui::RichText::new(format!("[{:>6}]", entry.kind.label()))
                                    .size(theme::sz(10.0))
                                    .monospace()
                                    .color(kind_color),
                            )
                                .on_hover_text(kind_tip);

                            ui.label(
                                egui::RichText::new(&entry.detail)
                                    .size(theme::sz(11.0))
                                    .color(theme::text()),
                            );
                        });
                    }
                });
        });
}

fn changelog_kind_style(kind: &ChangelogKind) -> (egui::Color32, &'static str) {
    match kind {
        ChangelogKind::Created => (theme::ok(), "Project created"),
        ChangelogKind::Opened => (theme::info(), "Project opened"),
        ChangelogKind::Renamed => (theme::accent(), "Project renamed"),
        ChangelogKind::SettingChanged => (theme::text_dim(), "Setting changed"),
        ChangelogKind::Patched => (theme::warn(), "Binary patched"),
        ChangelogKind::AddressNamed => (theme::accent(), "Address given a name"),
        ChangelogKind::HookAdded => (theme::ok(), "Hook added to project"),
        ChangelogKind::HookRemoved => (theme::blocked(), "Hook removed from project"),
        ChangelogKind::Note => (theme::text_dim(), "Note"),
        ChangelogKind::Saved => (theme::ok(), "Manual save"),
    }
}

// ============================================================
// TAB: All Projects
// ============================================================

fn all_projects_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("ALL PROJECTS")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Every project on disk. Click one to open it.");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refresh_btn = egui::Button::new(
                        egui::RichText::new("refresh")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));

                    let refresh_resp = ui.add(refresh_btn);
                    let refresh_clicked = refresh_resp.clicked();
                    refresh_resp.on_hover_text("Re-read the projects folder from disk.");
                    if refresh_clicked {
                        app.refresh_project_list();
                    }
                });
            });

            ui.add_space(10.0);

            if app.project_list.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "no projects yet — create one from the New Project tab",
                    )
                        .size(theme::sz(12.0))
                        .italics()
                        .color(theme::text_faint()),
                )
                    .on_hover_text("The Projects folder is empty.");
                return;
            }

            let list = app.project_list.clone();
            let mut open: Option<std::path::PathBuf> = None;

            egui::ScrollArea::vertical()
                .id_source("project_list_scroll")
                .auto_shrink([false, false])
                .max_height(480.0)
                .show(ui, |ui| {
                    for summary in &list {
                        egui::Frame::none()
                            .fill(theme::bg())
                            .rounding(egui::Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&summary.name)
                                                .size(theme::sz(13.0))
                                                .strong()
                                                .color(theme::text()),
                                        );
                                        if let Some(target) = &summary.target_path {
                                            ui.label(
                                                egui::RichText::new(target)
                                                    .size(theme::sz(10.0))
                                                    .monospace()
                                                    .color(theme::text_faint()),
                                            );
                                        }
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "modified {}",
                                                summary.modified
                                            ))
                                                .size(theme::sz(10.0))
                                                .color(theme::text_faint()),
                                        );
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
                                                "Delete this project and everything inside it.",
                                            );
                                            if del_clicked {
                                                app.show_delete_confirm =
                                                    Some(summary.folder.clone());
                                            }

                                            ui.add_space(4.0);

                                            let open_resp = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("open")
                                                        .size(theme::sz(11.0))
                                                        .color(theme::text()),
                                                )
                                                    .fill(theme::panel_hover())
                                                    .rounding(egui::Rounding::same(6.0)),
                                            );
                                            let open_clicked = open_resp.clicked();
                                            open_resp.on_hover_text("Open this project.");
                                            if open_clicked {
                                                open = Some(summary.folder.clone());
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(6.0);
                    }
                });

            if let Some(folder) = open {
                app.open_project(&folder);
            }

            // Confirmation dialog for delete.
            if let Some(folder) = app.show_delete_confirm.clone() {
                let name = folder
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("this project")
                    .to_string();

                let mut still_open = true;
                let mut do_delete = false;

                egui::Window::new("Confirm delete")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ui.ctx(), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Delete '{}' and everything inside it?",
                                name
                            ))
                                .size(theme::sz(12.0))
                                .color(theme::text()),
                        )
                            .on_hover_text("This cannot be undone.");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let del_btn = egui::Button::new(
                                egui::RichText::new("Delete")
                                    .size(theme::sz(12.0))
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                                .fill(theme::error())
                                .rounding(egui::Rounding::same(8.0));

                            let del_resp = ui.add(del_btn);
                            let del_clicked = del_resp.clicked();
                            del_resp.on_hover_text("Permanently delete the project folder.");
                            if del_clicked {
                                do_delete = true;
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
                            cancel_resp.on_hover_text("Keep the project.");
                            if cancel_clicked {
                                still_open = false;
                            }
                        });
                    });

                if do_delete {
                    app.delete_project(&folder);
                    app.show_delete_confirm = None;
                } else if !still_open {
                    app.show_delete_confirm = None;
                }
            }
        });
}

// ============================================================
// TAB: New Project
// ============================================================

fn new_project_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(24.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("CREATE A NEW PROJECT")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            )
                .on_hover_text("A project is a folder that tracks everything you do to a target.");

            ui.add_space(6.0);

            ui.label(
                egui::RichText::new(
                    "The project stores your settings snapshot, notes, renames, and patches.",
                )
                    .size(theme::sz(12.0))
                    .color(theme::text_faint()),
            );

            ui.add_space(16.0);

            let name_resp = ui.add(
                egui::TextEdit::singleline(&mut app.new_project_name)
                    .hint_text("project name")
                    .desired_width(360.0)
                    .margin(egui::Margin::symmetric(10.0, 8.0)),
            );
            name_resp.on_hover_text("A short name for the project. Used as the folder name.");

            ui.add_space(12.0);

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
                create_resp.on_hover_text(
                    "Create the project and switch to the Open Project tab.",
                );
                if create_clicked && !app.new_project_name.trim().is_empty() {
                    let name = app.new_project_name.clone();
                    app.new_project(&name);
                    app.new_project_name.clear();
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
                    app.new_project_name.clear();
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
            )
                .on_hover_text("The folder layout of a new project.");

            ui.add_space(8.0);

            for line in [
                "project.json — name, target, appearance snapshot, notes",
                "changelog.json — every rename, patch, and setting change",
                "the folder itself lives under Documents\\Sigil\\Projects\\",
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