//! Memory view. Attach to a process, scan for values, browse regions,
//! edit bytes, freeze values, and run SigilScript programs.

use crate::core::memory::{ChangeDir, ValueType};
use crate::core::processes::{list_processes_sorted, ProcessListing};
use crate::state::{App, MemoryTab};
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    // Refresh live values once per frame. The method internally
    // throttles to ~30 Hz so we don't hammer the target.
    app.memory_refresh_visible_values();

    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

    page_header(app, ui);
    ui.add_space(6.0);
    attach_bar(app, ui);
    ui.add_space(4.0);
    tab_strip(app, ui);
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .id_source("memory_body_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match app.memory_tab {
                MemoryTab::Scan => scan_tab(app, ui),
                MemoryTab::Regions => regions_tab(app, ui),
                MemoryTab::Freeze => freeze_tab(app, ui),
                MemoryTab::Scripts => scripts_tab(app, ui),
            }
            ui.add_space(12.0);
            hex_panel(app, ui);
        });

    // Modal on top of everything.
    if app.mem_proc_picker_open {
        process_picker_modal(app, ui.ctx());
    }
}

// ============================================================
// PAGE HEADER
// ============================================================

fn page_header(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Memory")
                .size(theme::sz(22.0))
                .strong()
                .color(theme::text()),
        );
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("inspect · scan · freeze · script")
                .size(theme::sz(11.0))
                .italics()
                .color(theme::text_faint()),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(status) = &app.mem_status {
                ui.label(
                    egui::RichText::new(status)
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                );
            }
        });
    });
}

// ============================================================
// ATTACH BAR
// ============================================================

fn attach_bar(app: &mut App, ui: &mut egui::Ui) {
    let attached = app.mem_reader.lock().map(|g| g.is_some()).unwrap_or(false);

    let (accent, ring) = if attached {
        (theme::ok(), theme::ok())
    } else {
        (theme::text_faint(), theme::panel_hover())
    };

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
        .stroke(egui::Stroke::new(1.0, ring.gamma_multiply(0.35)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("●")
                        .size(theme::sz(18.0))
                        .color(accent),
                )
                    .on_hover_text(if attached {
                        "Attached with full VM read/write access."
                    } else {
                        "Not attached. Click Open process."
                    });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    let (title, subtitle) = if attached {
                        let pid = app
                            .mem_reader
                            .lock()
                            .ok()
                            .and_then(|g| g.as_ref().map(|r| r.pid));
                        let name = pid
                            .and_then(crate::core::processes::find_process_by_pid)
                            .map(|e| e.name)
                            .unwrap_or_else(|| "unknown".into());
                        (
                            name,
                            format!("PID {} · VM read/write enabled", pid.unwrap_or(0)),
                        )
                    } else {
                        (
                            "No process attached".to_string(),
                            "Click Open process to pick a target".to_string(),
                        )
                    };

                    ui.label(
                        egui::RichText::new(title)
                            .size(theme::sz(14.0))
                            .strong()
                            .color(theme::text()),
                    );
                    ui.label(
                        egui::RichText::new(subtitle)
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if attached {
                        let detach = egui::Button::new(
                            egui::RichText::new("Detach")
                                .size(theme::sz(12.0))
                                .color(theme::blocked()),
                        )
                            .fill(theme::bg())
                            .rounding(egui::Rounding::same(8.0))
                            .min_size(egui::vec2(90.0, 32.0));
                        let r = ui.add(detach);
                        let c = r.clicked();
                        r.on_hover_text("Close the handle and free any frozen values.");
                        if c {
                            app.memory_detach();
                        }
                        ui.add_space(8.0);
                    }

                    let open_label = if attached { "Change process" } else { "Open process" };
                    let btn = egui::Button::new(
                        egui::RichText::new(open_label)
                            .size(theme::sz(12.0))
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                        .fill(theme::accent())
                        .rounding(egui::Rounding::same(8.0))
                        .min_size(egui::vec2(140.0, 32.0));

                    let resp = ui.add(btn);
                    let clicked = resp.clicked();
                    resp.on_hover_text("Pick a running process to attach to.");
                    if clicked {
                        app.memory_open_proc_picker();
                    }
                });
            });
        });
}

// ============================================================
// TAB STRIP
// ============================================================

fn tab_strip(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::symmetric(6.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in [
                    MemoryTab::Scan,
                    MemoryTab::Regions,
                    MemoryTab::Freeze,
                    MemoryTab::Scripts,
                ] {
                    let selected = app.memory_tab == tab;
                    let fill = if selected {
                        theme::accent()
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let text_color = if selected {
                        egui::Color32::WHITE
                    } else {
                        theme::text_dim()
                    };

                    let btn = egui::Button::new(
                        egui::RichText::new(tab.label())
                            .size(theme::sz(12.0))
                            .color(text_color),
                    )
                        .fill(fill)
                        .rounding(egui::Rounding::same(8.0))
                        .min_size(egui::vec2(110.0, 30.0));

                    let resp = ui.add(btn);
                    let clicked = resp.clicked();
                    resp.on_hover_text(match tab {
                        MemoryTab::Scan => "Search for values by type and freeze them.",
                        MemoryTab::Regions => "Committed memory regions with protections.",
                        MemoryTab::Freeze => "Everything currently frozen, in one place.",
                        MemoryTab::Scripts => "Write and run SigilScript programs.",
                    });
                    if clicked {
                        app.memory_tab = tab;
                    }
                    ui.add_space(2.0);
                }
            });
        });
}

