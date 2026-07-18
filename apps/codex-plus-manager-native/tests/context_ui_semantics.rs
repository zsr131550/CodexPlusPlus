use std::sync::Arc;

use codex_plus_core::context_ownership::ContextOwnershipRevision;
use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::context::{ContextFailureKind, ContextViewState};
use codex_plus_manager_native::state::{OverviewPhase, Route};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::context::{self, ContextAction};
use codex_plus_manager_native::views::shell::{ShellViewModel, render_shell};
use codex_plus_manager_service::{
    ContextBundle, ContextEntryDraft, ContextEntryKey, ContextEntryLiveState, ContextEntrySummary,
    ContextKind, ContextOwnershipOutcome, ContextSyncDiffSummary, ContextSyncGuard,
    ContextSyncKeys, ContextSyncOutcome, ContextSyncPreview, ContextToolsErrorKind,
    ContextWorkspace, ProviderActivationSummary, ProviderDocument, ProviderLiveRevision,
    ProviderRevision, ProviderWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

const SECRET: &str = "context-ui-secret-sentinel";

#[test]
fn context_action_debug_redacts_editor_body() {
    let action = ContextAction::SetEditorBody(format!("token = \"{SECRET}\""));

    let debug = format!("{action:?}");

    assert!(debug.contains("SetEditorBody"));
    assert!(!debug.contains(SECRET));
    assert!(!debug.contains("token"));
}

fn revision(character: char) -> ProviderRevision {
    ProviderRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn live_revision(character: char) -> ProviderLiveRevision {
    ProviderLiveRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn ownership_revision(character: char) -> ContextOwnershipRevision {
    ContextOwnershipRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_owned(),
    }
}

fn bundle() -> Arc<ContextBundle> {
    Arc::new(ContextBundle {
        context: ContextWorkspace {
            provider_revision: revision('a'),
            live_revision: live_revision('b'),
            ownership_revision: ownership_revision('c'),
            active_provider_id: Some("relay-a".to_owned()),
            active_provider_name: Some("Relay A".to_owned()),
            entries: vec![
                ContextEntrySummary {
                    key: key(ContextKind::Mcp, "alpha"),
                    display_name: "alpha".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::StoredOnly,
                },
                ContextEntrySummary {
                    key: key(ContextKind::Mcp, "beta-with-a-very-long-context-entry-id"),
                    display_name: "beta-with-a-very-long-context-entry-id".to_owned(),
                    enabled: false,
                    live_state: ContextEntryLiveState::Different,
                },
                ContextEntrySummary {
                    key: key(ContextKind::Skill, "review"),
                    display_name: "review".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::Matching,
                },
                ContextEntrySummary {
                    key: key(ContextKind::Plugin, "old-plugin"),
                    display_name: "old-plugin".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::PendingRemoval,
                },
            ],
            unmanaged_live_count: 2,
            sync_needed: true,
        },
        provider: ProviderWorkspace {
            revision: revision('a'),
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: true,
                active_profile_id: Some("relay-a".to_owned()),
                active_profile_kind: None,
            },
            context_options: CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    })
}

fn loaded_state() -> ContextViewState {
    let mut state = ContextViewState::default();
    let request_id = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(request_id, Ok(bundle())));
    state
}

struct ViewState {
    context: ContextViewState,
    locale: Locale,
    provider_dirty: bool,
    emitted: Vec<ContextAction>,
}

fn render_context(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    context::render(
        ui,
        &state.context,
        state.provider_dirty,
        state.locale,
        &mut actions,
    );
    state.emitted.extend(actions);
}

fn harness(state: ViewState) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 620.0))
        .build_ui_state(render_context, state)
}

fn model(locale: Locale) -> ShellViewModel {
    ShellViewModel {
        route: Route::Context,
        locale,
        theme: ThemeMode::Dark,
        overview_phase: OverviewPhase::Ready,
        overview_snapshot: Some(common::snapshot("0.16.0")),
        overview_error: None,
        last_updated: Some("12:34:56 UTC".to_owned()),
        renderer: "WGPU".to_owned(),
    }
}

