use std::sync::Arc;

use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, RelayMode, RelayProfile,
};
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::provider::{
    ProviderLoadFailureKind, ProviderSaveFailureKind, ProviderViewState, TransitionResult,
};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::provider::ProviderAction;
use codex_plus_manager_native::views::shell::{ShellAction, ShellViewModel, render_shell};
use codex_plus_manager_service::{
    ProviderActivationSummary, ProviderDocument, ProviderKind, ProviderProfile, ProviderRevision,
    ProviderWorkspace,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

const SECRET: &str = "provider-ui-secret-sentinel";

struct ProviderHarnessState {
    model: ShellViewModel,
    provider: ProviderViewState,
    emitted: Vec<ShellAction>,
}

fn ordinary(id: &str, name: &str) -> ProviderProfile {
    ProviderProfile::Ordinary(RelayProfile {
        id: id.to_owned(),
        name: name.to_owned(),
        relay_mode: RelayMode::PureApi,
        upstream_base_url: "https://api.example.test/v1".to_owned(),
        api_key: SECRET.to_owned(),
        config_contents: format!("secret = \"{SECRET}\""),
        auth_contents: format!("{{\"OPENAI_API_KEY\":\"{SECRET}\"}}"),
        model: "model-a".to_owned(),
        model_list: "model-a\nmodel-b".to_owned(),
        ..RelayProfile::default()
    })
}

fn workspace() -> ProviderWorkspace {
    let aggregate_id = "aggregate-a".to_owned();
    ProviderWorkspace {
        revision: ProviderRevision::parse("a".repeat(64)).unwrap(),
        document: ProviderDocument {
            profiles: vec![
                ordinary("active", "Active provider"),
                ordinary("secondary", "Secondary provider"),
                ProviderProfile::Aggregate {
                    shell: RelayProfile {
                        id: aggregate_id.clone(),
                        name: "Aggregate pool".to_owned(),
                        relay_mode: RelayMode::Aggregate,
                        ..RelayProfile::default()
                    },
                    routing: AggregateRelayProfile {
                        id: aggregate_id,
                        name: "Aggregate pool".to_owned(),
                        strategy: Default::default(),
                        members: vec![AggregateRelayMember {
                            relay_id: "active".to_owned(),
                            weight: 1,
                        }],
                    },
                },
            ],
            common_config_contents: String::new(),
            context_config_contents: String::new(),
            default_test_model: "model-a".to_owned(),
        },
        activation: ProviderActivationSummary {
            enabled: true,
            active_profile_id: Some("active".to_owned()),
            active_profile_kind: Some(ProviderKind::Ordinary),
        },
        context_options: CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
    }
}

fn loaded_provider() -> ProviderViewState {
    let mut state = ProviderViewState::default();
    let request_id = state.begin_load();
    assert!(state.apply_load_response(request_id, Ok(Arc::new(workspace()))));
    state
}

fn apply_provider_action(state: &mut ProviderViewState, action: &ProviderAction) {
    match action {
        ProviderAction::Select(profile_id) => {
            assert_ne!(
                state.request_selection(profile_id),
                TransitionResult::NotFound
            );
        }
        ProviderAction::SetTab(tab) => state.editor_tab = *tab,
        ProviderAction::ToggleList => state.list_collapsed = !state.list_collapsed,
        _ => {}
    }
}

fn render(ui: &mut egui::Ui, state: &mut ProviderHarnessState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    for action in render_shell(ui, &state.model, Some(&state.provider)) {
        if let ShellAction::Navigate(route) = &action {
            state.model.route = *route;
        }
        if let ShellAction::Provider(provider_action) = &action {
            apply_provider_action(&mut state.provider, provider_action);
        }
        state.emitted.push(action);
        ui.ctx().request_repaint();
    }
}

fn harness(
    size: [f32; 2],
    route: Route,
    provider: ProviderViewState,
) -> Harness<'static, ProviderHarnessState> {
    let mut model = common::model(Locale::En, ThemeMode::Dark);
    model.route = route;
    Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_ui_state(
            render,
            ProviderHarnessState {
                model,
                provider,
                emitted: Vec::new(),
            },
        )
}