// ============================================================
// TAB: SCAN
// ============================================================

fn scan_tab(app: &mut App, ui: &mut egui::Ui) {
    let total_height = (ui.available_height() * 0.75).max(360.0);

    ui.horizontal_top(|ui| {
        // ---------------- Left: controls ----------------
        ui.allocate_ui_with_layout(
            egui::vec2(280.0, total_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::none()
                    .fill(theme::panel())
                    .rounding(egui::Rounding::same(theme::corner()))
                    .inner_margin(egui::Margin::same(16.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("SCAN CONTROLS")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.add_space(10.0);

                        ui.label(
                            egui::RichText::new("Type")
                                .size(theme::sz(10.0))
                                .color(theme::text_dim()),
                        );
                        let mut sel = app.mem_scan_type;
                        egui::ComboBox::from_id_source("mem_scan_type")
                            .selected_text(sel.label())
                            .width(ui.available_width() - 4.0)
                            .show_ui(ui, |ui| {
                                for t in ValueType::all() {
                                    ui.selectable_value(&mut sel, t, t.label())
                                        .on_hover_text(match t {
                                            ValueType::All => {
                                                "Try every numeric type at once."
                                            }
                                            ValueType::Bytes => "Raw bytes, e.g. 48 8B 05",
                                            ValueType::String => "ASCII text",
                                            _ => t.label(),
                                        });
                                }
                            });
                        if sel != app.mem_scan_type {
                            app.mem_scan_type = sel;
                            app.mem_scan_results.clear();
                            app.mem_scan_undo.clear();
                            app.mem_selected_hit = None;
                        }

                        ui.add_space(8.0);

                        ui.label(
                            egui::RichText::new("Value")
                                .size(theme::sz(10.0))
                                .color(theme::text_dim()),
                        );
                        ui.add_sized(
                            egui::vec2(ui.available_width(), 32.0),
                            egui::TextEdit::singleline(&mut app.mem_scan_input)
                                .hint_text(match app.mem_scan_type {
                                    ValueType::Bytes => "48 8B 05 ?? ??",
                                    ValueType::String => "hello",
                                    ValueType::All => "100",
                                    _ => "100",
                                })
                                .margin(egui::Margin::symmetric(10.0, 6.0)),
                        )
                            .on_hover_text(
                                "Value to search for. Filter the results with the box on the right.",
                            );

                        ui.add_space(14.0);

                        let can_scan = !app.mem_scan_in_progress;

                        let first_btn = egui::Button::new(
                            egui::RichText::new(if app.mem_scan_in_progress {
                                "Scanning…"
                            } else {
                                "First scan"
                            })
                                .size(theme::sz(12.0))
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                            .fill(theme::accent())
                            .rounding(egui::Rounding::same(8.0))
                            .min_size(egui::vec2(ui.available_width(), 34.0));

                        let resp = ui.add_enabled(can_scan, first_btn);
                        let clicked = resp.clicked();
                        resp.on_hover_text("Search every readable region.");
                        if clicked {
                            app.memory_start_scan();
                        }

                        ui.add_space(6.0);

                        let next_btn = egui::Button::new(
                            egui::RichText::new("Next scan")
                                .size(theme::sz(12.0))
                                .color(theme::text()),
                        )
                            .fill(theme::panel_hover())
                            .rounding(egui::Rounding::same(8.0))
                            .min_size(egui::vec2(ui.available_width(), 34.0));

                        let resp = ui.add(next_btn);
                        let clicked = resp.clicked();
                        resp.on_hover_text(
                            "Refine the current results: keep only addresses whose current value matches the new one.",
                        );
                        if clicked {
                            app.memory_refine_scan();
                        }

                        ui.add_space(6.0);

                        let can_undo = !app.mem_scan_undo.is_empty();
                        let undo_btn = egui::Button::new(
                            egui::RichText::new("Undo scan")
                                .size(theme::sz(12.0))
                                .color(if can_undo {
                                    theme::text()
                                } else {
                                    theme::text_faint()
                                }),
                        )
                            .fill(theme::panel_hover())
                            .rounding(egui::Rounding::same(8.0))
                            .min_size(egui::vec2(ui.available_width(), 32.0));

                        let resp = ui.add_enabled(can_undo, undo_btn);
                        let clicked = resp.clicked();
                        resp.on_hover_text("Restore the results from before the last refine.");
                        if clicked {
                            app.memory_undo_scan();
                        }

                        ui.add_space(6.0);

                        let clear_btn = egui::Button::new(
                            egui::RichText::new("New scan")
                                .size(theme::sz(12.0))
                                .color(theme::text_dim()),
                        )
                            .fill(egui::Color32::TRANSPARENT)
                            .rounding(egui::Rounding::same(8.0))
                            .min_size(egui::vec2(ui.available_width(), 30.0));

                        let resp = ui.add(clear_btn);
                        let clicked = resp.clicked();
                        resp.on_hover_text("Throw away the current results.");
                        if clicked {
                            app.memory_clear_results();
                        }

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}",
                                    app.mem_scan_results.len()
                                ))
                                    .size(theme::sz(20.0))
                                    .strong()
                                    .color(theme::accent()),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("hits")
                                    .size(theme::sz(11.0))
                                    .color(theme::text_dim()),
                            );
                        });

                        if let Some(err) = &app.mem_scan_error {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(err)
                                    .size(theme::sz(11.0))
                                    .color(theme::error()),
                            );
                        }

                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "values refresh live at ~30 Hz (first {} shown)",
                                500
                            ))
                                .size(theme::sz(10.0))
                                .italics()
                                .color(theme::text_faint()),
                        );
                    });
            },
        );

        ui.add_space(10.0);

        // ---------------- Right: results ----------------
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), total_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::none()
                    .fill(theme::panel())
                    .rounding(egui::Rounding::same(theme::corner()))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("RESULTS")
                                    .size(theme::sz(10.0))
                                    .strong()
                                    .color(theme::text_dim()),
                            );
                            ui.add_space(8.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut app.mem_results_filter)
                                    .hint_text("filter by value or address")
                                    .desired_width(260.0)
                                    .margin(egui::Margin::symmetric(8.0, 4.0)),
                            )
                                .on_hover_text(
                                    "Narrow the displayed rows. Doesn't affect the scan itself.",
                                );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if !app.mem_results_filter.is_empty() {
                                        let clear = egui::Button::new(
                                            egui::RichText::new("×")
                                                .size(theme::sz(12.0))
                                                .color(theme::text_dim()),
                                        )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .rounding(egui::Rounding::same(4.0))
                                            .min_size(egui::vec2(22.0, 22.0));
                                        let r = ui.add(clear);
                                        let c = r.clicked();
                                        if c {
                                            app.mem_results_filter.clear();
                                        }
                                    }
                                },
                            );
                        });

                        ui.add_space(8.0);

                        let mut toggle_freeze: Option<usize> = None;
                        let mut click_row: Option<usize> = None;
                        let mut follow_pointer: Option<u64> = None;
                        let mut disasm_at: Option<u64> = None;

                        let filter = app.mem_results_filter.trim().to_lowercase();
                        let has_filter = !filter.is_empty();

                        // Snapshot the rows we're going to render so we
                        // can iterate without holding a borrow on app.
                        #[derive(Clone)]
                        struct Row {
                            idx: usize,
                            address: u64,
                            value: String,
                            previous: Option<String>,
                            change: ChangeDir,
                            frozen: bool,
                            kind: ValueType,
                        }

                        let rows: Vec<Row> = app
                            .mem_scan_results
                            .iter()
                            .enumerate()
                            .filter(|(_, h)| {
                                if !has_filter {
                                    return true;
                                }
                                h.value.to_lowercase().contains(&filter)
                                    || format!("{:#x}", h.address).contains(&filter)
                            })
                            .take(500)
                            .map(|(i, h)| Row {
                                idx: i,
                                address: h.address,
                                value: h.value.clone(),
                                previous: h.previous.clone(),
                                change: h.change,
                                frozen: h.frozen,
                                kind: h.kind,
                            })
                            .collect();

                        egui::ScrollArea::vertical()
                            .id_source("mem_results_scroll")
                            .auto_shrink([false, false])
                            .max_height(total_height - 60.0)
                            .show(ui, |ui| {
                                egui::Grid::new("mem_results_grid")
                                    .striped(false)
                                    .num_columns(5)
                                    .spacing([16.0, 2.0])
                                    .show(ui, |ui| {
                                        // Header row
                                        for h in
                                            ["ADDRESS", "VALUE", "PREV", "", "FREEZE"]
                                        {
                                            ui.label(
                                                egui::RichText::new(h)
                                                    .size(theme::sz(10.0))
                                                    .strong()
                                                    .color(theme::text_faint()),
                                            );
                                        }
                                        ui.end_row();

                                        for row in &rows {
                                            let selected = app.mem_selected_hit
                                                == Some(row.idx);

                                            // --- Address ---
                                            let addr_resp = ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(format!(
                                                        "{:#018x}",
                                                        row.address
                                                    ))
                                                        .size(theme::sz(11.0))
                                                        .monospace()
                                                        .color(if selected {
                                                            theme::accent()
                                                        } else {
                                                            theme::text()
                                                        }),
                                                )
                                                    .sense(egui::Sense::click()),
                                            );
                                            if addr_resp.clicked() {
                                                click_row = Some(row.idx);
                                            }
                                            addr_resp.context_menu(|ui| {
                                                if ui
                                                    .button("Read 256 bytes here")
                                                    .clicked()
                                                {
                                                    click_row = Some(row.idx);
                                                    ui.close_menu();
                                                }
                                                if ui.button("Follow pointer").clicked() {
                                                    follow_pointer = Some(row.address);
                                                    ui.close_menu();
                                                }
                                                if ui.button("Disassemble here").clicked() {
                                                    disasm_at = Some(row.address);
                                                    ui.close_menu();
                                                }
                                            });

                                            // --- Value, coloured by change ---
                                            let value_color = match row.change {
                                                ChangeDir::Up => theme::ok(),
                                                ChangeDir::Down => theme::error(),
                                                _ => theme::text(),
                                            };
                                            ui.label(
                                                egui::RichText::new(&row.value)
                                                    .size(theme::sz(11.0))
                                                    .monospace()
                                                    .color(value_color),
                                            );

                                            // --- Previous ---
                                            let prev = row
                                                .previous
                                                .as_deref()
                                                .unwrap_or("");
                                            ui.label(
                                                egui::RichText::new(prev)
                                                    .size(theme::sz(11.0))
                                                    .monospace()
                                                    .color(theme::text_faint()),
                                            );

                                            // --- Change arrow ---
                                            let (arrow, arrow_color) =
                                                match row.change {
                                                    ChangeDir::Up => {
                                                        ("▲", theme::ok())
                                                    }
                                                    ChangeDir::Down => {
                                                        ("▼", theme::error())
                                                    }
                                                    ChangeDir::Same => {
                                                        ("━", theme::text_faint())
                                                    }
                                                    ChangeDir::Unknown => {
                                                        ("·", theme::text_faint())
                                                    }
                                                };
                                            ui.label(
                                                egui::RichText::new(arrow)
                                                    .size(theme::sz(11.0))
                                                    .color(arrow_color),
                                            );

                                            // --- Freeze checkbox ---
                                            let mut fz = row.frozen;
                                            let cb = ui.checkbox(&mut fz, "");
                                            let changed = cb.changed();
                                            cb.on_hover_text("Pin this value.");
                                            if changed {
                                                toggle_freeze = Some(row.idx);
                                            }
                                            ui.end_row();
                                        }
                                    });

                                ui.add_space(6.0);

                                let total = app.mem_scan_results.len();
                                let shown = rows.len();
                                let text = if has_filter {
                                    format!("showing {} of {} (filtered)", shown, total)
                                } else {
                                    format!("showing {} of {}", shown, total)
                                };
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(theme::sz(10.0))
                                        .italics()
                                        .color(theme::text_faint()),
                                );
                            });

                        if let Some(i) = click_row {
                            app.mem_selected_hit = Some(i);
                            let addr = app.mem_scan_results[i].address;
                            app.memory_read_hex(addr, 256);
                        }
                        if let Some(i) = toggle_freeze {
                            app.memory_toggle_freeze(i);
                        }
                        if let Some(addr) = follow_pointer {
                            let ptr = app
                                .with_reader(|r| r.read(addr, 8))
                                .and_then(|res| res.ok())
                                .and_then(|b| {
                                    if b.len() == 8 {
                                        Some(u64::from_le_bytes([
                                            b[0], b[1], b[2], b[3], b[4], b[5],
                                            b[6], b[7],
                                        ]))
                                    } else {
                                        None
                                    }
                                });
                            if let Some(target) = ptr {
                                app.memory_read_hex(target, 256);
                                app.mem_status = Some(format!(
                                    "pointer at {:#x} -> {:#x}",
                                    addr, target
                                ));
                            } else {
                                app.mem_status = Some(format!(
                                    "could not read pointer at {:#x}",
                                    addr
                                ));
                            }
                        }
                        if let Some(addr) = disasm_at {
                            app.memory_read_hex(addr, 64);
                            app.mem_status = Some(format!(
                                "loaded 64 bytes at {:#x} into the hex view",
                                addr
                            ));
                        }
                    });
            },
        );
    });

    // Selected row editor
    if let Some(i) = app.mem_selected_hit {
        if i < app.mem_scan_results.len() {
            ui.add_space(10.0);
            let addr = app.mem_scan_results[i].address;
            let current = app.mem_scan_results[i].value.clone();
            let kind = app.mem_scan_results[i].kind;

            egui::Frame::none()
                .fill(theme::panel())
                .rounding(egui::Rounding::same(theme::corner()))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("EDIT")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:#018x}", addr))
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::accent()),
                        );
                        ui.label(
                            egui::RichText::new(kind.label())
                                .size(theme::sz(10.0))
                                .color(theme::text_faint()),
                        );

                        ui.add_space(10.0);

                        let mut buf = current.clone();
                        let te = ui.add_sized(
                            egui::vec2(200.0, 28.0),
                            egui::TextEdit::singleline(&mut buf)
                                .margin(egui::Margin::symmetric(10.0, 6.0)),
                        );
                        if te.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            app.memory_write_value(addr, &buf);
                            if let Some(h) = app.mem_scan_results.get_mut(i) {
                                h.value = buf.clone();
                                h.change = ChangeDir::Unknown;
                                h.previous = None;
                            }
                            let ty = if app.mem_scan_type == ValueType::All {
                                kind
                            } else {
                                app.mem_scan_type
                            };
                            if let Ok(bytes) =
                                crate::core::memory::parse_value(ty, &buf)
                            {
                                app.memory_set_frozen_bytes(addr, bytes);
                            }
                        }
                        te.on_hover_text("New value. Press Enter to write.");
                    });
                });
        }
    }
}

