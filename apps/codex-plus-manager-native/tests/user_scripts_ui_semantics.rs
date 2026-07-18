use std::sync::Arc;

use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::user_scripts::{ScriptsTab, UserScriptViewState};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::user_scripts::{self, UserScriptAction};
use codex_plus_manager_service::{
    ScriptIntegrity, ScriptMarketRevision, ScriptMarketSummary, ScriptMarketWorkspace,
    UserScriptOrigin, UserScriptRevision, UserScriptStatus, UserScriptSummary, UserScriptWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

struct ViewState {
    scripts: UserScriptViewState,
    locale: Locale,
    emitted: Vec<UserScriptAction>,
}

#[test]
fn scripts_workbench_has_complete_bilingual_market_semantics() {
    for (locale, labels) in [
        (
            Locale::ZhCn,
            [
                "脚本市场",
                "本地脚本",
                "搜索脚本",
                "刷新脚本市场",
                "未验证脚本",
                "更新脚本",
            ],
        ),
        (
            Locale::En,
            [
                "Script market",
                "Local scripts",
                "Search scripts",
                "Refresh script market",
                "Unverified script",
                "Update script",
            ],
        ),
    ] {
        let harness = harness(ViewState {
            scripts: loaded_state(ScriptIntegrity::Unverified),
            locale,
            emitted: Vec::new(),
        });
        for label in labels {
            let rect = harness.get_by_label(label).rect();
            assert!(rect.is_positive(), "{label}");
            assert!(
                rect.max.x <= 960.0 && rect.max.y <= 640.0,
                "{label}: {rect:?}"
            );
        }
    }
}

#[test]
fn local_controls_are_complete_and_builtin_scripts_have_no_delete_action() {
    let mut state = loaded_state(ScriptIntegrity::Verified);
    state.tab = ScriptsTab::Local;
    let local_harness = harness(ViewState {
        scripts: state,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    for label in [
        "Enable all scripts",
        "Refresh local scripts",
        "Base",
        "Custom",
        "Built-in",
        "User",
        "Delete user script",
    ] {
        assert!(
            local_harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
                })
                .count()
                > 0,
            "{label}"
        );
    }
    assert_eq!(
        local_harness
            .query_all_by(|node| node.label().as_deref() == Some("Delete user script"))
            .count(),
        2
    );
}

#[test]
fn unverified_confirmation_is_disabled_until_acknowledged() {
    let mut state = loaded_state(ScriptIntegrity::Unverified);
    assert!(state.request_install("demo"));
    let disabled_harness = harness(ViewState {
        scripts: state,
        locale: Locale::En,
        emitted: Vec::new(),
    });
    for label in [
        "Update script?",
        "Unverified script",
        "I acknowledge this unverified script",
        "Cancel",
    ] {
        assert!(
            disabled_harness
                .query_all_by(|node| {
                    node.label().as_deref() == Some(label) || node.value().as_deref() == Some(label)
                })
                .count()
                > 0,
            "{label}"
        );
    }
    assert!(
        disabled_harness
            .query_by(|node| {
                node.label().as_deref() == Some("Update script") && node.is_disabled()
            })
            .is_some()
    );
    let title = disabled_harness.get_by_label("Update script?").rect();
    assert!(
        title.height() < 260.0,
        "confirmation is not compact: {title:?}"
    );

    let mut acknowledged = loaded_state(ScriptIntegrity::Unverified);
    acknowledged.request_install("demo");
    acknowledged.set_unverified_acknowledgement(true);
    let enabled_harness = harness(ViewState {
        scripts: acknowledged,
        locale: Locale::En,
        emitted: Vec::new(),
    });
    assert!(
        enabled_harness
            .query_all_by(|node| {
                node.label().as_deref() == Some("Update script") && !node.is_disabled()
            })
            .count()
            > 0
    );
}

#[test]
fn row_and_header_controls_emit_typed_actions() {
    let mut harness = harness(ViewState {
        scripts: loaded_state(ScriptIntegrity::Verified),
        locale: Locale::En,
        emitted: Vec::new(),
    });

    harness.get_by_label("Refresh script market").click();
    harness.get_by_label("Update script").click();
    harness.run();
    assert!(
        harness
            .state()
            .emitted
            .contains(&UserScriptAction::RefreshMarket)
    );
    assert!(
        harness
            .state()
            .emitted
            .contains(&UserScriptAction::RequestInstall("demo".to_string()))
    );
}

#[test]
fn compact_market_columns_never_overlap_the_action_button() {
    let harness = harness_at(
        ViewState {
            scripts: loaded_state(ScriptIntegrity::Unverified),
            locale: Locale::En,
            emitted: Vec::new(),
        },
        egui::vec2(752.0, 640.0),
    );

    let integrity = harness.get_by_label("Unverified script").rect();
    let action = harness.get_by_label("Update script").rect();
    let second_action = harness.get_by_label("Install script").rect();
    assert!(
        integrity.max.x <= action.min.x,
        "integrity and action overlap: {integrity:?}, {action:?}"
    );
    assert!(
        (action.max.x - second_action.max.x).abs() <= 1.0,
        "action column shifted: {action:?}, {second_action:?}"
    );
}

#[test]
fn compact_local_origin_column_stays_aligned_for_short_names() {
    let mut state = loaded_state(ScriptIntegrity::Verified);
    state.tab = ScriptsTab::Local;
    let harness = harness_at(
        ViewState {
            scripts: state,
            locale: Locale::En,
            emitted: Vec::new(),
        },
        egui::vec2(752.0, 640.0),
    );

    let builtin = harness.get_by_label("Built-in").rect();
    let user = harness.query_all_by_label("User").next().unwrap().rect();
    assert!(
        (builtin.min.x - user.min.x).abs() <= 1.0,
        "origin column shifted with name length: {builtin:?}, {user:?}"
    );
}

#[test]
fn current_market_version_is_labeled_installed_instead_of_update() {
    let mut state = loaded_state(ScriptIntegrity::Verified);
    Arc::make_mut(state.local.as_mut().unwrap())
        .scripts
        .push(UserScriptSummary {
            key: "user:new.js".to_string(),
            name: "New".to_string(),
            origin: UserScriptOrigin::User,
            enabled: true,
            status: UserScriptStatus::NotLoaded,
            market_id: Some("new".to_string()),
            version: Some("1".to_string()),
        });
    let harness = harness(ViewState {
        scripts: state,
        locale: Locale::En,
        emitted: Vec::new(),
    });

    assert!(
        harness
            .query_by(|node| { node.label().as_deref() == Some("Installed") && node.is_disabled() })
            .is_some()
    );
}

fn harness(state: ViewState) -> Harness<'static, ViewState> {
    harness_at(state, egui::vec2(960.0, 640.0))
}

