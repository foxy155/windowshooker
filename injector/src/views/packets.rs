use crate::core::filter::eval_expr;
use crate::core::packets::Packet;
use crate::state::{App, InterceptState, SortColumn, SortOrder};
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    app.ensure_filter_parsed();

    let current_count = app.store.lock().map(|s| s.packets.len()).unwrap_or(0);
    if app.intercept_state == InterceptState::Listening
        && current_count > app.intercept_packet_count_at_start
    {
        app.intercept_state = InterceptState::Receiving;
    }

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let active = app.intercept_state != InterceptState::Idle;

                let (label, fill) = if active {
                    ("Stop Intercepting", theme::error())
                } else {
                    ("Start Intercepting", theme::ok())
                };

                let btn = egui::Button::new(
                    egui::RichText::new(label)
                        .size(theme::sz(13.0))
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                    .fill(fill)
                    .rounding(egui::Rounding::same(8.0))
                    .min_size(egui::vec2(180.0, 34.0));

                if ui
                    .add(btn)
                    .on_hover_text(if active {
                        "Stop capturing packets from the injected DLL."
                    } else {
                        "Begin capturing packets from the injected DLL. Starts the reader thread if needed."
                    })
                    .clicked()
                {
                    if active {
                        app.stop_intercepting();
                    } else {
                        app.start_intercepting();
                    }
                }

                ui.add_space(12.0);

                let (status_text, status_color) = match app.intercept_state {
                    InterceptState::Idle => ("idle", theme::text_dim()),
                    InterceptState::Listening => ("listening...", theme::warn()),
                    InterceptState::Receiving => ("receiving", theme::ok()),
                };
                ui.label(
                    egui::RichText::new(status_text)
                        .size(theme::sz(12.0))
                        .strong()
                        .color(status_color),
                )
                    .on_hover_text(
                        "idle = not capturing · listening = waiting for the first packet · receiving = packets are arriving",
                    );

                if let Some(started) = app.intercept_started_at {
                    let elapsed = started.elapsed().as_secs_f32();
                    let captured =
                        current_count.saturating_sub(app.intercept_packet_count_at_start);
                    let rate = if elapsed > 0.5 {
                        captured as f32 / elapsed
                    } else {
                        0.0
                    };
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} packets in {:.1}s ({:.1}/s)",
                            captured, elapsed, rate
                        ))
                            .size(theme::sz(11.0))
                            .color(theme::text_dim()),
                    )
                        .on_hover_text(
                            "Packets captured since the current interception session started, and the average rate.",
                        );
                }
            });
        });

    ui.add_space(10.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Filter")
                        .size(theme::sz(12.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text(
                        "DSL filter applied to every captured packet. See the field reference below the bar.",
                    );
                ui.add(
                    egui::TextEdit::singleline(&mut app.filter_text)
                        .hint_text("e.g. pid == 15616 && len > 20")
                        .desired_width(360.0)
                        .margin(egui::Margin::symmetric(8.0, 6.0)),
                )
                    .on_hover_text(
                        "Fields: dir, len, op, hex, text, id, pid. Ops: == != < <= > >= contains. Logic: && || ! ( ).",
                    );

                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Clear")
                                .size(theme::sz(12.0))
                                .color(theme::text()),
                        )
                            .fill(theme::panel_hover())
                            .rounding(egui::Rounding::same(8.0)),
                    )
                    .on_hover_text("Discard every captured packet. Cannot be undone.")
                    .clicked()
                {
                    if let Ok(mut store) = app.store.lock() {
                        store.clear();
                    }
                    app.selected_packet = None;
                }

                ui.label(
                    egui::RichText::new(format!("{} captured", current_count))
                        .size(theme::sz(12.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text(
                        "Total packets currently held in the in-memory store. Oldest are dropped past 10,000.",
                    );
            });

            if let Some(err) = &app.filter_error {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("filter error: {}", err))
                        .size(theme::sz(11.0))
                        .color(theme::error()),
                )
                    .on_hover_text("The filter expression could not be parsed. Fix the syntax above.");
            }

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Presets")
                        .size(theme::sz(11.0))
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Click a preset to replace the filter expression above.");
                let mut preset_to_apply: Option<String> = None;
                for (name, expr) in &app.presets {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(name)
                                    .size(theme::sz(11.0))
                                    .color(theme::text()),
                            )
                                .fill(theme::panel_hover())
                                .rounding(egui::Rounding::same(6.0)),
                        )
                        .on_hover_text(if expr.is_empty() {
                            "Clear the current filter and show every packet.".to_string()
                        } else {
                            format!("Apply filter: {}", expr)
                        })
                        .clicked()
                    {
                        preset_to_apply = Some(expr.clone());
                    }
                }
                if let Some(expr) = preset_to_apply {
                    app.filter_text = expr;
                }
            });
        });

    ui.add_space(10.0);

    ui.label(
        egui::RichText::new(
            "fields: dir, len, op, hex, text, id, pid  |  ops: == != < <= > >= contains  |  logic: && || ! ( )",
        )
            .size(theme::sz(10.0))
            .color(theme::text_faint()),
    )
        .on_hover_text("Quick reference for the filter DSL syntax.");

    ui.add_space(8.0);

    let available = ui.available_height();
    let table_height = (available * 0.55).max(160.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_source("packet_table")
                .auto_shrink([false, false])
                .max_height(table_height)
                .show(ui, |ui| {
                    let filter_expr = app.cached_filter.clone();
                    let sort_col = app.sort_column;
                    let sort_ord = app.sort_order;

                    let mut matched: Vec<Packet> = match app.store.lock() {
                        Ok(store) => store
                            .packets
                            .iter()
                            .filter(|p| match &filter_expr {
                                Some(e) => eval_expr(e, p),
                                None => true,
                            })
                            .cloned()
                            .collect(),
                        Err(_) => Vec::new(),
                    };

                    matched.sort_by(|a, b| {
                        let ord = match sort_col {
                            SortColumn::Id => a.id.cmp(&b.id),
                            SortColumn::Pid => a.source_pid.cmp(&b.source_pid),
                            SortColumn::Direction => {
                                (a.direction as u8).cmp(&(b.direction as u8))
                            }
                            SortColumn::Time => a.timestamp.cmp(&b.timestamp),
                            SortColumn::Opcode => {
                                a.opcode.unwrap_or(0).cmp(&b.opcode.unwrap_or(0))
                            }
                            SortColumn::Size => a.size.cmp(&b.size),
                        };
                        match sort_ord {
                            SortOrder::Asc => ord,
                            SortOrder::Desc => ord.reverse(),
                        }
                    });

                    let matched_count = matched.len();
                    let total_count = current_count;

                    egui::Grid::new("packet_grid")
                        .striped(true)
                        .num_columns(6)
                        .spacing([12.0, 3.0])
                        .show(ui, |ui| {
                            if sort_header(ui, "#", SortColumn::Id, sort_col, sort_ord) {
                                app.toggle_sort(SortColumn::Id);
                            }
                            if sort_header(ui, "PID", SortColumn::Pid, sort_col, sort_ord) {
                                app.toggle_sort(SortColumn::Pid);
                            }
                            if sort_header(
                                ui,
                                "DIR",
                                SortColumn::Direction,
                                sort_col,
                                sort_ord,
                            ) {
                                app.toggle_sort(SortColumn::Direction);
                            }
                            if sort_header(ui, "TIME", SortColumn::Time, sort_col, sort_ord) {
                                app.toggle_sort(SortColumn::Time);
                            }
                            if sort_header(ui, "OP", SortColumn::Opcode, sort_col, sort_ord) {
                                app.toggle_sort(SortColumn::Opcode);
                            }
                            if sort_header(ui, "LEN", SortColumn::Size, sort_col, sort_ord) {
                                app.toggle_sort(SortColumn::Size);
                            }
                            ui.end_row();

                            for p in &matched {
                                let selected = app.selected_packet == Some(p.id);
                                let row_color = if selected {
                                    theme::accent()
                                } else {
                                    p.direction_color()
                                };

                                let id_resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("{}", p.id))
                                            .size(theme::sz(11.0))
                                            .monospace()
                                            .color(row_color),
                                    )
                                        .sense(egui::Sense::click()),
                                );
                                if id_resp.clicked() {
                                    app.selected_packet = Some(p.id);
                                }
                                id_resp.on_hover_text(
                                    "Click to inspect this packet's bytes in the hex view below.",
                                );

                                ui.label(
                                    egui::RichText::new(format!("{}", p.source_pid))
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text_dim()),
                                )
                                    .on_hover_text("Process ID that produced this packet.");

                                ui.label(
                                    egui::RichText::new(p.direction_label())
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(p.direction_color()),
                                )
                                    .on_hover_text(
                                        "SEND = outbound from the target · RECV = inbound to the target.",
                                    );

                                ui.label(
                                    egui::RichText::new(&p.timestamp)
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text_dim()),
                                )
                                    .on_hover_text("Wall-clock time the packet was captured.");

                                let op_str = p
                                    .opcode
                                    .map(|o| format!("0x{:04x}", o))
                                    .unwrap_or_else(|| "----".to_string());
                                ui.label(
                                    egui::RichText::new(op_str)
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text()),
                                )
                                    .on_hover_text(
                                        "First two bytes of the payload, read little-endian. Often a protocol opcode.",
                                    );

                                ui.label(
                                    egui::RichText::new(format!("{}", p.size))
                                        .size(theme::sz(11.0))
                                        .monospace()
                                        .color(theme::text()),
                                )
                                    .on_hover_text("Payload length in bytes.");
                                ui.end_row();
                            }
                        });

                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} of {} packets match",
                            matched_count, total_count
                        ))
                            .size(theme::sz(10.0))
                            .color(theme::text_faint()),
                    )
                        .on_hover_text(
                            "Packets shown after the current filter, out of the total in the store.",
                        );
                });
        });

    ui.add_space(10.0);

    egui::Frame::none()
        .fill(theme::panel())
        .rounding(egui::Rounding::same(theme::corner()))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("HEX VIEW")
                    .size(theme::sz(10.0))
                    .color(theme::text_dim()),
            )
                .on_hover_text("Raw bytes of the currently selected packet.");
            ui.add_space(4.0);

            let selected = app.selected_packet.and_then(|id| {
                app.store
                    .lock()
                    .ok()
                    .and_then(|s| s.packets.iter().find(|p| p.id == id).cloned())
            });

            egui::ScrollArea::vertical()
                .id_source("hex_view")
                .auto_shrink([false, false])
                .max_height((available * 0.35).max(100.0))
                .show(ui, |ui| {
                    if let Some(p) = selected {
                        ui.label(
                            egui::RichText::new(format!(
                                "Packet #{} | PID {} | {} | {} bytes",
                                p.id,
                                p.source_pid,
                                p.direction_label(),
                                p.size
                            ))
                                .size(theme::sz(11.0))
                                .color(theme::text_dim()),
                        )
                            .on_hover_text("Metadata for the packet shown below.");
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(p.hex_dump())
                                    .size(theme::sz(11.0))
                                    .monospace()
                                    .color(theme::text()),
                            )
                                .sense(egui::Sense::click()),
                        )
                            .on_hover_text("Hex dump. Click to select the text for copying.");
                    } else {
                        ui.label(
                            egui::RichText::new("select a packet to view its bytes")
                                .size(theme::sz(12.0))
                                .italics()
                                .color(theme::text_faint()),
                        )
                            .on_hover_text(
                                "Click a row in the table above to populate this view.",
                            );
                    }
                });
        });
}

fn sort_header(
    ui: &mut egui::Ui,
    label: &str,
    col: SortColumn,
    current_col: SortColumn,
    current_ord: SortOrder,
) -> bool {
    let arrow = if current_col == col {
        match current_ord {
            SortOrder::Asc => " ▲",
            SortOrder::Desc => " ▼",
        }
    } else {
        ""
    };
    let text = egui::RichText::new(format!("{}{}", label, arrow))
        .size(theme::sz(11.0))
        .strong()
        .color(theme::text_dim());
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        .on_hover_text(format!(
            "Sort by {} (click again to reverse).",
            match col {
                SortColumn::Id => "packet id",
                SortColumn::Pid => "source PID",
                SortColumn::Direction => "direction",
                SortColumn::Time => "timestamp",
                SortColumn::Opcode => "opcode",
                SortColumn::Size => "size",
            }
        ))
        .clicked()
}