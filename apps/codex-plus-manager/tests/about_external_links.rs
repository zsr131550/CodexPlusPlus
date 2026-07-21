use codex_plus_manager::i18n::{Locale, ThemeMode};
use codex_plus_manager::theme;
use codex_plus_manager::views::about;
use codex_plus_manager::views::shell::ShellViewModel;
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

#[test]
fn about_exposes_bilingual_validated_external_links() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            ["项目仓库", "提交问题", "Discord", "Telegram"],
        ),
        (
            Locale::En,
            [
                "Project repository",
                "Report an issue",
                "Discord",
                "Telegram",
            ],
        ),
    ] {
        let harness = harness(locale);
        for label in labels {
            assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn about_link_click_emits_only_the_validated_egui_command() {
    let mut harness = harness(Locale::En);

    harness.get_by_label("Project repository").click();
    harness.step();

    assert_eq!(
        harness.output().platform_output.commands,
        [egui::OutputCommand::OpenUrl(egui::OpenUrl::new_tab(
            "https://github.com/BigPizzaV3/CodexPlusPlus"
        ))]
    );
}

fn harness(locale: Locale) -> Harness<'static, ShellViewModel> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 720.0))
        .build_ui_state(render, common::model(locale, ThemeMode::Dark))
}

fn render(ui: &mut egui::Ui, model: &mut ShellViewModel) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), model.theme);
    let mut actions = Vec::new();
    about::render(ui, model, &model.update, &mut actions);
}