// ============================================================
// TAB: REGIONS
// ============================================================

fn regions_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("COMMITTED REGIONS")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let reload = egui::Button::new(
                        egui::RichText::new("reload")
                            .size(theme::sz(11.0))
                            .color(theme::text()),
                    )
                        .fill(theme::panel_hover())
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(reload);
                    let c = r.clicked();
                    r.on_hover_text("Re-query the target's memory map.");
                    if c {
                        app.memory_load_regions();
                    }

                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} regions",
                            app.mem_regions.len()
                        ))
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    );
                });
            });

            ui.add_space(8.0);

            if !app.mem_regions_loaded {
                ui.label(
                    egui::RichText::new("click reload to fetch the region list")
                        .size(theme::sz(11.0))
                        .italics()
                        .color(theme::text_faint()),
                );
                return;
            }

            let mut click_row: Option<usize> = None;

            egui::ScrollArea::vertical()
                .id_source("mem_regions_scroll")
                .auto_shrink([false, false])
                .max_height(460.0)
                .show(ui, |ui| {
                    egui::Grid::new("mem_regions_grid")
                        .striped(true)
                        .num_columns(4)
                        .spacing([16.0, 2.0])
                        .show(ui, |ui| {
                            for h in ["BASE", "SIZE", "PROTECT", "TYPE"] {
                                ui.label(
                                    egui::RichText::new(h)
                                        .size(theme::sz(10.0))
                                        .strong()
                                        .color(theme::text_faint()),
                                );
                            }
                            ui.end_row();

                            for (i, r) in app.mem_regions.iter().enumerate() {
                                let selected = app.mem_selected_region == Some(i);
                                let color = if selected {
                                    theme::accent()
                                } else {
                                    theme::text()
                                };

                                let resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "{:#018x}",
                                            r.base
                                        ))
                                            .size(theme::sz(11.0))
                                            .monospace()
                                            .color(color),
                                    )
                                        .sense(egui::Sense::click()),
                                );
                                if resp.clicked() {
                                    click_row = Some(i);
                                }

                                ui.label(
                                    egui::RichText::new(format!("{:#x}", r.size))
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text_dim()),
                                );
                                ui.label(
                                    egui::RichText::new(r.protection_label())
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(if r.is_writable() {
                                            theme::ok()
                                        } else {
                                            theme::text_dim()
                                        }),
                                );
                                ui.label(
                                    egui::RichText::new(r.type_label())
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text_dim()),
                                );
                                ui.end_row();
                            }
                        });
                });

            if let Some(i) = click_row {
                app.mem_selected_region = Some(i);
                let base = app.mem_regions[i].base;
                app.memory_read_hex(base, 256);
            }
        });
}

