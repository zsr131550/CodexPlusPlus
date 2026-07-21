use std::sync::Arc;

use codex_plus_core::env_conflicts::{
    EnvConflict, EnvConflictRemoval, EnvConflictRemovalFailure, EnvConflictSource,
};
use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_core::settings::RelayProtocol;
use codex_plus_manager::i18n::{Locale, ThemeMode};
use codex_plus_manager::state::Route;
use codex_plus_manager::state::environment::{EnvironmentFailureKind, EnvironmentViewState};
use codex_plus_manager::state::import::ImportViewState;
use codex_plus_manager::state::provider::ProviderViewState;
use codex_plus_manager::theme;
use codex_plus_manager::views::environment::EnvironmentAction;
use codex_plus_manager::views::import::ImportAction;
use codex_plus_manager::views::shell::{
    ShellAction, ShellFeatureStates, ShellViewModel, render_shell,
};
use codex_plus_manager_service::{
    CcsDiscovery, CcsProviderSummary, EnvironmentRemovalOutcome, PendingImportSnapshot,
    PendingImportSummary, ProviderRevision, RelayEnvironmentErrorKind, RelayEnvironmentWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

const SECRET_SENTINEL: &str = "sk-ui-import-secret-sentinel";

struct FeatureState {
    model: ShellViewModel,
    provider: ProviderViewState,
    provider_import: ImportViewState,
    environment: EnvironmentViewState,
    emitted: Vec<ShellAction>,
}

fn render(ui: &mut egui::Ui, state: &mut FeatureState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    for action in render_shell(
        ui,
        &state.model,
        ShellFeatureStates {
            provider: Some(&state.provider),
            provider_import: Some(&state.provider_import),
            environment: Some(&state.environment),
            ..ShellFeatureStates::default()
        },
    ) {
        match &action {
            ShellAction::Navigate(route) => state.model.route = *route,
            ShellAction::Environment(EnvironmentAction::SetSelected { name, selected }) => {
                state.environment.toggle_selection(name, *selected);
            }
            ShellAction::Environment(EnvironmentAction::RequestCleanup) => {
                state.environment.request_cleanup_confirmation();
            }
            ShellAction::Environment(EnvironmentAction::CancelCleanup) => {
                state.environment.cancel_cleanup_confirmation();
            }
            ShellAction::Import(_)
            | ShellAction::Environment(_)
            | ShellAction::Sessions(_)
            | ShellAction::UserScripts(_)
            | ShellAction::Context(_)
            | ShellAction::Marketplace(_)
            | ShellAction::Enhancements(_)
            | ShellAction::ZedRemote(_)
            | ShellAction::Maintenance(_)
            | ShellAction::Settings(_)
            | ShellAction::Update(_) => {}
            ShellAction::Refresh
            | ShellAction::Retry
            | ShellAction::SetLocale(_)
            | ShellAction::SetTheme(_)
            | ShellAction::Provider(_) => {}
        }
        state.emitted.push(action);
        ui.ctx().request_repaint();
    }
}

fn harness(state: FeatureState) -> Harness<'static, FeatureState> {
    Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(render, state)
}

fn base_state(locale: Locale, route: Route) -> FeatureState {
    let mut model = common::model(locale, ThemeMode::Dark);
    model.route = route;
    FeatureState {
        model,
        provider: common::provider_state(),
        provider_import: ImportViewState::default(),
        environment: environment_state(),
        emitted: Vec::new(),
    }
}

fn environment_state() -> EnvironmentViewState {
    let mut state = EnvironmentViewState::default();
    let request_id = state.begin_inspection();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(RelayEnvironmentWorkspace {
            report: RelayEnvironmentReport {
                clash_verge_tun: ClashVergeTunCheck {
                    enabled: false,
                    config_path: None,
                },
                proxy_environment: ProxyEnvironmentCheck {
                    variables: Vec::new(),
                },
                codex_env_file: CodexEnvFileCheck {
                    exists: false,
                    path: "fixture/.env".to_owned(),
                },
            },
            conflicts: vec![
                EnvConflict {
                    name: "OPENAI_API_KEY".to_owned(),
                    source: EnvConflictSource::Process,
                    value_present: true,
                },
                EnvConflict {
                    name: "OPENAI_BASE_URL".to_owned(),
                    source: EnvConflictSource::User,
                    value_present: true,
                },
            ],
            revision: "a".repeat(64),
        })),
    );
    state
}

