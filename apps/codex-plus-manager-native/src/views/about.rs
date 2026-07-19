use eframe::egui;

use crate::external_url::ExternalUrl;
use crate::i18n::Locale;
use crate::i18n::{TextKey, text};
use crate::icons;

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
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        external_button(
            ui,
            icons::folder_git_2(),
            about_text(model.locale, AboutText::Repository),
            "https://github.com/BigPizzaV3/CodexPlusPlus",
        );
        external_button(
            ui,
            icons::triangle_alert(),
            about_text(model.locale, AboutText::Issues),
            "https://github.com/BigPizzaV3/CodexPlusPlus/issues",
        );
        external_button(
            ui,
            icons::message_circle(),
            "Discord",
            "https://discord.gg/y96kX7A76v",
        );
        external_button(
            ui,
            icons::message_circle(),
            "Telegram",
            "https://t.me/CodexPlusPlus",
        );
    });
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

fn external_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, label: &str, value: &str) {
    let url = ExternalUrl::parse(value).expect("built-in external URL is valid");
    if ui
        .add_sized(
            [156.0, 34.0],
            egui::Button::image_and_text(
                egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                label,
            ),
        )
        .clicked()
    {
        url.emit(ui.ctx());
    }
}

#[derive(Clone, Copy)]
enum AboutText {
    Repository,
    Issues,
}

fn about_text(locale: Locale, key: AboutText) -> &'static str {
    match (locale, key) {
        (Locale::ZhCn, AboutText::Repository) => "项目仓库",
        (Locale::En, AboutText::Repository) => "Project repository",
        (Locale::ZhCn, AboutText::Issues) => "提交问题",
        (Locale::En, AboutText::Issues) => "Report an issue",
    }
}
