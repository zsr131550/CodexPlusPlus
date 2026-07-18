use std::sync::Arc;

use codex_plus_core::models::DeleteStatus;
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::provider::OperationPhase;
use codex_plus_manager_native::state::sessions::{SessionFilter, SessionViewState};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::sessions::{self, SessionAction};
use codex_plus_manager_native::views::shell::{ShellFeatureStates, render_shell};
use codex_plus_manager_service::{
    ProviderSyncRevision, ProviderSyncTargetList, ProviderSyncTargetOption,
    ProviderSyncTargetSource, ProviderSyncWorkspace, SessionDeleteBatchOutcome,
    SessionDeleteOutcome, SessionRevision, SessionSummary, SessionWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

struct ViewState {
    sessions: SessionViewState,
    locale: Locale,
    emitted: Vec<SessionAction>,
}

#[test]
fn sessions_route_has_exact_bilingual_navigation_and_header_copy() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            ["会话管理", "Codex++ 会话管理", "搜索、清理与修复本地会话"],
        ),
        (
            Locale::En,
            [
                "Session management",
                "Codex++ Session management",
                "Search, clean up, and repair local sessions",
            ],
        ),
    ] {
        let mut model = common::model(locale, ThemeMode::Dark);
        model.route = Route::Sessions;
        let sessions = loaded_state(2);
        let harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 800.0))
            .build_ui(move |ui| {
                egui_extras::install_image_loaders(ui.ctx());
                theme::apply(ui.ctx(), ThemeMode::Dark);
                let _ = render_shell(
                    ui,
                    &model,
                    ShellFeatureStates {
                        sessions: Some(&sessions),
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
fn workbench_has_complete_bilingual_semantics() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "搜索会话",
                "刷新会话",
                "全部 2",
                "活跃 1",
                "已归档 1",
                "全选筛选结果",
                "删除选中的会话",
                "历史会话修复",
                "供应商目标",
                "自动修复",
                "运行供应商修复",
            ],
        ),
        (
            Locale::En,
            [
                "Search sessions",
                "Refresh sessions",
                "All 2",
                "Active 1",
                "Archived 1",
                "Select all filtered",
                "Delete selected sessions",
                "Historical session repair",
                "Provider target",
                "Automatic repair",
                "Run provider repair",
            ],
        ),
    ] {
        let harness = harness(ViewState {
            sessions: loaded_state(2),
            locale,
            emitted: Vec::new(),
        });
        for label in labels {
            assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn select_all_control_emits_one_cross_page_action() {
    let mut harness = harness(ViewState {
        sessions: loaded_state(51),
        locale: Locale::En,
        emitted: Vec::new(),
    });

    harness.get_by_label("Select all filtered").click();
    harness.run();

    assert_eq!(
        harness.state().emitted,
        vec![SessionAction::SelectAllFiltered]
    );
}

#[test]
fn destructive_controls_are_disabled_while_delete_is_running() {
    let mut sessions = loaded_state(2);
    sessions.selected_ids.insert("session-0".to_owned());
    sessions.delete_phase = OperationPhase::Running;
    let harness = harness(ViewState {
        sessions,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in ["Delete selected sessions", "Select all filtered"] {
        assert!(
            harness
                .query_by(|node| node.label().as_deref() == Some(label) && node.is_disabled())
                .is_some(),
            "{label}"
        );
    }
}

#[test]
fn delete_confirmation_repeats_exact_count_and_bounded_preview() {
    let mut sessions = loaded_state(8);
    sessions.select_all_filtered();
    assert!(sessions.request_delete());
    let harness = harness(ViewState {
        sessions,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "Confirm deletion",
        "Delete 8 selected sessions?",
        "Preview: Session 0",
        "Preview: Session 5",
        "2 more sessions",
        "SQLite session records and related rollout files will be removed; backups are created before local session data is changed.",
        "Cancel",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
}

#[test]
fn provider_repair_confirmation_repeats_the_frozen_target() {
    let mut sessions = loaded_state(2);
    assert!(sessions.request_provider_run_confirmation());
    let harness = harness(ViewState {
        sessions,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "Confirm provider repair",
        "Target provider: openai",
        "Historical session provider metadata is backed up before it is updated.",
        "Cancel",
        "Repair now",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
}

#[test]
fn initial_loading_does_not_report_zero_read_issues() {
    let mut sessions = SessionViewState::default();
    sessions.begin_workspace_refresh();
    sessions.begin_provider_workspace_refresh().unwrap();
    let harness = harness(ViewState {
        sessions,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    assert!(harness.query_by_label("Read issues: 0").is_none());
}

#[test]
fn delete_result_reports_every_session_status_and_backup_evidence() {
    let mut sessions = loaded_state(3);
    sessions.delete_outcome = Some(Arc::new(SessionDeleteBatchOutcome {
        outcomes: vec![
            SessionDeleteOutcome::metadata_only(
                "session-0",
                DeleteStatus::LocalDeleted,
                Some("C:/backups/session-0.json".to_owned()),
            ),
            SessionDeleteOutcome::metadata_only(
                "session-1",
                DeleteStatus::Partial,
                Some("C:/backups/session-1.json".to_owned()),
            ),
            SessionDeleteOutcome::metadata_only("session-2", DeleteStatus::Failed, None),
        ],
        workspace: SessionWorkspace::default(),
    }));
    let harness = harness(ViewState {
        sessions,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "session-0: Deleted",
        "Backup evidence: C:/backups/session-0.json",
        "session-1: Partial",
        "Backup evidence: C:/backups/session-1.json",
        "session-2: Failed",
        "Backup evidence: None",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
}

fn harness(state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(900.0, 720.0))
        .build_ui_state(render, state)
}

fn render(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    sessions::render(ui, &state.sessions, state.locale, &mut actions);
    state.emitted.extend(actions);
}

fn loaded_state(count: usize) -> SessionViewState {
    let mut state = SessionViewState::default();
    let mut workspace = SessionWorkspace {
        db_paths: vec!["sessions.sqlite".to_owned()],
        sessions: (0..count)
            .map(|index| {
                let mut session = SessionSummary::new(
                    format!("session-{index}"),
                    format!("Session {index}"),
                    SessionRevision::from_digest([index as u8; 32]),
                );
                session.cwd = format!("C:/workspace/{index}");
                session.model_provider = "openai".to_owned();
                session.archived = index == 1;
                session.updated_at_ms = Some(1_700_000_000_000 + index as i64);
                session.source_db_paths = vec!["sessions.sqlite".to_owned()];
                session
            })
            .collect(),
        read_issues: Vec::new(),
    };
    if count > 2 {
        for session in &mut workspace.sessions {
            session.archived = false;
        }
    }
    let request_id = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(request_id, Ok(Arc::new(workspace))));
    let provider_id = state.begin_provider_workspace_refresh().unwrap();
    assert!(state.apply_provider_workspace_response(
        provider_id,
        Ok(Arc::new(ProviderSyncWorkspace {
            targets: ProviderSyncTargetList {
                current_provider: "openai".to_owned(),
                targets: vec![ProviderSyncTargetOption {
                    id: "openai".to_owned(),
                    sources: vec![ProviderSyncTargetSource::Config],
                    is_current_provider: true,
                    is_manual: false,
                    is_saved: true,
                }],
            },
            selected_target: "openai".to_owned(),
            auto_repair: false,
            revision: ProviderSyncRevision::from_digest([9; 32]),
        })),
    ));
    state.filter = SessionFilter::All;
    state
}