fn discovery_state() -> ImportViewState {
    let mut state = ImportViewState::default();
    let request_id = state.begin_discovery();
    state.apply_discovery_response(
        request_id,
        Ok(Arc::new(CcsDiscovery {
            source_path: "fixture/cc-switch.db".to_owned(),
            source_revision: "b".repeat(64),
            provider_revision: ProviderRevision::parse("c".repeat(64)).unwrap(),
            providers: vec![CcsProviderSummary {
                source_id: "fixture".to_owned(),
                name: "Safe provider".to_owned(),
                base_url: "https://safe.invalid/v1".to_owned(),
                protocol: RelayProtocol::Responses,
                duplicate: false,
            }],
            importable_count: 1,
            duplicate_count: 0,
        })),
    );
    state
}

fn pending_state() -> ImportViewState {
    let mut state = ImportViewState::default();
    let request_id = state.begin_pending_load();
    state.apply_pending_load_response(
        request_id,
        Ok(PendingImportSnapshot {
            pending: Some(PendingImportSummary {
                name: "Pending safe provider".to_owned(),
                base_url: "https://pending.invalid/v1".to_owned(),
                wire_api: "responses".to_owned(),
                relay_mode: "pureApi".to_owned(),
                api_key_present: true,
                revision: "d".repeat(64),
            }),
        }),
    );
    state
}

#[test]
fn environment_route_has_bilingual_navigation_and_operational_semantics() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "环境检查",
                "Codex++ 环境检查",
                "中转环境诊断",
                "TUN 模式",
                "Codex .env 文件",
                "OPENAI 环境冲突",
                "清理所选项",
            ],
        ),
        (
            Locale::En,
            [
                "Environment",
                "Codex++ Environment",
                "Relay environment diagnostics",
                "TUN mode",
                "Codex .env file",
                "OPENAI environment conflicts",
                "Clean selected",
            ],
        ),
    ] {
        let harness = harness(base_state(locale, Route::Environment));
        for label in labels {
            let rect = harness.get_by_label(label).rect();
            assert!(rect.is_positive(), "missing {label}");
            assert!(
                rect.max.x <= 960.0 && rect.max.y <= 720.0,
                "{label}: {rect:?}"
            );
        }
    }
}

#[test]
fn environment_selection_opens_exact_name_confirmation() {
    let mut harness = harness(base_state(Locale::En, Route::Environment));
    assert_eq!(harness.state().environment.selected_names().count(), 0);
    harness.get_by_label("OPENAI_API_KEY").click();
    harness.run();
    assert!(harness.state().environment.is_selected("OPENAI_API_KEY"));
    harness.get_by_label("Clean selected").click();
    harness.run();
    harness.run();
    assert!(
        harness
            .get_by_label("Confirm environment cleanup")
            .rect()
            .is_positive()
    );
    assert!(harness.get_by_label("Confirm cleanup").rect().is_positive());
}

#[test]
fn ccs_import_modal_exposes_safe_summary_and_dirty_guard() {
    let mut state = base_state(Locale::En, Route::Providers);
    state.provider_import = discovery_state();
    state.provider.draft_mut().unwrap().default_test_model = "dirty".to_owned();
    let harness = harness(state);

    for label in [
        "Import providers",
        "cc-switch provider import",
        "Safe provider",
        "https://safe.invalid/v1",
        "Save or discard the provider draft first.",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
    assert!(
        harness
            .query_by(|node| node.label().as_deref() == Some("Import new") && node.is_disabled())
            .is_some()
    );
}

#[test]
fn pending_modal_never_exposes_secret_and_keeps_dismiss_enabled_when_dirty() {
    let mut state = base_state(Locale::En, Route::Overview);
    state.provider_import = pending_state();
    state.provider.draft_mut().unwrap().default_test_model = "dirty".to_owned();
    let harness = harness(state);

    for label in [
        "Confirm pending provider import",
        "Pending safe provider",
        "API key provided",
        "Dismiss import",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Confirm import") && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Dismiss import") && !node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label()
                    .is_some_and(|label| label.contains(SECRET_SENTINEL))
                    || node
                        .value()
                        .is_some_and(|value| value.contains(SECRET_SENTINEL))
            })
            .is_none()
    );
}