fn harness_at(state: ViewState, size: egui::Vec2) -> Harness<'static, ViewState> {
    Harness::builder()
        .with_size(size)
        .build_ui_state(render, state)
}

fn render(ui: &mut egui::Ui, state: &mut ViewState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), ThemeMode::Dark);
    let mut actions = Vec::new();
    user_scripts::render(ui, &state.scripts, state.locale, &mut actions);
    state.emitted.extend(actions);
}

fn loaded_state(integrity: ScriptIntegrity) -> UserScriptViewState {
    let mut state = UserScriptViewState::default();
    let local_request = state.begin_local_refresh();
    state.apply_local_response(
        local_request,
        Ok(Arc::new(UserScriptWorkspace {
            revision: UserScriptRevision::from_digest([1; 32]),
            globally_enabled: true,
            scripts: vec![
                UserScriptSummary {
                    key: "builtin:base.js".to_string(),
                    name: "Base".to_string(),
                    origin: UserScriptOrigin::Builtin,
                    enabled: true,
                    status: UserScriptStatus::NotLoaded,
                    market_id: None,
                    version: None,
                },
                UserScriptSummary {
                    key: "user:custom.js".to_string(),
                    name: "Custom".to_string(),
                    origin: UserScriptOrigin::User,
                    enabled: false,
                    status: UserScriptStatus::Disabled,
                    market_id: None,
                    version: None,
                },
                UserScriptSummary {
                    key: "user:market-demo.js".to_string(),
                    name: "Demo".to_string(),
                    origin: UserScriptOrigin::User,
                    enabled: true,
                    status: UserScriptStatus::NotLoaded,
                    market_id: Some("demo".to_string()),
                    version: Some("1".to_string()),
                },
            ],
        })),
    );
    let market_request = state.begin_market_refresh();
    state.apply_market_response(
        market_request,
        Ok(Arc::new(ScriptMarketWorkspace {
            revision: ScriptMarketRevision::from_digest([1; 32]),
            updated_at: Some("2026-07-18T00:00:00Z".to_string()),
            entries: vec![
                ScriptMarketSummary {
                    id: "demo".to_string(),
                    name: "Demo with a long metadata title for stable columns".to_string(),
                    description: "Useful script".to_string(),
                    version: "2".to_string(),
                    author: "Fixture".to_string(),
                    tags: vec!["ui".to_string()],
                    source_host: "example.invalid".to_string(),
                    integrity,
                    installed_version: Some("1".to_string()),
                    update_available: true,
                },
                ScriptMarketSummary {
                    id: "new".to_string(),
                    name: "New".to_string(),
                    description: "Short row".to_string(),
                    version: "1".to_string(),
                    author: "Fixture".to_string(),
                    tags: vec!["ui".to_string()],
                    source_host: "example.invalid".to_string(),
                    integrity: ScriptIntegrity::Verified,
                    installed_version: None,
                    update_available: false,
                },
            ],
        })),
    );
    state
}