#[test]
fn tools_route_has_exact_bilingual_navigation_and_header_copy() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            ["工具与插件", "Codex++ 工具与插件", "管理 MCP、技能与插件"],
        ),
        (
            Locale::En,
            [
                "Tools and Plugins",
                "Codex++ Tools and Plugins",
                "Manage MCP servers, skills, and plugins",
            ],
        ),
    ] {
        let mut shell_model = model(locale);
        let harness = Harness::builder()
            .with_size(egui::vec2(960.0, 720.0))
            .build_ui(move |ui| {
                egui_extras::install_image_loaders(ui.ctx());
                theme::apply(ui.ctx(), ThemeMode::Dark);
                let _ = render_shell(ui, &shell_model, None, None, None, None, None);
                shell_model.route = Route::Context;
            });
        for label in labels {
            assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn rows_have_fixed_safe_semantics_and_never_expose_toml() {
    let harness = harness(ViewState {
        context: loaded_state(),
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });

    for label in [
        "Active provider: Relay A",
        "MCP (2)",
        "Skills (1)",
        "Plugins (1)",
        "alpha",
        "Stored only",
        "Disable MCP alpha",
        "Enable MCP beta-with-a-very-long-context-entry-id",
        "Edit MCP alpha",
        "Delete MCP alpha",
    ] {
        let rect = harness.get_by_label(label).rect();
        assert!(rect.is_positive(), "{label}");
        assert!(
            rect.max.x <= 760.0 && rect.max.y <= 620.0,
            "{label}: {rect:?}"
        );
    }
    assert!(
        harness
            .query_by(|node| {
                node.label().is_some_and(|label| label.contains(SECRET))
                    || node.value().is_some_and(|value| value.contains(SECRET))
            })
            .is_none()
    );
}

#[test]
fn core_workbench_controls_are_bilingual() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "当前供应商: Relay A",
                "MCP (2)",
                "技能 (1)",
                "插件 (1)",
                "需要同步实时配置",
                "同步到 Codex",
                "新建 MCP 条目",
            ],
        ),
        (
            Locale::En,
            [
                "Active provider: Relay A",
                "MCP (2)",
                "Skills (1)",
                "Plugins (1)",
                "Live sync needed",
                "Sync to Codex",
                "Create MCP entry",
            ],
        ),
    ] {
        let harness = harness(ViewState {
            context: loaded_state(),
            locale,
            provider_dirty: false,
            emitted: Vec::new(),
        });
        for label in labels {
            assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
        }
    }
}

#[test]
fn row_and_header_controls_emit_typed_actions() {
    let mut harness = harness(ViewState {
        context: loaded_state(),
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });
    for label in [
        "Plugins (1)",
        "Disable MCP alpha",
        "Edit MCP alpha",
        "Delete MCP alpha",
        "Create MCP entry",
        "Sync to Codex",
    ] {
        harness.get_by_label(label).click();
        harness.run();
    }
    let emitted = &harness.state().emitted;
    assert!(emitted.contains(&ContextAction::SelectKind(ContextKind::Plugin)));
    assert!(emitted.iter().any(|action| matches!(
        action,
        ContextAction::SetEnabled { key: entry_key, enabled: false }
            if entry_key == &key(ContextKind::Mcp, "alpha")
    )));
    assert!(emitted.contains(&ContextAction::OpenEdit(key(ContextKind::Mcp, "alpha"))));
    assert!(emitted.contains(&ContextAction::RequestDelete(key(
        ContextKind::Mcp,
        "alpha"
    ))));
    assert!(emitted.contains(&ContextAction::OpenCreate(ContextKind::Mcp)));
    assert!(emitted.contains(&ContextAction::PreviewSync));
}

#[test]
fn dirty_provider_disables_all_context_mutations() {
    let harness = harness(ViewState {
        context: loaded_state(),
        locale: Locale::En,
        provider_dirty: true,
        emitted: Vec::new(),
    });
    for label in [
        "Sync to Codex",
        "Disable MCP alpha",
        "Edit MCP alpha",
        "Delete MCP alpha",
        "Create MCP entry",
    ] {
        assert!(
            harness
                .query_by(|node| node.label().as_deref() == Some(label) && node.is_disabled())
                .is_some(),
            "{label}"
        );
    }
}

#[test]
fn editor_masks_toml_and_keeps_edit_id_immutable() {
    let mut context = loaded_state();
    let target = key(ContextKind::Mcp, "alpha");
    let (request_id, _) = context.begin_edit(target.clone()).unwrap();
    context.apply_draft_response(
        request_id,
        Ok(Arc::new(ContextEntryDraft {
            provider_revision: revision('a'),
            key: target,
            toml_body: format!("token = \"{SECRET}\"\n"),
        })),
    );
    let harness = harness(ViewState {
        context,
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });

    for label in [
        "Edit MCP entry",
        "Context ID",
        "TOML body",
        "Reveal TOML",
        "Save entry",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Context ID")
                    && node.value().is_some()
                    && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("TOML body")
                    && node.value().as_deref() == Some("********")
                    && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| node.value().is_some_and(|value| value.contains(SECRET)))
            .is_none()
    );
}

#[test]
fn delete_confirmation_repeats_exact_kind_and_id() {
    let mut context = loaded_state();
    context.request_delete(key(ContextKind::Plugin, "old-plugin"));
    let harness = harness(ViewState {
        context,
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });

    for label in [
        "Delete context entry",
        "Plugin / old-plugin",
        "Confirm delete",
    ] {
        assert!(harness.get_by_label(label).rect().is_positive(), "{label}");
    }
}