#[test]
fn provider_navigation_opens_the_native_workspace() {
    let mut harness = harness([1180.0, 820.0], Route::Overview, loaded_provider());

    harness.get_by_label("Providers").click();
    harness.run();

    assert_eq!(harness.state().model.route, Route::Providers);
    assert!(harness.get_by_label("Provider list").rect().is_positive());
    assert!(harness.get_by_label("Provider editor").rect().is_positive());
}

#[test]
fn provider_workspace_keeps_master_detail_and_save_bar_visible_at_supported_sizes() {
    for size in [[1180.0, 820.0], [960.0, 720.0]] {
        let harness = harness(size, Route::Providers, loaded_provider());
        let list = harness.get_by_label("Provider list").rect();
        let editor = harness.get_by_label("Provider editor").rect();
        assert!(
            list.max.x < editor.min.x,
            "overlapping panes at {size:?}: {list:?} {editor:?}"
        );
        for label in [
            "Collapse provider list",
            "Add provider",
            "Add aggregate",
            "General",
            "Models",
            "Config",
            "Diagnostics",
            "Save changes",
            "Discard changes",
        ] {
            let rect = harness.get_by_label(label).rect();
            assert!(rect.is_positive(), "missing {label} at {size:?}");
            assert!(
                rect.max.x <= size[0] && rect.max.y <= size[1],
                "clipped {label}: {rect:?}"
            );
        }
    }
}

#[test]
fn active_delete_is_disabled_and_default_semantics_never_expose_secrets() {
    let harness = harness([1180.0, 820.0], Route::Providers, loaded_provider());

    assert!(
        harness
            .query_by(|node| {
                node.label().as_deref() == Some("Delete provider") && node.is_disabled()
            })
            .is_some()
    );
    assert!(
        harness
            .query_by(|node| {
                node.label().is_some_and(|label| label.contains(SECRET))
                    || node.value().is_some_and(|value| value.contains(SECRET))
                    || node
                        .description()
                        .is_some_and(|description| description.contains(SECRET))
            })
            .is_none()
    );
}

#[test]
fn aggregate_selection_switches_to_routing_controls() {
    let mut harness = harness([1180.0, 820.0], Route::Providers, loaded_provider());

    harness.get_by_label("Aggregate pool").click();
    harness.run();

    assert!(harness.get_by_label("Routing").rect().is_positive());
    assert!(harness.get_by_label("Failover").rect().is_positive());
    assert!(harness.get_by_label("Active provider").rect().is_positive());
}

#[test]
fn provider_load_error_has_a_retry_control() {
    let mut provider = ProviderViewState::default();
    let request_id = provider.begin_load();
    assert!(provider.apply_load_response(request_id, Err(ProviderLoadFailureKind::LoadFailed)));
    let harness = harness([960.0, 720.0], Route::Providers, provider);

    assert!(
        harness
            .get_by_label("Unable to load providers.")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .get_by_label("Retry provider load")
            .rect()
            .is_positive()
    );
}

#[test]
fn save_conflict_is_visible_without_discarding_the_draft() {
    let mut provider = loaded_provider();
    assert!(provider.edit_selected(|profile| match profile {
        ProviderProfile::Ordinary(profile) => profile.name = "Edited provider".to_owned(),
        ProviderProfile::Aggregate { .. } => unreachable!(),
    }));
    let (request_id, _) = provider.begin_save().unwrap();
    assert!(provider.apply_save_response(request_id, Err(ProviderSaveFailureKind::Conflict),));
    let harness = harness([1180.0, 820.0], Route::Providers, provider);

    assert!(
        harness
            .get_by_label("Provider workspace changed on disk. Reload before saving again.")
            .rect()
            .is_positive()
    );
    assert_eq!(
        harness.state().provider.selected_profile().unwrap().name(),
        "Edited provider"
    );
}
