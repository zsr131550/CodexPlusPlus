use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_core::install::{EntryPointState, ShortcutState};
use codex_plus_core::relay_config::{CodexContextEntries, RelayStatus};
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, RelayMode, RelayProfile,
};
use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::provider::ProviderViewState;
use codex_plus_manager_native::state::{OverviewPhase, Route};
use codex_plus_manager_native::views::shell::ShellViewModel;
use codex_plus_manager_service::{
    CodexLaunchPlan, DiagnosticPathPresence, LocatedResource, MaintenanceEnvironment,
    MaintenanceService, MaintenanceWorkspace, ManagerSettingsEnvironment, ManagerSettingsService,
    ManagerSettingsWorkspace, OverviewSnapshot, PathKind, ProviderActivationSummary,
    ProviderDocument, ProviderKind, ProviderLiveFiles, ProviderLiveRevision, ProviderLiveWorkspace,
    ProviderProfile, ProviderRevision, ProviderWorkspace, ResourcePresence, ShortcutSnapshot,
    StepwiseTestFailure, UpdateCheckState,
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

#[derive(Clone)]
struct FixtureMaintenanceEnvironment {
    settings: codex_plus_core::settings::BackendSettings,
    log_tail: Vec<u8>,
}

impl MaintenanceEnvironment for FixtureMaintenanceEnvironment {
    fn load_maintenance_settings(
        &self,
    ) -> anyhow::Result<codex_plus_core::settings::BackendSettings> {
        Ok(self.settings.clone())
    }

    fn update_maintenance_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<codex_plus_core::settings::BackendSettings>>
    where
        F: FnOnce(&codex_plus_core::settings::BackendSettings) -> bool,
    {
        Ok(predicate(&self.settings).then(|| self.settings.clone()))
    }

    fn inspect_path(&self, _path: &std::path::Path) -> anyhow::Result<PathKind> {
        Ok(PathKind::File)
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        (!saved.is_empty()).then(|| PathBuf::from(saved))
    }

    fn codex_app_version(&self, _path: &std::path::Path) -> Option<String> {
        Some("fixture-codex-1.0".to_owned())
    }

    fn inspect_entrypoints(&self) -> anyhow::Result<EntryPointState> {
        Ok(EntryPointState {
            silent_shortcut: ShortcutState {
                installed: true,
                path: None,
            },
            management_shortcut: ShortcutState {
                installed: false,
                path: None,
            },
        })
    }

    fn watcher_disabled(&self) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn load_latest_launch(&self) -> anyhow::Result<Option<codex_plus_core::status::LaunchStatus>> {
        Ok(Some(codex_plus_core::status::LaunchStatus {
            status: "ready".to_owned(),
            message: "private-message-sentinel".to_owned(),
            started_at_ms: 42,
            debug_port: Some(9229),
            helper_port: Some(57321),
            codex_app: Some("C:/private/status-path-sentinel".to_owned()),
        }))
    }

    fn read_diagnostic_tail(&self, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        let start = self.log_tail.len().saturating_sub(max_bytes);
        Ok(self.log_tail[start..].to_vec())
    }

    fn diagnostic_path_presence(&self) -> DiagnosticPathPresence {
        DiagnosticPathPresence {
            settings: true,
            logs: true,
            latest_status: true,
        }
    }

    fn launch_codex(&self, _plan: &CodexLaunchPlan) -> anyhow::Result<()> {
        Ok(())
    }
}

impl ManagerSettingsEnvironment for FixtureMaintenanceEnvironment {
    fn load_manager_settings(&self) -> anyhow::Result<codex_plus_core::settings::BackendSettings> {
        Ok(self.settings.clone())
    }

    fn update_manager_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<codex_plus_core::settings::BackendSettings>>
    where
        F: FnOnce(&codex_plus_core::settings::BackendSettings) -> bool,
    {
        Ok(predicate(&self.settings).then(|| self.settings.clone()))
    }

    fn inspect_path(&self, _path: &std::path::Path) -> anyhow::Result<PathKind> {
        Ok(PathKind::File)
    }

    fn environment_value_present(&self, name: &str) -> bool {
        name == "CODEX_PLUS_FIXTURE_KEY"
    }

    fn test_stepwise_candidate(
        &self,
        _settings: &codex_plus_core::settings::BackendSettings,
    ) -> Result<usize, StepwiseTestFailure> {
        Ok(3)
    }
}

#[allow(dead_code)]
pub fn maintenance_workspace(path: &str) -> Arc<MaintenanceWorkspace> {
    let settings = codex_plus_core::settings::BackendSettings {
        codex_app_path: path.to_owned(),
        codex_app_stepwise_base_url: "https://private.invalid/body-sentinel".to_owned(),
        codex_app_stepwise_api_key: "private-key-sentinel".to_owned(),
        codex_app_image_overlay_path: "C:/private/overlay-sentinel.png".to_owned(),
        ..codex_plus_core::settings::BackendSettings::default()
    };
    let log_tail = format!(
        "{}\n",
        serde_json::json!({
            "timestamp_ms": 1,
            "event": "native.maintenance.load",
            "detail": {
                "request_id": 7,
                "path": path,
                "key": "private-key-sentinel",
                "body": "body-sentinel"
            }
        })
    )
    .into_bytes();
    Arc::new(
        MaintenanceService::new(FixtureMaintenanceEnvironment { settings, log_tail })
            .load_workspace(codex_plus_manager_service::LoadMaintenance { log_lines: 100 })
            .unwrap(),
    )
}

#[allow(dead_code)]
pub fn manager_settings_workspace(seed: u8) -> Arc<ManagerSettingsWorkspace> {
    let settings = codex_plus_core::settings::BackendSettings {
        codex_app_stepwise_enabled: true,
        codex_app_stepwise_direct_send: seed.is_multiple_of(2),
        codex_app_stepwise_base_url: format!("https://private-{seed}.invalid/body-sentinel"),
        codex_app_stepwise_api_key: "private-key-sentinel".to_owned(),
        codex_app_stepwise_api_key_env: "CODEX_PLUS_FIXTURE_KEY".to_owned(),
        codex_app_stepwise_model: format!("fixture-model-{seed}"),
        codex_app_stepwise_max_items: seed.min(6),
        codex_app_stepwise_max_input_chars: 8_000 + u32::from(seed),
        codex_app_stepwise_max_output_tokens: 1_000 + u32::from(seed),
        codex_app_stepwise_timeout_ms: 20_000 + u64::from(seed),
        codex_app_image_overlay_enabled: true,
        codex_app_image_overlay_path: format!("C:/private/overlay-{seed}.png"),
        codex_app_image_overlay_opacity: 60 + seed.min(30),
        codex_app_image_overlay_fit_mode: "fit".to_owned(),
        codex_extra_args: vec![format!("--fixture-{seed}"), "--safe-mode".to_owned()],
        ..codex_plus_core::settings::BackendSettings::default()
    };
    Arc::new(
        ManagerSettingsService::new(FixtureMaintenanceEnvironment {
            settings,
            log_tail: Vec::new(),
        })
        .load_workspace()
        .unwrap(),
    )
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
        update: Default::default(),
    }
}
