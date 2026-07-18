use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_core::relay_config::{CodexContextEntries, RelayStatus};
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, RelayMode, RelayProfile,
};
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::provider::ProviderViewState;
use codex_plus_manager_native::state::{OverviewPhase, Route};
use codex_plus_manager_native::views::shell::ShellViewModel;
use codex_plus_manager_service::{
    LocatedResource, OverviewSnapshot, ProviderActivationSummary, ProviderDocument, ProviderKind,
    ProviderLiveFiles, ProviderLiveRevision, ProviderLiveWorkspace, ProviderProfile,
    ProviderRevision, ProviderWorkspace, ResourcePresence, ShortcutSnapshot, UpdateCheckState,
};

pub fn snapshot(codex_version: &str) -> Arc<OverviewSnapshot> {
    Arc::new(OverviewSnapshot {
        codex_app: LocatedResource {
            presence: ResourcePresence::Found,
            path: Some(PathBuf::from("C:/Program Files/Codex")),
        },
        codex_version: Some(codex_version.to_owned()),
        silent_shortcut: ShortcutSnapshot {
            installed: true,
            path: Some(PathBuf::from("C:/Users/Test/Desktop/Codex++.lnk")),
        },
        management_shortcut: ShortcutSnapshot {
            installed: true,
            path: Some(PathBuf::from("C:/Users/Test/Desktop/Codex++ Manager.lnk")),
        },
        latest_launch: Some(codex_plus_core::status::LaunchStatus {
            status: "running".to_owned(),
            message: "ready".to_owned(),
            started_at_ms: 42,
            debug_port: Some(9229),
            helper_port: Some(57321),
            codex_app: Some("C:/Program Files/Codex".to_owned()),
        }),
        current_version: "1.2.36".to_owned(),
        update_status: UpdateCheckState::NotChecked,
        settings_path: PathBuf::from("C:/Users/Test/AppData/Roaming/Codex++/settings.json"),
        logs_path: PathBuf::from("C:/Users/Test/AppData/Roaming/Codex++/diagnostic.log"),
    })
}

#[allow(dead_code)]
pub fn provider_state() -> ProviderViewState {
    let aggregate_id = "aggregate-local".to_owned();
    let workspace = ProviderWorkspace {
        revision: ProviderRevision::parse("c".repeat(64)).unwrap(),
        document: ProviderDocument {
            profiles: vec![
                ProviderProfile::Ordinary(RelayProfile {
                    id: "local-api".to_owned(),
                    name: "Local API".to_owned(),
                    relay_mode: RelayMode::PureApi,
                    upstream_base_url: "https://api.example.test/v1".to_owned(),
                    model: "model-alpha".to_owned(),
                    test_model: "model-alpha".to_owned(),
                    model_list: "model-alpha\nmodel-beta".to_owned(),
                    model_windows: r#"{"model-alpha":"1M"}"#.to_owned(),
                    ..RelayProfile::default()
                }),
                ProviderProfile::Ordinary(RelayProfile {
                    id: "backup-api".to_owned(),
                    name: "Backup API".to_owned(),
                    relay_mode: RelayMode::MixedApi,
                    model: "model-beta".to_owned(),
                    ..RelayProfile::default()
                }),
                ProviderProfile::Aggregate {
                    shell: RelayProfile {
                        id: aggregate_id.clone(),
                        name: "Resilient pool".to_owned(),
                        relay_mode: RelayMode::Aggregate,
                        ..RelayProfile::default()
                    },
                    routing: AggregateRelayProfile {
                        id: aggregate_id,
                        name: "Resilient pool".to_owned(),
                        strategy: Default::default(),
                        members: vec![AggregateRelayMember {
                            relay_id: "local-api".to_owned(),
                            weight: 1,
                        }],
                    },
                },
            ],
            common_config_contents: String::new(),
            context_config_contents: String::new(),
            default_test_model: "model-alpha".to_owned(),
        },
        activation: ProviderActivationSummary {
            enabled: true,
            active_profile_id: Some("local-api".to_owned()),
            active_profile_kind: Some(ProviderKind::Ordinary),
        },
        context_options: CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
    };
    let mut state = ProviderViewState::default();
    let request_id = state.begin_live_load().unwrap();
    assert!(state.apply_live_load_response(
        request_id,
        Ok(Arc::new(ProviderLiveWorkspace {
            provider: workspace,
            status: RelayStatus {
                authenticated: true,
                auth_source: "fixture".to_owned(),
                account_label: None,
                config_path: "C:/Fixtures/Codex/config.toml".to_owned(),
                configured: true,
                requires_openai_auth: true,
                has_bearer_token: true,
            },
            files: ProviderLiveFiles {
                config_path: "C:/Fixtures/Codex/config.toml".to_owned(),
                auth_path: "C:/Fixtures/Codex/auth.json".to_owned(),
                config_exists: true,
                auth_exists: true,
                config_contents: "model = \"model-alpha\"\n".to_owned(),
                auth_contents: "{}\n".to_owned(),
            },
            revision: ProviderLiveRevision::parse("d".repeat(64)).unwrap(),
        })),
    ));
    state
}

#[allow(dead_code)]
pub fn model(locale: Locale, theme: ThemeMode) -> ShellViewModel {
    ShellViewModel {
        route: Route::Overview,
        locale,
        theme,
        overview_phase: OverviewPhase::Ready,
        overview_snapshot: Some(snapshot("0.16.0")),
        overview_error: None,
        last_updated: Some("12:34:56 UTC".to_owned()),
        renderer: "WGPU".to_owned(),
    }
}
