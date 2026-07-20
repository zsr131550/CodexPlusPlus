use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::desktop_integration::DesktopIntegrationViewState;
use codex_plus_manager_native::state::maintenance::{
    MaintenanceDocumentTab, MaintenanceTransition, MaintenanceViewState,
};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::maintenance::{self, MaintenanceAction};
use codex_plus_manager_native::views::shell::{ShellFeatureStates, render_shell};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

struct ViewState {
    maintenance: MaintenanceViewState,
    desktop_integration: DesktopIntegrationViewState,
    locale: Locale,
    emitted: Vec<MaintenanceAction>,
}

#[test]
fn maintenance_route_has_exact_bilingual_navigation_header_and_status() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "维护",
                "Codex++ 维护",
                "检查、启动并查看安全诊断",
                "状态: 已就绪",
            ],
        ),
        (
            Locale::En,
            [
                "Maintenance",
                "Codex++ Maintenance",
                "Inspect, launch, and review safe diagnostics",
                "Status: Ready",
            ],
        ),
    ] {
        let mut model = common::model(locale, ThemeMode::Dark);
        model.route = Route::Maintenance;
        let maintenance = loaded_state();
        let harness = Harness::builder()
            .with_size(egui::vec2(1180.0, 820.0))
            .build_ui(move |ui| {
                egui_extras::install_image_loaders(ui.ctx());
                theme::apply(ui.ctx(), ThemeMode::Dark);
                let _ = render_shell(
                    ui,
                    &model,
                    ShellFeatureStates {
                        maintenance: Some(&maintenance),
                        desktop_integration: Some(&common::desktop_integration_state(false, false)),
                        ..ShellFeatureStates::default()
                    },
                );
            });

        for label in labels {
            assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn maintenance_workbench_exposes_complete_read_only_and_command_semantics() {
    let harness = harness(
        900.0,
        ViewState {
            maintenance: loaded_state(),
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );

    for label in [
        "Codex application",
        "Application path",
        "Select executable",
        "Select directory",
        "Save application path",
        "Clear application path",
        "Debug port",
        "Helper port",
        "Launch Codex",
        "Desktop integration",
        "Desktop shortcut",
        "Start menu launcher",
        "Start menu manager",
        "URL protocol",
        "Start Codex++ at sign-in",
        "Logs",
        "Report",
        "50 lines",
        "100 lines",
        "200 lines",
        "Copy document",
        "Refresh diagnostics",
    ] {
        assert!(
            harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
                })
                .count()
                > 0,
            "{label}"
        );
    }

    for forbidden in [
        "Watcher",
        "Install entry points",
        "Uninstall entry points",
        "Restart",
        "Reset all settings",
    ] {
        assert!(harness.query_by_label(forbidden).is_none(), "{forbidden}");
    }
}

#[test]
fn maintenance_layout_switches_from_fixed_columns_to_vertical_stack() {
    let wide = harness(
        900.0,
        ViewState {
            maintenance: loaded_state(),
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );
    let wide_application = wide.get_by_label("Codex application").rect();
    let wide_diagnostics = wide.get_by_label("Diagnostics").rect();
    assert!(wide_diagnostics.min.x > wide_application.min.x + 300.0);
    assert!((wide_diagnostics.min.y - wide_application.min.y).abs() < 8.0);

    let compact = harness(
        700.0,
        ViewState {
            maintenance: loaded_state(),
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );
    let compact_path = compact.get_by_label("Application path").rect();
    let compact_diagnostics = compact.get_by_label("Diagnostics").rect();
    assert!(compact_diagnostics.min.y > compact_path.max.y + 120.0);
}

#[test]
fn maintenance_clear_and_discard_are_explicit_confirmations() {
    let mut clear = harness(
        900.0,
        ViewState {
            maintenance: loaded_state(),
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );
    clear.get_by_label("Clear application path").click();
    clear.run();
    for label in ["Clear application path?", "Clear path", "Cancel"] {
        assert!(clear.get_by_label(label).rect().is_positive(), "{label}");
    }

    let mut dirty = loaded_state();
    dirty.set_app_path("C:/private/dirty-path-sentinel.exe".to_owned());
    assert!(!dirty.request_transition(MaintenanceTransition::Refresh));
    let discard = harness(
        900.0,
        ViewState {
            maintenance: dirty,
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );
    for label in [
        "Discard application path changes?",
        "Discard changes",
        "Keep editing",
    ] {
        assert!(discard.get_by_label(label).rect().is_positive(), "{label}");
    }
}

#[test]
fn copy_is_exact_while_action_and_state_debug_are_redacted() {
    let mut state = loaded_state();
    state.set_document_tab(MaintenanceDocumentTab::Report);
    let expected = state.active_document_text().unwrap().to_owned();
    let mut harness = harness(
        900.0,
        ViewState {
            maintenance: state,
            desktop_integration: common::desktop_integration_state(false, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );
    harness.get_by_label("Copy document").click();
    harness.run();
    assert!(
        harness
            .state()
            .emitted
            .contains(&MaintenanceAction::CopyDocument(expected))
    );

    let action_debug = format!(
        "{:?}",
        MaintenanceAction::SetAppPath("C:/private/path-sentinel.exe".to_owned())
    );
    let document_debug = format!(
        "{:?}",
        MaintenanceAction::CopyDocument("raw-report-sentinel private-key-sentinel".to_owned())
    );
    let state_debug = format!("{:?}", harness.state().maintenance);
    for debug in [action_debug, document_debug, state_debug] {
        assert!(!debug.contains("path-sentinel"), "{debug}");
        assert!(!debug.contains("raw-report-sentinel"), "{debug}");
        assert!(!debug.contains("private-key-sentinel"), "{debug}");
    }
}

#[test]
fn desktop_repair_confirmation_lists_only_safe_item_kinds() {
    let mut harness = harness(
        900.0,
        ViewState {
            maintenance: loaded_state(),
            desktop_integration: common::desktop_integration_state(true, false),
            locale: Locale::En,
            emitted: Vec::new(),
        },
    );

    harness.get_by_label("Repair desktop integration").click();
    harness.run();
    for label in [
        "Repair desktop integration?",
        "Desktop shortcut",
        "Start menu launcher",
        "Start menu manager",
        "URL protocol",
        "Repair",
        "Cancel",
    ] {
        assert!(
            harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
                })
                .any(|node| node.rect().is_positive()),
            "{label}"
        );
    }
    let debug = format!("{:?}", harness.state().desktop_integration);
    assert!(!debug.contains("Program Files"));
    assert!(!debug.contains("CurrentVersion"));
}

#[test]
fn legacy_sign_in_is_on_and_requires_explicit_migration_in_both_locales() {
    for (locale, warning, command) in [
        (
            Locale::ZhCn,
            "检测到正在生效的旧版登录启动注册，请显式迁移。",
            "迁移登录启动注册",
        ),
        (
            Locale::En,
            "A legacy sign-in registration is active. Migrate it explicitly.",
            "Migrate sign-in registration",
        ),
    ] {
        let mut harness = harness(
            900.0,
            ViewState {
                maintenance: loaded_state(),
                desktop_integration: common::desktop_integration_state(false, true),
                locale,
                emitted: Vec::new(),
            },
        );
        assert!(
            harness.get_by_label(warning).rect().is_positive(),
            "{warning}"
        );
        assert!(
            harness.get_by_label(command).rect().is_positive(),
            "{command}"
        );
        harness.get_by_label(command).click();
        harness.run();
        assert!(
            harness
                .state()
                .emitted
                .contains(&MaintenanceAction::MigrateSignIn)
        );
    }
}

fn harness(width: f32, state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(width, 760.0))
        .build_ui_state(render, state)
}

fn render(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    maintenance::render(
        ui,
        &state.maintenance,
        &state.desktop_integration,
        state.locale,
        &mut actions,
    );
    for action in actions {
        match &action {
            MaintenanceAction::SetAppPath(path) => {
                state.maintenance.set_app_path(path.clone());
            }
            MaintenanceAction::RequestClear => {
                state.maintenance.request_clear();
            }
            MaintenanceAction::SetDebugPort(port) => state.maintenance.debug_port = *port,
            MaintenanceAction::SetHelperPort(port) => state.maintenance.helper_port = *port,
            MaintenanceAction::SetDocumentTab(tab) => state.maintenance.set_document_tab(*tab),
            MaintenanceAction::SetLogLimit(limit) => {
                state.maintenance.set_log_limit(*limit);
            }
            MaintenanceAction::CancelClear => state.maintenance.cancel_clear(),
            MaintenanceAction::CancelDiscard => state.maintenance.cancel_transition(),
            MaintenanceAction::RequestRepair => {
                state.desktop_integration.request_repair_confirmation();
            }
            MaintenanceAction::CancelRepair => {
                state.desktop_integration.cancel_repair_confirmation();
            }
            _ => {}
        }
        state.emitted.push(action);
    }
}

fn loaded_state() -> MaintenanceViewState {
    let mut state = MaintenanceViewState::default();
    let request_id = state.begin_load();
    assert!(state.apply_load_response(
        request_id,
        Ok(common::maintenance_workspace(
            "C:/private/path-sentinel.exe"
        )),
    ));
    state
}
