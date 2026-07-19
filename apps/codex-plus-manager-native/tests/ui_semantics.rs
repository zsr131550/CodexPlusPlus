use codex_plus_manager_native::i18n::{Locale, TextKey, ThemeMode, text};
use codex_plus_manager_native::state::settings::SettingsViewState;
use codex_plus_manager_native::state::user_scripts::{UserScriptFailureKind, UserScriptViewState};
use codex_plus_manager_native::state::{OverviewFailureKind, OverviewPhase, Route};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{
    ShellAction, ShellFeatureStates, ShellViewModel, render_shell,
};
use codex_plus_manager_service::UserScriptErrorKind;
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

use common::{model, snapshot};

struct TestShellState {
    model: ShellViewModel,
    scripts: Option<UserScriptViewState>,
    settings: Option<SettingsViewState>,
    emitted: Vec<ShellAction>,
}

fn render_test_shell(ui: &mut egui::Ui, state: &mut TestShellState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    let feature_states = ShellFeatureStates {
        user_scripts: state.scripts.as_ref(),
        settings: state.settings.as_ref(),
        ..ShellFeatureStates::default()
    };
    for action in render_shell(ui, &state.model, feature_states) {
        state.emitted.push(action.clone());
        match action {
            ShellAction::Navigate(route) => state.model.route = route,
            ShellAction::SetLocale(locale) => state.model.locale = locale,
            ShellAction::SetTheme(theme) => state.model.theme = theme,
            ShellAction::Refresh | ShellAction::Retry => {}
            ShellAction::Provider(_) => {}
            ShellAction::Import(_)
            | ShellAction::Environment(_)
            | ShellAction::Sessions(_)
            | ShellAction::UserScripts(_)
            | ShellAction::Context(_)
            | ShellAction::Marketplace(_)
            | ShellAction::Enhancements(_)
            | ShellAction::ZedRemote(_)
            | ShellAction::Maintenance(_)
            | ShellAction::Settings(_) => {}
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
                scripts: None,
                settings: None,
                emitted: Vec::new(),
            },
        )
}

fn harness_with_scripts(
    size: [f32; 2],
    model: ShellViewModel,
    scripts: UserScriptViewState,
) -> Harness<'static, TestShellState> {
    Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(
            render_test_shell,
            TestShellState {
                model,
                scripts: Some(scripts),
                settings: None,
                emitted: Vec::new(),
            },
        )
}

fn harness_with_settings(
    size: [f32; 2],
    model: ShellViewModel,
    settings: SettingsViewState,
) -> Harness<'static, TestShellState> {
    Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(
            render_test_shell,
            TestShellState {
                model,
                scripts: None,
                settings: Some(settings),
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
fn shell_navigates_to_the_native_scripts_route() {
    let mut harness = harness([960.0, 720.0], model(Locale::En, ThemeMode::Dark));

    harness.get_by_label("Scripts").click();
    harness.run();

    assert!(
        harness
            .state()
            .emitted
            .contains(&ShellAction::Navigate(Route::Scripts))
    );
    assert!(harness.get_by_label("Codex++ Scripts").rect().is_positive());
}

#[test]
fn settings_route_sits_between_maintenance_and_about_with_ready_shell_copy() {
    for locale in [Locale::ZhCn, Locale::En] {
        let mut shell = model(locale, ThemeMode::Dark);
        shell.route = Route::Settings;
        let settings = SettingsViewState::from_workspace(common::manager_settings_workspace(1));
        let harness = harness_with_settings([1180.0, 820.0], shell, settings);

        let maintenance = harness
            .get_by_label(text(locale, TextKey::Maintenance))
            .rect();
        let settings = harness.get_by_label(text(locale, TextKey::Settings)).rect();
        let about = harness.get_by_label(text(locale, TextKey::About)).rect();
        assert!(
            maintenance.max.y <= settings.min.y,
            "{maintenance:?} {settings:?}"
        );
        assert!(settings.max.y <= about.min.y, "{settings:?} {about:?}");

        for label in [
            format!(
                "{} {}",
                text(locale, TextKey::AppName),
                text(locale, TextKey::Settings)
            ),
            text(locale, TextKey::SettingsSubtitle).to_owned(),
            format!(
                "{}: {}",
                text(locale, TextKey::Status),
                text(locale, TextKey::Ready)
            ),
        ] {
            assert!(harness.get_by_label(&label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn scripts_header_uses_the_active_tab_failure_copy() {
    let mut scripts = UserScriptViewState::default();
    let request_id = scripts.begin_market_refresh();
    scripts.apply_market_response(
        request_id,
        Err(UserScriptFailureKind::Service(
            UserScriptErrorKind::MarketRefreshFailed,
        )),
    );
    let mut shell = model(Locale::En, ThemeMode::Dark);
    shell.route = Route::Scripts;
    let harness = harness_with_scripts([960.0, 720.0], shell, scripts);

    assert!(
        harness
            .get_by_label("Status: Script market load failed")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .query_by_label("Status: Local scripts load failed")
            .is_none()
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
