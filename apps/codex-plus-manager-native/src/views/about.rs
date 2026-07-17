use eframe::egui;

use crate::i18n::{TextKey, text};

use super::shell::ShellViewModel;

pub fn render(ui: &mut egui::Ui, model: &ShellViewModel) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AppName))
            .strong()
            .size(26.0),
    );
    ui.label(
        egui::RichText::new(text(model.locale, TextKey::AboutSubtitle))
            .weak()
            .size(13.0),
    );
    ui.add_space(24.0);
    ui.separator();
    about_row(
        ui,
        text(model.locale, TextKey::Version),
        env!("CARGO_PKG_VERSION"),
    );
    about_row(ui, text(model.locale, TextKey::Renderer), &model.renderer);
    about_row(ui, "OS", std::env::consts::OS);
    about_row(ui, text(model.locale, TextKey::License), "AGPL-3.0-only");
}

fn about_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(42.0);
        ui.label(egui::RichText::new(label).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).strong());
        });
    });
    ui.separator();
}