#[test]
fn pending_modal_exposes_explicit_refresh_action() {
    let mut state = base_state(Locale::En, Route::Overview);
    state.provider_import = pending_state();
    let mut harness = harness(state);

    harness.get_by_label("Refresh pending import").click();
    harness.run();

    assert!(
        harness
            .state()
            .emitted
            .iter()
            .any(|action| matches!(action, ShellAction::Import(ImportAction::RefreshPending)))
    );
}

#[test]
fn pending_review_takes_priority_over_ccs_modal() {
    let mut state = base_state(Locale::En, Route::Providers);
    state.provider_import = discovery_state();
    let pending_id = state.provider_import.begin_pending_load();
    state.provider_import.apply_pending_load_response(
        pending_id,
        Ok(PendingImportSnapshot {
            pending: pending_state().pending,
        }),
    );
    let harness = harness(state);

    assert!(
        harness
            .get_by_label("Confirm pending provider import")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .query_by(|node| node.label().as_deref() == Some("cc-switch provider import"))
            .is_none()
    );
}

#[test]
fn cleanup_result_shows_backup_path_and_per_name_outcomes() {
    let mut state = base_state(Locale::En, Route::Environment);
    state.environment.toggle_selection("OPENAI_API_KEY", true);
    state.environment.toggle_selection("OPENAI_BASE_URL", true);
    assert!(state.environment.request_cleanup_confirmation());
    let (request_id, _) = state.environment.begin_cleanup().unwrap();
    let backup_path =
        "C:/isolated/native-manager/backups/env-conflicts-very-long-evidence-name.json";
    state.environment.apply_cleanup_response(
        request_id,
        Ok(Arc::new(EnvironmentRemovalOutcome {
            removed: vec![
                EnvConflictRemoval {
                    name: "OPENAI_API_KEY".to_owned(),
                    removed_process: true,
                    removed_user: false,
                },
                EnvConflictRemoval {
                    name: "OPENAI_BASE_URL".to_owned(),
                    removed_process: false,
                    removed_user: false,
                },
            ],
            failures: vec![EnvConflictRemovalFailure {
                name: "OPENAI_BASE_URL".to_owned(),
                source: EnvConflictSource::User,
            }],
            backup_path: Some(backup_path.to_owned()),
            remaining: vec![EnvConflict {
                name: "OPENAI_BASE_URL".to_owned(),
                source: EnvConflictSource::User,
                value_present: true,
            }],
            report: environment_state().workspace.unwrap().report.clone(),
            revision: "e".repeat(64),
        })),
    );
    let harness = harness(state);

    for label in [backup_path, "Process: Removed", "User: Failed"] {
        let rect = harness.get_by_label(label).rect();
        assert!(rect.is_positive(), "{label}");
        assert!(
            rect.max.x <= 960.0 && rect.max.y <= 720.0,
            "{label}: {rect:?}"
        );
    }
}

#[test]
fn cleanup_service_failure_uses_cleanup_specific_copy() {
    let mut state = base_state(Locale::En, Route::Environment);
    state.environment.toggle_selection("OPENAI_API_KEY", true);
    assert!(state.environment.request_cleanup_confirmation());
    let (request_id, _) = state.environment.begin_cleanup().unwrap();
    state.environment.apply_cleanup_response(
        request_id,
        Err(EnvironmentFailureKind::Service(
            RelayEnvironmentErrorKind::MutationFailed,
        )),
    );
    let harness = harness(state);

    assert!(
        harness
            .get_by_label("Environment cleanup failed")
            .rect()
            .is_positive()
    );
}

#[test]
fn stale_cleanup_uses_environment_specific_copy() {
    let mut state = base_state(Locale::En, Route::Environment);
    state.environment.toggle_selection("OPENAI_API_KEY", true);
    assert!(state.environment.request_cleanup_confirmation());
    let (request_id, _) = state.environment.begin_cleanup().unwrap();
    state.environment.apply_cleanup_response(
        request_id,
        Err(EnvironmentFailureKind::Service(
            RelayEnvironmentErrorKind::Conflict,
        )),
    );
    let harness = harness(state);

    assert!(
        harness
            .get_by_label("The environment changed. Inspect it again.")
            .rect()
            .is_positive()
    );
}