// ============================================================
// TAB: FREEZE
// ============================================================

fn freeze_tab(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("FROZEN VALUES")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let active = app
                        .mem_freeze_active
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let (label, color) = if active {
                        ("running", theme::ok())
                    } else {
                        ("idle", theme::text_faint())
                    };
                    ui.label(
                        egui::RichText::new(label)
                            .size(theme::sz(11.0))
                            .strong()
                            .color(color),
                    );

                    ui.add_space(10.0);

                    let clear = egui::Button::new(
                        egui::RichText::new("unfreeze all")
                            .size(theme::sz(11.0))
                            .color(theme::blocked()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(clear);
                    let c = r.clicked();
                    r.on_hover_text("Remove every frozen entry.");
                    if c {
                        app.stop_memory_freeze();
                    }
                });
            });

            ui.add_space(10.0);

            let list = match app.mem_frozen.lock() {
                Ok(l) => l.clone(),
                Err(_) => Vec::new(),
            };

            if list.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "nothing frozen — tick the FREEZE checkbox on a scan result",
                    )
                        .size(theme::sz(11.0))
                        .italics()
                        .color(theme::text_faint()),
                );
                return;
            }

            let mut unfreeze: Option<u64> = None;

            egui::Grid::new("mem_freeze_grid")
                .striped(true)
                .num_columns(4)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for h in ["ADDRESS", "TYPE", "BYTES", ""] {
                        ui.label(
                            egui::RichText::new(h)
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_faint()),
                        );
                    }
                    ui.end_row();

                    for entry in &list {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:#018x}",
                                entry.address
                            ))
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::accent()),
                        );
                        ui.label(
                            egui::RichText::new(entry.value_type.label())
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::text_dim()),
                        );
                        ui.label(
                            egui::RichText::new(
                                entry
                                    .bytes
                                    .iter()
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            )
                                .size(theme::sz(11.0))
                                .monospace()
                                .color(theme::text()),
                        );
                        let btn = egui::Button::new(
                            egui::RichText::new("unfreeze")
                                .size(theme::sz(10.0))
                                .color(theme::blocked()),
                        )
                            .fill(egui::Color32::TRANSPARENT)
                            .rounding(egui::Rounding::same(4.0));
                        let r = ui.add(btn);
                        let c = r.clicked();
                        r.on_hover_text("Stop freezing this address.");
                        if c {
                            unfreeze = Some(entry.address);
                        }
                        ui.end_row();
                    }
                });

            if let Some(addr) = unfreeze {
                if let Ok(mut l) = app.mem_frozen.lock() {
                    l.retain(|e| e.address != addr);
                }
                for h in &mut app.mem_scan_results {
                    if h.address == addr {
                        h.frozen = false;
                    }
                }
                if app.mem_frozen.lock().map(|l| l.is_empty()).unwrap_or(true) {
                    app.stop_memory_freeze();
                }
            }
        });
}

