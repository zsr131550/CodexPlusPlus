use std::sync::Arc;

use codex_plus_core::zed_remote::{
    SshTarget, ZedAvailability, ZedOpenStrategy, ZedRemoteProjectSource, ZedRemoteRegistryRevision,
};
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::zed_remote::ZedRemoteViewState;
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellFeatureStates, render_shell};
use codex_plus_manager_native::views::zed_remote::{self, ZedRemoteAction};
use codex_plus_manager_service::{
    ZedProjectRevision, ZedRememberOutcome, ZedRemoteErrorKind, ZedRemoteOpenOutcome,
    ZedRemoteProjectSummary, ZedRemoteWorkspace, ZedSettingsRevision,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

struct ViewState {
    zed: ZedRemoteViewState,
    locale: Locale,
    emitted: Vec<ZedRemoteAction>,
}

#[test]
fn zed_route_has_exact_bilingual_navigation_and_header_copy() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "Zed 远程",
                "Codex++ Zed 远程",
                "管理 Zed SSH 工作区并保存最近项目",
            ],
        ),
        (
            Locale::En,
            [
                "Zed Remote",
                "Codex++ Zed Remote",
                "Manage Zed SSH workspaces and recent projects",
            ],
        ),
    ] {
        let mut model = common::model(locale, ThemeMode::Dark);
        model.route = Route::ZedRemote;
        let zed = loaded_state();
        let harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 800.0))
            .build_ui(move |ui| {
                egui_extras::install_image_loaders(ui.ctx());
                theme::apply(ui.ctx(), ThemeMode::Dark);
                let _ = render_shell(
                    ui,
                    &model,
                    ShellFeatureStates {
                        zed_remote: Some(&zed),
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
fn zed_workbench_exposes_preferences_sources_and_project_commands() {
    let harness = harness(ViewState {
        zed: loaded_state(),
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "Search projects",
        "Open strategy",
        "Recent project registry",
        "Save Zed preferences",
        "Current project",
        "Recent projects",
        "Discovered projects",
        "Copy URL",
        "Forget",
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
}

#[test]
fn open_and_forget_use_explicit_confirmation_dialogs() {
    let mut open_harness = harness(ViewState {
        zed: loaded_state(),
        locale: Locale::En,
        emitted: Vec::new(),
    });
    open_harness
        .query_all_by(|node| node.label().as_deref() == Some("Open"))
        .next()
        .expect("open button")
        .click();
    open_harness.run();
    for label in [
        "Confirm opening Zed project",
        "Remember opened projects",
        "Open now",
    ] {
        assert!(
            open_harness.get_by_label(label).rect().is_positive(),
            "{label}"
        );
    }

    let mut forget_harness = harness(ViewState {
        zed: loaded_state(),
        locale: Locale::En,
        emitted: Vec::new(),
    });
    forget_harness.get_by_label("Forget").click();
    forget_harness.run();
    for label in ["Confirm forgetting Zed project", "Forget now", "Cancel"] {
        assert!(
            forget_harness.get_by_label(label).rect().is_positive(),
            "{label}"
        );
    }
}

#[test]
fn compact_view_keeps_core_controls_inside_the_viewport() {
    let harness = Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(
            render,
            ViewState {
                zed: loaded_state(),
                locale: Locale::En,
                emitted: Vec::new(),
            },
        );
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 720.0));
    for label in [
        "Search projects",
        "Open strategy",
        "Current project",
        "Copy URL",
    ] {
        let matches = harness
            .query_all_by(|node| {
                node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
            })
            .collect::<Vec<_>>();
        assert!(!matches.is_empty(), "{label}");
        for node in matches {
            let rect = node.rect();
            assert!(
                rect.is_positive() && viewport.contains_rect(rect),
                "{label}: {rect:?}"
            );
        }
    }
}

#[test]
fn unavailable_zed_disables_launch_but_keeps_copy_and_refresh_available() {
    let mut zed = loaded_state();
    Arc::make_mut(zed.workspace.as_mut().unwrap()).availability = ZedAvailability {
        platform_supported: true,
        cli_found: false,
        app_found: false,
    };
    assert!(zed.request_open("current", ZedOpenStrategy::Default, true));
    let harness = harness(ViewState {
        zed,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    assert!(
        harness
            .query_by(|node| { node.label().as_deref() == Some("Open now") && node.is_disabled() })
            .is_some()
    );
    for label in ["Copy URL", "Refresh"] {
        assert!(
            harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) && !node.is_disabled()
                })
                .count()
                > 0,
            "{label}"
        );
    }
}

#[test]
fn save_and_copy_emit_only_explicit_typed_actions() {
    let mut zed = loaded_state();
    zed.set_strategy(ZedOpenStrategy::NewWindow);
    let mut save_harness = harness(ViewState {
        zed,
        locale: Locale::En,
        emitted: Vec::new(),
    });
    save_harness.get_by_label("Save Zed preferences").click();
    save_harness.run();
    assert_eq!(
        save_harness.state().emitted,
        vec![ZedRemoteAction::SavePreferences]
    );

    let mut copy_harness = harness(ViewState {
        zed: loaded_state(),
        locale: Locale::En,
        emitted: Vec::new(),
    });
    copy_harness
        .query_all_by_label("Copy URL")
        .next()
        .unwrap()
        .click();
    copy_harness.run();
    assert_eq!(
        copy_harness.state().emitted,
        vec![ZedRemoteAction::CopyUrl("current".to_owned())]
    );
}

#[test]
fn forget_is_exposed_only_for_recent_rows() {
    let harness = harness(ViewState {
        zed: loaded_state(),
        locale: Locale::En,
        emitted: Vec::new(),
    });
    assert_eq!(harness.query_all_by_label("Forget").count(), 1);
}

#[test]
fn settings_conflict_and_partial_success_keep_actionable_operational_state() {
    let mut conflict = loaded_state();
    conflict.set_strategy(ZedOpenStrategy::NewWindow);
    let (save_id, _) = conflict.begin_save_preferences().unwrap();
    assert!(conflict.apply_save_response(
        save_id,
        Err(
            codex_plus_manager_native::state::zed_remote::ZedRemoteFailureKind::Service(
                ZedRemoteErrorKind::SettingsConflict,
            )
        ),
    ));
    let conflict_harness = harness(ViewState {
        zed: conflict,
        locale: Locale::En,
        emitted: Vec::new(),
    });
    for label in ["Zed preferences changed", "Save Zed preferences", "Refresh"] {
        assert!(
            conflict_harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
                })
                .count()
                > 0,
            "{label}"
        );
    }

    let mut partial = loaded_state();
    assert!(partial.request_open("current", ZedOpenStrategy::Default, true));
    let (open_id, _) = partial.begin_open().unwrap();
    assert!(partial.apply_open_response(
        open_id,
        Ok(Arc::new(ZedRemoteOpenOutcome {
            workspace: workspace(),
            strategy: ZedOpenStrategy::Default,
            url: "zed://redacted".to_owned(),
            remember: ZedRememberOutcome::Failed(ZedRemoteErrorKind::RegistryWriteFailed),
        })),
    ));
    let partial_harness = harness(ViewState {
        zed: partial,
        locale: Locale::En,
        emitted: Vec::new(),
    });
    for label in [
        "Zed project opened",
        "Project opened, but saving it to recents failed",
    ] {
        assert!(
            partial_harness.get_by_label(label).rect().is_positive(),
            "{label}"
        );
    }
}

#[test]
fn long_ipv6_metadata_stays_left_of_the_fixed_command_column() {
    let mut fixture = workspace();
    fixture.projects.truncate(1);
    fixture.projects[0].ssh.host = "2001:0db8:85a3:0000:0000:8a2e:0370:7334".to_owned();
    fixture.projects[0].remote_path = format!("/{}", "very-long-segment/".repeat(12));
    let mut zed = ZedRemoteViewState::default();
    let load = zed.begin_load();
    assert!(zed.apply_load_response(load, Ok(Arc::new(fixture))));
    let harness = Harness::builder()
        .with_size(egui::vec2(960.0, 720.0))
        .build_ui_state(
            render,
            ViewState {
                zed,
                locale: Locale::En,
                emitted: Vec::new(),
            },
        );
    let metadata = harness
        .query_all_by(|node| {
            node.label()
                .as_deref()
                .is_some_and(|label| label.contains("dev@[2001:0db8"))
                || node
                    .value()
                    .as_deref()
                    .is_some_and(|value| value.contains("dev@[2001:0db8"))
        })
        .next()
        .expect("IPv6 metadata");
    let open = harness.get_by_label("Open");
    assert!(
        metadata.rect().max.x <= open.rect().min.x,
        "{:?} / {:?}",
        metadata.rect(),
        open.rect()
    );
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
    zed_remote::render(ui, &state.zed, state.locale, &mut actions);
    for action in actions {
        match &action {
            ZedRemoteAction::RequestOpen {
                project_id,
                strategy,
                remember,
            } => {
                state
                    .zed
                    .request_open(project_id.clone(), *strategy, *remember);
            }
            ZedRemoteAction::RequestForget(project_id) => {
                state.zed.request_forget(project_id.clone());
            }
            ZedRemoteAction::CancelOpen => {
                state.zed.cancel_open();
            }
            ZedRemoteAction::CancelForget => {
                state.zed.cancel_forget();
            }
            _ => {}
        }
        state.emitted.push(action);
    }
}

fn loaded_state() -> ZedRemoteViewState {
    let mut state = ZedRemoteViewState::default();
    let request_id = state.begin_load();
    assert!(state.apply_load_response(request_id, Ok(Arc::new(workspace()))));
    state
}

fn workspace() -> ZedRemoteWorkspace {
    ZedRemoteWorkspace {
        settings_revision: ZedSettingsRevision::from_digest([1; 32]),
        registry_revision: ZedRemoteRegistryRevision::from_digest([2; 32]),
        default_strategy: ZedOpenStrategy::ReuseWindow,
        registry_enabled: true,
        availability: ZedAvailability {
            platform_supported: true,
            cli_found: true,
            app_found: true,
        },
        projects: vec![
            project(
                "current",
                "Current workspace",
                ZedRemoteProjectSource::CurrentThread,
            ),
            project("recent", "Recent workspace", ZedRemoteProjectSource::Recent),
            project(
                "discovered",
                "Discovered workspace",
                ZedRemoteProjectSource::SqliteThreadCwd,
            ),
        ],
    }
}

fn project(id: &str, label: &str, source: ZedRemoteProjectSource) -> ZedRemoteProjectSummary {
    ZedRemoteProjectSummary {
        id: id.to_owned(),
        revision: ZedProjectRevision::from_digest([id.len() as u8; 32]),
        label: label.to_owned(),
        host_id: "fixture-host".to_owned(),
        ssh: SshTarget {
            user: "dev".to_owned(),
            host: "fixture.example.test".to_owned(),
            port: Some(22),
        },
        remote_path: format!("/workspace/{id}"),
        url: format!("zed://ssh/fixture.example.test/workspace/{id}"),
        source,
        last_opened_at_ms: None,
        is_current: source == ZedRemoteProjectSource::CurrentThread,
    }
}