#[test]
fn sync_preview_is_metadata_only_and_partial_result_is_explicit() {
    let mut context = loaded_state();
    let guard = ContextSyncGuard {
        expected_provider_revision: revision('a'),
        expected_live_revision: live_revision('b'),
        expected_ownership_revision: ownership_revision('c'),
    };
    let (preview_id, _) = context.begin_preview().unwrap();
    context.apply_preview_response(
        preview_id,
        Ok(Arc::new(ContextSyncPreview {
            guard,
            active_provider_id: Some("relay-a".to_owned()),
            diff: ContextSyncDiffSummary {
                added: 1,
                updated: 2,
                removed: 3,
                unchanged: 4,
            },
            keys: ContextSyncKeys {
                added: vec![key(ContextKind::Mcp, "gamma")],
                updated: vec![
                    key(ContextKind::Skill, "review"),
                    key(ContextKind::Plugin, "old-plugin"),
                ],
                removed: vec![
                    key(ContextKind::Mcp, "alpha"),
                    key(ContextKind::Mcp, "beta-with-a-very-long-context-entry-id"),
                    key(ContextKind::Plugin, "legacy"),
                ],
                unchanged: vec![
                    key(ContextKind::Mcp, "manual"),
                    key(ContextKind::Skill, "format"),
                    key(ContextKind::Skill, "lint"),
                    key(ContextKind::Plugin, "browser"),
                ],
            },
        })),
    );
    let preview_harness = harness(ViewState {
        context,
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });
    for label in [
        "Preview live sync",
        "Added: 1",
        "Updated: 2",
        "Removed: 3",
        "Unchanged: 4",
        "Active provider: relay-a",
        "MCP / gamma",
        "Skill / review",
        "Plugin / legacy",
        "Plugin / browser",
        "Confirm sync",
    ] {
        assert!(
            preview_harness.get_by_label(label).rect().is_positive(),
            "{label}"
        );
    }

    let mut context = loaded_state();
    let (preview_id, _) = context.begin_preview().unwrap();
    context.apply_preview_response(
        preview_id,
        Ok(Arc::new(ContextSyncPreview {
            guard: ContextSyncGuard {
                expected_provider_revision: revision('a'),
                expected_live_revision: live_revision('b'),
                expected_ownership_revision: ownership_revision('c'),
            },
            active_provider_id: Some("relay-a".to_owned()),
            diff: ContextSyncDiffSummary::default(),
            keys: ContextSyncKeys::default(),
        })),
    );
    let (sync_id, _) = context.begin_sync().unwrap();
    context.apply_sync_response(
        sync_id,
        Ok(Arc::new(ContextSyncOutcome {
            bundle: (*bundle()).clone(),
            backup_path: Some("C:/fixture/context-backup.toml".to_owned()),
            ownership: ContextOwnershipOutcome::PartialFailure,
            diff: ContextSyncDiffSummary::default(),
        })),
    );
    let outcome_harness = harness(ViewState {
        context,
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });
    for label in [
        "Live settings updated; ownership metadata needs repair.",
        "C:/fixture/context-backup.toml",
    ] {
        assert!(
            outcome_harness.get_by_label(label).rect().is_positive(),
            "{label}"
        );
    }
}

#[test]
fn stable_context_errors_have_specific_copy() {
    let cases = [
        (
            ContextToolsErrorKind::InvalidToml,
            "The TOML body is invalid.",
        ),
        (
            ContextToolsErrorKind::ProviderConflict,
            "Provider settings changed. Refresh context tools.",
        ),
        (
            ContextToolsErrorKind::LiveConflict,
            "Live settings changed. Preview the sync again.",
        ),
        (
            ContextToolsErrorKind::OwnershipConflict,
            "Ownership metadata changed. Preview the sync again.",
        ),
    ];
    for (kind, expected) in cases {
        let mut context = loaded_state();
        assert!(context.open_create(ContextKind::Mcp));
        context.set_editor_id("new-entry".to_owned());
        let (request_id, _) = context.begin_save().unwrap();
        context.apply_stored_mutation_response(request_id, Err(ContextFailureKind::Service(kind)));
        let harness = harness(ViewState {
            context,
            locale: Locale::En,
            provider_dirty: false,
            emitted: Vec::new(),
        });
        assert!(
            harness.get_by_label(expected).rect().is_positive(),
            "{expected}"
        );
    }

    let mut context = ContextViewState::default();
    let request_id = context.begin_workspace_refresh();
    context.apply_workspace_response(request_id, Err(ContextFailureKind::WorkerStopped));
    let harness = harness(ViewState {
        context,
        locale: Locale::En,
        provider_dirty: false,
        emitted: Vec::new(),
    });
    assert!(
        harness
            .get_by_label("The context tools worker has stopped.")
            .rect()
            .is_positive()
    );
}