// ============================================================
// TAB: SCRIPTS
// ============================================================

fn scripts_tab(app: &mut App, ui: &mut egui::Ui) {
    let running = app.script_runtime.is_some();

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("SIGILSCRIPT")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                );

                ui.add_space(10.0);

                let (status_text, status_color) = match &app.script_status {
                    crate::state::ScriptStatus::Idle => ("idle", theme::text_faint()),
                    crate::state::ScriptStatus::Running => ("running", theme::ok()),
                    crate::state::ScriptStatus::Stopped => ("stopped", theme::warn()),
                    crate::state::ScriptStatus::Error(_) => ("error", theme::error()),
                };
                ui.label(
                    egui::RichText::new(status_text)
                        .size(theme::sz(11.0))
                        .strong()
                        .color(status_color),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let clear_out = egui::Button::new(
                        egui::RichText::new("clear output")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(clear_out);
                    let c = r.clicked();
                    if c {
                        if let Ok(mut o) = app.script_output.lock() {
                            o.clear();
                        }
                    }

                    ui.add_space(10.0);

                    let stop = egui::Button::new(
                        egui::RichText::new("Stop")
                            .size(theme::sz(12.0))
                            .color(if running {
                                theme::blocked()
                            } else {
                                theme::text_faint()
                            }),
                    )
                        .fill(theme::bg())
                        .rounding(egui::Rounding::same(8.0))
                        .min_size(egui::vec2(80.0, 30.0));
                    let r = ui.add_enabled(running, stop);
                    let c = r.clicked();
                    if c {
                        app.stop_script();
                    }

                    ui.add_space(6.0);

                    let run = egui::Button::new(
                        egui::RichText::new("Run")
                            .size(theme::sz(12.0))
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                        .fill(theme::accent())
                        .rounding(egui::Rounding::same(8.0))
                        .min_size(egui::vec2(80.0, 30.0));
                    let r = ui.add_enabled(!running, run);
                    let c = r.clicked();
                    if c {
                        app.start_script();
                    }
                });
            });
        });

    ui.add_space(10.0);

    let body_height = (ui.available_height() * 0.6).max(280.0);

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(200.0, body_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::none()
                    .fill(theme::panel())
                    .rounding(egui::Rounding::same(theme::corner()))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("LIBRARY")
                                .size(theme::sz(10.0))
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut app.script_new_name)
                                    .hint_text("new script")
                                    .desired_width(130.0)
                                    .margin(egui::Margin::symmetric(6.0, 4.0)),
                            );

                            let add = egui::Button::new(
                                egui::RichText::new("+")
                                    .size(theme::sz(14.0))
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                                .fill(theme::accent())
                                .rounding(egui::Rounding::same(6.0))
                                .min_size(egui::vec2(28.0, 26.0));
                            let r = ui.add(add);
                            let c = r.clicked();
                            if c && !app.script_new_name.trim().is_empty() {
                                let n = app.script_new_name.clone();
                                app.create_script(&n);
                                app.script_new_name.clear();
                            }
                        });

                        ui.add_space(8.0);

                        let mut to_select: Option<usize> = None;
                        let mut to_delete: Option<usize> = None;

                        egui::ScrollArea::vertical()
                            .id_source("script_lib_scroll")
                            .auto_shrink([false, false])
                            .max_height(body_height - 80.0)
                            .show(ui, |ui| {
                                if app.scripts.is_empty() {
                                    ui.label(
                                        egui::RichText::new("empty")
                                            .size(theme::sz(11.0))
                                            .italics()
                                            .color(theme::text_faint()),
                                    );
                                    return;
                                }
                                for i in 0..app.scripts.len() {
                                    let (name, desc) = {
                                        let s = &app.scripts[i];
                                        (s.name.clone(), s.description.clone())
                                    };
                                    let selected = app.active_script_idx == Some(i);
                                    let fill = if selected {
                                        theme::accent_dim()
                                    } else {
                                        theme::bg()
                                    };
                                    let color = if selected {
                                        egui::Color32::WHITE
                                    } else {
                                        theme::text()
                                    };

                                    egui::Frame::none()
                                        .fill(fill)
                                        .rounding(egui::Rounding::same(6.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let lbl = ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(&name)
                                                            .size(theme::sz(12.0))
                                                            .color(color),
                                                    )
                                                        .sense(egui::Sense::click()),
                                                );
                                                if lbl.clicked() {
                                                    to_select = Some(i);
                                                }
                                                lbl.on_hover_text(if desc.is_empty() {
                                                    "click to edit".to_string()
                                                } else {
                                                    desc
                                                });

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let del = egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(theme::sz(12.0))
                                                                .color(theme::blocked()),
                                                        )
                                                            .fill(egui::Color32::TRANSPARENT)
                                                            .rounding(egui::Rounding::same(4.0))
                                                            .min_size(egui::vec2(18.0, 18.0));
                                                        let r = ui.add(del);
                                                        let c = r.clicked();
                                                        if c {
                                                            to_delete = Some(i);
                                                        }
                                                    },
                                                );
                                            });
                                        });
                                    ui.add_space(4.0);
                                }
                            });

                        if let Some(i) = to_select {
                            app.active_script_idx = Some(i);
                        }
                        if let Some(i) = to_delete {
                            app.delete_script(i);
                        }
                    });
            },
        );

        ui.add_space(10.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), body_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::none()
                    .fill(theme::panel())
                    .rounding(egui::Rounding::same(theme::corner()))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        let Some(active) = app.active_script().cloned() else {
                            ui.vertical_centered(|ui| {
                                ui.add_space(60.0);
                                ui.label(
                                    egui::RichText::new("no script selected")
                                        .size(theme::sz(13.0))
                                        .italics()
                                        .color(theme::text_faint()),
                                );
                            });
                            return;
                        };

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&active.name)
                                    .size(theme::sz(14.0))
                                    .strong()
                                    .color(theme::text()),
                            );
                            ui.add_space(8.0);

                            let mut desc = active.description.clone();
                            let d = ui.add(
                                egui::TextEdit::singleline(&mut desc)
                                    .hint_text("description")
                                    .desired_width(240.0)
                                    .margin(egui::Margin::symmetric(6.0, 4.0)),
                            );
                            if d.changed() {
                                if let Some(s) = app.active_script_mut() {
                                    s.description = desc;
                                }
                                app.save_scripts();
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let save = egui::Button::new(
                                        egui::RichText::new("Save")
                                            .size(theme::sz(11.0))
                                            .color(theme::text()),
                                    )
                                        .fill(theme::panel_hover())
                                        .rounding(egui::Rounding::same(6.0));
                                    let r = ui.add(save);
                                    let c = r.clicked();
                                    if c {
                                        app.save_scripts();
                                    }
                                },
                            );
                        });

                        ui.add_space(6.0);

                        let mut source = active.source.clone();
                        egui::Frame::none()
                            .fill(theme::bg())
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                egui::ScrollArea::both()
                                    .id_source("script_editor_scroll")
                                    .auto_shrink([false, false])
                                    .max_height(body_height - 90.0)
                                    .show(ui, |ui| {
                                        let te = ui.add_sized(
                                            egui::vec2(
                                                ui.available_width(),
                                                body_height - 110.0,
                                            ),
                                            egui::TextEdit::multiline(&mut source)
                                                .font(egui::TextStyle::Monospace)
                                                .code_editor()
                                                .desired_width(f32::INFINITY),
                                        );
                                        if te.changed() {
                                            if let Some(s) = app.active_script_mut() {
                                                s.source = source;
                                                s.modified =
                                                    crate::core::session::now_string();
                                            }
                                            app.save_scripts();
                                        }
                                    });
                            });
                    });
            },
        );
    });

    ui.add_space(10.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("OUTPUT")
                    .size(theme::sz(10.0))
                    .strong()
                    .color(theme::text_dim()),
            );
            ui.add_space(4.0);

            let lines: Vec<String> = app
                .script_output
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();

            egui::ScrollArea::vertical()
                .id_source("script_output_scroll")
                .auto_shrink([false, false])
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if lines.is_empty() {
                        ui.label(
                            egui::RichText::new("no output yet")
                                .size(theme::sz(11.0))
                                .italics()
                                .color(theme::text_faint()),
                        );
                    } else {
                        for line in &lines {
                            let color = if line.starts_with("[error]") {
                                theme::error()
                            } else if line.starts_with("[warn]") {
                                theme::warn()
                            } else if line.starts_with("[start]")
                                || line.starts_with("[done]")
                            {
                                theme::accent()
                            } else if line.starts_with("[stop]") {
                                theme::blocked()
                            } else {
                                theme::text()
                            };
                            ui.label(
                                egui::RichText::new(line)
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(color),
                            );
                        }
                    }
                });
        });
}

