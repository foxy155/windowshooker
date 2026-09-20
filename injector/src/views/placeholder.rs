use crate::state::App;
use crate::theme;

pub fn draw(app: &mut App, ui: &mut egui::Ui, title: &str, blurb: &str, roadmap: &[&str]) {
    let _ = app;

    ui.vertical_centered(|ui| {
        ui.add_space(60.0);

        ui.label(
            egui::RichText::new(title)
                .size(theme::sz(28.0))
                .strong()
                .color(theme::text()),
        );

        ui.add_space(6.0);

        ui.label(
            egui::RichText::new("planned")
                .size(theme::sz(10.0))
                .strong()
                .color(theme::accent()),
        )
            .on_hover_text("This view is not implemented yet. The roadmap below lists what it will do.");

        ui.add_space(16.0);

        ui.label(
            egui::RichText::new(blurb)
                .size(theme::sz(13.0))
                .color(theme::text_dim()),
        );

        ui.add_space(28.0);

        egui::Frame::none()
            .fill(theme::panel())
            .rounding(egui::Rounding::same(theme::corner()))
            .inner_margin(egui::Margin::symmetric(24.0, 18.0))
            .show(ui, |ui| {
                ui.set_max_width(420.0);

                ui.label(
                    egui::RichText::new("ROADMAP")
                        .size(theme::sz(10.0))
                        .strong()
                        .color(theme::text_dim()),
                )
                    .on_hover_text("Features planned for this view.");

                ui.add_space(8.0);

                for item in roadmap {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("•")
                                .size(theme::sz(12.0))
                                .color(theme::accent()),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(*item)
                                .size(theme::sz(12.0))
                                .color(theme::text()),
                        )
                            .on_hover_text("Planned feature. Not yet available.");
                    });
                }
            });
    });
}