use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::{OverviewFailureKind, OverviewPhase};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellAction, ShellViewModel, render_shell};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

use common::{model, snapshot};

struct TestShellState {
    model: ShellViewModel,
    emitted: Vec<ShellAction>,
}

fn render_test_shell(ui: &mut egui::Ui, state: &mut TestShellState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    for action in render_shell(ui, &state.model, None, None, None, None) {
        state.emitted.push(action.clone());
        match action {
            ShellAction::Navigate(route) => state.model.route = route,
            ShellAction::SetLocale(locale) => state.model.locale = locale,
            ShellAction::SetTheme(theme) => state.model.theme = theme,
            ShellAction::Refresh | ShellAction::Retry => {}
            ShellAction::Provider(_) => {}
            ShellAction::Import(_) | ShellAction::Environment(_) | ShellAction::Context(_) => {}
        }
        ui.ctx().request_repaint();
    }
}

fn harness(size: [f32; 2], model: ShellViewModel) -> Harness<'static, TestShellState> {
    Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(
            render_test_shell,
            TestShellState {
                model,
                emitted: Vec::new(),
            },
        )
}

#[test]
fn chinese_shell_navigates_to_about_and_switches_language_live() {
    let mut harness = harness([1180.0, 820.0], model(Locale::ZhCn, ThemeMode::Dark));

    harness.get_by_label("关于").click();
    harness.run();
    assert!(
        harness
            .get_by_label("关于 Codex++")
            .rect()
            .intersects(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1180.0, 820.0),
            ))
    );

    harness.get_by_label("English").click();
    harness.run();
    assert!(
        harness
            .get_by_label("About Codex++")
            .rect()
            .intersects(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1180.0, 820.0),
            ))
    );
}

#[test]
fn compact_overview_keeps_operational_controls_and_rows_visible() {
    let harness = harness([960.0, 720.0], model(Locale::ZhCn, ThemeMode::Dark));
    let labels = [
        "概览",
        "关于",
        "刷新",
        "中文",
        "English",
        "深色",
        "浅色",
        "Codex 应用",
        "静默启动入口",
        "管理工具入口",
        "Codex++ 版本",
        "设置文件",
        "诊断日志",
        "状态: 已就绪",
        "渲染器: WGPU",
    ];

    for label in labels {
        let node = harness.get_by_label(label);
        let rect = node.rect();
        assert!(rect.is_positive(), "empty rect: {label}: {rect:?}");
        assert!(rect.min.x >= 0.0 && rect.min.y >= 0.0, "{label}: {rect:?}");
        assert!(
            rect.max.x <= 960.0 && rect.max.y <= 720.0,
            "{label}: {rect:?}"
        );
    }
}

#[test]
fn retry_emits_action_and_keeps_last_good_snapshot_visible() {
    let mut error_model = model(Locale::En, ThemeMode::Light);
    error_model.overview_phase = OverviewPhase::Error;
    error_model.overview_error = Some(OverviewFailureKind::LoadFailed);
    error_model.overview_snapshot = Some(snapshot("kept-version"));
    let mut harness = harness([960.0, 720.0], error_model);

    assert!(harness.get_by_label("kept-version").rect().is_positive());
    harness.get_by_label("Retry").click();
    harness.run();

    assert!(harness.state().emitted.contains(&ShellAction::Retry));
    assert!(harness.get_by_label("kept-version").rect().is_positive());
}