// ============================================================
// PROCESS PICKER MODAL
// ============================================================

fn process_picker_modal(app: &mut App, ctx: &egui::Context) {
    let mut close = false;

    egui::Window::new("Select process")
        .id(egui::Id::new("mem_proc_picker"))
        .collapsible(false)
        .resizable(true)
        .default_size([560.0, 480.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PROCESSES")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.mem_proc_search)
                        .hint_text("search by name or PID")
                        .desired_width(260.0)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refresh = egui::Button::new(
                        egui::RichText::new("refresh")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(refresh);
                    let c = r.clicked();
                    if c {
                        app.memory_refresh_proc_list();
                    }
                });
            });

            ui.add_space(6.0);

            // Filter the list
            let needle = app.mem_proc_search.trim().to_lowercase();
            let filtered: Vec<ProcessListing> = if needle.is_empty() {
                app.mem_proc_list.clone()
            } else {
                app.mem_proc_list
                    .iter()
                    .filter(|p| {
                        p.name.to_lowercase().contains(&needle)
                            || p.pid.to_string().contains(&needle)
                    })
                    .cloned()
                    .collect()
            };

            let mut pick: Option<u32> = None;

            egui::ScrollArea::vertical()
                .id_source("mem_proc_list_scroll")
                .auto_shrink([false, false])
                .max_height(360.0)
                .show(ui, |ui| {
                    for p in &filtered {
                        let selected = app.mem_proc_selected == Some(p.pid);
                        let fill = if selected {
                            theme::accent_dim()
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let text_color = if selected {
                            egui::Color32::WHITE
                        } else if p.accessible {
                            theme::text()
                        } else {
                            theme::text_faint()
                        };

                        let dot = category_color(p.category);

                        egui::Frame::none()
                            .fill(fill)
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("●")
                                            .size(theme::sz(12.0))
                                            .color(dot),
                                    )
                                        .on_hover_text(p.category);

                                    ui.add_space(6.0);

                                    ui.label(
                                        egui::RichText::new(&p.name)
                                            .size(theme::sz(12.0))
                                            .color(text_color),
                                    );

                                    ui.with_layout(
                                        egui::Layout::right_to_left(
                                            egui::Align::Center,
                                        ),
                                        |ui| {
                                            if !p.accessible {
                                                ui.label(
                                                    egui::RichText::new("restricted")
                                                        .size(theme::sz(10.0))
                                                        .color(theme::blocked()),
                                                );
                                            }
                                            ui.add_space(8.0);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "PID {}",
                                                    p.pid
                                                ))
                                                    .size(theme::sz(11.0))
                                                    .monospace()
                                                    .color(theme::text_dim()),
                                            );
                                        },
                                    );
                                });
                            });

                        let resp = ui.interact(
                            ui.min_rect(),
                            ui.id().with(p.pid),
                            egui::Sense::click(),
                        );
                        if resp.clicked() {
                            pick = Some(p.pid);
                        }

                        ui.add_space(2.0);
                    }
                });

            if let Some(pid) = pick {
                app.mem_proc_selected = Some(pid);
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let attach = egui::Button::new(
                    egui::RichText::new("Attach")
                        .size(theme::sz(12.0))
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                    .fill(theme::accent())
                    .rounding(egui::Rounding::same(8.0))
                    .min_size(egui::vec2(100.0, 32.0));

                let can = app.mem_proc_selected.is_some();
                let r = ui.add_enabled(can, attach);
                let c = r.clicked();
                if c {
                    app.memory_attach_selected();
                    close = true;
                }

                ui.add_space(6.0);

                let cancel = egui::Button::new(
                    egui::RichText::new("Cancel")
                        .size(theme::sz(12.0))
                        .color(theme::text()),
                )
                    .fill(theme::panel_hover())
                    .rounding(egui::Rounding::same(8.0))
                    .min_size(egui::vec2(90.0, 32.0));
                let r = ui.add(cancel);
                let c = r.clicked();
                if c {
                    close = true;
                }

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} processes",
                                app.mem_proc_list.len()
                            ))
                                .size(theme::sz(10.0))
                                .color(theme::text_faint()),
                        );
                    },
                );
            });
        });

    if close {
        app.memory_close_proc_picker();
    }
}

fn category_color(cat: &str) -> egui::Color32 {
    match cat {
        "system" => egui::Color32::from_rgb(180, 180, 180),
        "game" => egui::Color32::from_rgb(120, 220, 140),
        "browser" => egui::Color32::from_rgb(120, 170, 240),
        "app" => egui::Color32::from_rgb(220, 180, 100),
        _ => egui::Color32::GRAY,
    }
}

// ============================================================
// HEX PANEL
// ============================================================

fn hex_panel(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("HEX VIEWER")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let write = egui::Button::new(
                        egui::RichText::new("Write back")
                            .size(theme::sz(11.0))
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                        .fill(theme::accent())
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(write);
                    let c = r.clicked();
                    r.on_hover_text("Write the displayed bytes back.");
                    if c {
                        app.memory_write_hex();
                    }

                    ui.add_space(6.0);

                    let read = egui::Button::new(
                        egui::RichText::new("Read")
                            .size(theme::sz(11.0))
                            .color(theme::text()),
                    )
                        .fill(theme::panel_hover())
                        .rounding(egui::Rounding::same(6.0));
                    let r = ui.add(read);
                    let c = r.clicked();
                    r.on_hover_text("Read 256 bytes from the address.");
                    if c {
                        let s = app
                            .mem_hex_address
                            .trim()
                            .trim_start_matches("0x")
                            .to_string();
                        if let Ok(addr) = u64::from_str_radix(&s, 16) {
                            app.memory_read_hex(addr, 256);
                        } else {
                            app.mem_hex_error = Some("address is not hex".into());
                        }
                    }

                    ui.add_space(6.0);

                    ui.add_sized(
                        egui::vec2(220.0, 30.0),
                        egui::TextEdit::singleline(&mut app.mem_hex_address)
                            .hint_text("0x00007ff6…")
                            .margin(egui::Margin::symmetric(10.0, 6.0)),
                    )
                        .on_hover_text("Address to inspect.");

                    ui.add_space(6.0);

                    ui.label(
                        egui::RichText::new("Address")
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    );
                });
            });

            if let Some(err) = &app.mem_hex_error {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(err)
                        .size(theme::sz(11.0))
                        .color(theme::error()),
                );
            }

            ui.add_space(8.0);

            if app.mem_hex_bytes.is_empty() {
                ui.label(
                    egui::RichText::new("no bytes loaded — click a result or a region")
                        .size(theme::sz(11.0))
                        .italics()
                        .color(theme::text_faint()),
                );
                return;
            }

            let mut bytes = app.mem_hex_bytes.clone();
            let mut changed = false;

            egui::ScrollArea::vertical()
                .id_source("mem_hex_scroll")
                .auto_shrink([false, false])
                .max_height(220.0)
                .show(ui, |ui| {
                    let base = u64::from_str_radix(
                        app.mem_hex_address.trim().trim_start_matches("0x"),
                        16,
                    )
                        .unwrap_or(0);

                    for (chunk_i, chunk) in bytes.chunks_mut(16).enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:#010x}",
                                    base + (chunk_i * 16) as u64
                                ))
                                    .size(theme::sz(10.0))
                                    .monospace()
                                    .color(theme::text_faint()),
                            );

                            for b in chunk.iter_mut() {
                                let mut s = format!("{:02x}", *b);
                                let te = ui.add(
                                    egui::TextEdit::singleline(&mut s)
                                        .desired_width(26.0)
                                        .font(egui::TextStyle::Monospace)
                                        .margin(egui::Margin::symmetric(2.0, 2.0)),
                                );
                                if te.changed() {
                                    if let Ok(v) = u8::from_str_radix(
                                        s.trim().trim_start_matches("0x"),
                                        16,
                                    ) {
                                        if *b != v {
                                            *b = v;
                                            changed = true;
                                        }
                                    }
                                }
                            }

                            ui.add_space(6.0);

                            let ascii: String = chunk
                                .iter()
                                .map(|b| {
                                    if b.is_ascii_graphic() || *b == b' ' {
                                        *b as char
                                    } else {
                                        '.'
                                    }
                                })
                                .collect();
                            ui.label(
                                egui::RichText::new(ascii)
                                    .size(theme::sz(10.0))
                                    .monospace()
                                    .color(theme::text_dim()),
                            );
                        });
                    }
                });

            if changed {
                app.mem_hex_bytes = bytes;
            }
        });
}