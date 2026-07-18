use std::path::{Path, PathBuf};

use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, AggregateRelayStrategy, BackendSettings,
    RelayMode, RelayProfile, SettingsStore,
};
use codex_plus_manager_service::{
    ApplyActiveProvider, BackfillActiveProvider, ClearLiveProvider, ProviderActivationError,
    ProviderActivationErrorKind, ProviderActivationSource, ProviderLiveFileKind,
    ProviderLiveWorkspace, ProviderMutationGuard, ProviderMutationOutcome, ProviderRollbackOutcome,
    ProviderService, SaveLiveFile, SwitchProvider, SystemProviderEnvironment,
};

const SECRET: &str = "activation-secret-sentinel";

struct Fixture {
    _temp: tempfile::TempDir,
    settings_path: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let home = temp.path().join("codex");
        std::fs::create_dir(&home).unwrap();
        let settings = BackendSettings {
            active_relay_id: "a".to_string(),
            relay_profiles: vec![
                pure_profile("a", "https://a.example/v1", "sk-a"),
                pure_profile("b", "https://b.example/v1", SECRET),
            ],
            ..BackendSettings::default()
        };
        SettingsStore::new(settings_path.clone())
            .save(&settings)
            .unwrap();
        std::fs::write(
            home.join("config.toml"),
            live_config("https://a.example/v1", "model-a"),
        )
        .unwrap();
        std::fs::write(home.join("auth.json"), r#"{"OPENAI_API_KEY":"sk-a"}"#).unwrap();
        Self {
            _temp: temp,
            settings_path,
            home,
        }
    }

    fn service(&self) -> ProviderService<SystemProviderEnvironment> {
        ProviderService::new(SystemProviderEnvironment::for_paths(
            self.settings_path.clone(),
            self.home.clone(),
        ))
    }

    fn store(&self) -> SettingsStore {
        SettingsStore::new(self.settings_path.clone())
    }
}

fn pure_profile(id: &str, base_url: &str, key: &str) -> RelayProfile {
    RelayProfile {
        id: id.to_string(),
        name: id.to_uppercase(),
        relay_mode: RelayMode::PureApi,
        config_contents: live_config(base_url, &format!("model-{id}")),
        auth_contents: format!(r#"{{"OPENAI_API_KEY":"{key}"}}"#),
        ..RelayProfile::default()
    }
}

fn live_config(base_url: &str, model: &str) -> String {
    format!(
        r#"model = "{model}"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "{base_url}"
"#
    )
}

fn guard(live: &ProviderLiveWorkspace) -> ProviderMutationGuard {
    ProviderMutationGuard {
        expected_provider_revision: live.provider.revision.clone(),
        expected_live_revision: live.revision.clone(),
    }
}

#[test]
fn live_workspace_is_coherent_revisioned_and_secret_safe() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let first = service.load_live_workspace().unwrap();
    let second = service.load_live_workspace().unwrap();

    assert_eq!(first.revision, second.revision);
    assert_eq!(first.provider.revision, second.provider.revision);
    assert_eq!(
        first.provider.activation.active_profile_id.as_deref(),
        Some("a")
    );
    assert!(first.status.configured);
    assert!(first.files.config_exists);
    assert!(first.files.auth_exists);
    assert!(first.files.config_contents.contains("https://a.example/v1"));
    assert!(first.files.auth_contents.contains("sk-a"));
    assert!(!format!("{first:?}").contains("sk-a"));
    assert!(!format!("{first:?}").contains(SECRET));
}

#[test]
fn activation_debug_redacts_raw_paths_and_backup_evidence() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let mut live = service.load_live_workspace().unwrap();
    live.files.config_path = format!("C:/private/{SECRET}/config.toml");
    live.files.auth_path = format!("C:/private/{SECRET}/auth.json");
    let outcome = ProviderMutationOutcome {
        live,
        backup_path: Some(format!("C:/private/{SECRET}/backup")),
        rollback: ProviderRollbackOutcome::Verified,
    };
    let error = ProviderActivationError::for_failure(
        ProviderActivationErrorKind::MutationFailed,
        ProviderRollbackOutcome::Verified,
        Some(format!("C:/private/{SECRET}/backup")),
    );

    for rendered in [
        format!("{outcome:?}"),
        format!("{error:?}"),
        error.to_string(),
    ] {
        assert!(
            !rendered.contains(SECRET),
            "activation diagnostic output leaked secret-bearing data"
        );
    }
}

#[test]
fn switch_backfills_previous_profile_and_returns_backup_evidence() {
    let fixture = Fixture::new();
    let service = fixture.service();
    std::fs::write(
        fixture.home.join("config.toml"),
        live_config("https://edited-a.example/v1", "edited-a"),
    )
    .unwrap();
    std::fs::write(
        fixture.home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"edited-a-key"}"#,
    )
    .unwrap();
    let before = service.load_live_workspace().unwrap();

    let result = service
        .switch_provider(SwitchProvider {
            guard: guard(&before),
            target_profile_id: "b".to_string(),
        })
        .unwrap();

    assert_eq!(
        result.live.provider.activation.active_profile_id.as_deref(),
        Some("b")
    );
    assert!(result.live.status.configured);
    assert!(
        result
            .live
            .files
            .config_contents
            .contains("https://b.example/v1")
    );
    assert_eq!(result.rollback, ProviderRollbackOutcome::NotRequired);
    let backup = result
        .backup_path
        .as_deref()
        .expect("switch should create backup");
    assert!(Path::new(backup).join("config.toml").is_file());
    let stored = fixture.store().load().unwrap();
    let previous = stored
        .relay_profiles
        .iter()
        .find(|profile| profile.id == "a")
        .unwrap();
    assert!(previous.config_contents.contains("edited-a"));
    assert!(previous.auth_contents.contains("edited-a-key"));
}

#[test]
fn stale_provider_or_live_revision_rejects_switch_without_writing() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let stale = service.load_live_workspace().unwrap();
    let edited_live = live_config("https://manual.example/v1", "manual");
    std::fs::write(fixture.home.join("config.toml"), &edited_live).unwrap();

    let live_error = service
        .switch_provider(SwitchProvider {
            guard: guard(&stale),
            target_profile_id: "b".to_string(),
        })
        .unwrap_err();

    assert_eq!(live_error.kind(), ProviderActivationErrorKind::LiveConflict);
    assert_eq!(fixture.store().load().unwrap().active_relay_id, "a");
    assert_eq!(
        std::fs::read_to_string(fixture.home.join("config.toml")).unwrap(),
        edited_live
    );

    let stale_provider = service.load_live_workspace().unwrap();
    fixture
        .store()
        .update(serde_json::json!({"relayTestModel": "new-model"}))
        .unwrap();
    let provider_error = service
        .switch_provider(SwitchProvider {
            guard: guard(&stale_provider),
            target_profile_id: "b".to_string(),
        })
        .unwrap_err();
    assert_eq!(
        provider_error.kind(),
        ProviderActivationErrorKind::ProviderConflict
    );
    assert_eq!(fixture.store().load().unwrap().active_relay_id, "a");
}

#[test]
fn failed_switch_preserves_last_good_state_and_reports_verified_rollback() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.load().unwrap();
    let broken = settings
        .relay_profiles
        .iter_mut()
        .find(|profile| profile.id == "b")
        .unwrap();
    broken.config_contents = "model_provider = \"custom\"\n".to_string();
    broken.base_url.clear();
    broken.upstream_base_url.clear();
    store.save(&settings).unwrap();
    let service = fixture.service();
    let before = service.load_live_workspace().unwrap();
    let original_settings = store.load().unwrap();
    let original_config = std::fs::read(fixture.home.join("config.toml")).unwrap();
    let original_auth = std::fs::read(fixture.home.join("auth.json")).unwrap();

    let error = service
        .switch_provider(SwitchProvider {
            guard: guard(&before),
            target_profile_id: "b".to_string(),
        })
        .unwrap_err();

    assert_eq!(error.kind(), ProviderActivationErrorKind::MutationFailed);
    assert_eq!(error.rollback(), ProviderRollbackOutcome::Verified);
    assert!(error.backup_path().is_some());
    assert!(!format!("{error:?}").contains(SECRET));
    assert!(!error.to_string().contains(SECRET));
    assert_eq!(store.load().unwrap(), original_settings);
    assert_eq!(
        std::fs::read(fixture.home.join("config.toml")).unwrap(),
        original_config
    );
    assert_eq!(
        std::fs::read(fixture.home.join("auth.json")).unwrap(),
        original_auth
    );
}

#[test]
fn apply_clear_and_backfill_are_explicit_revision_guarded_mutations() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let before_apply = service.load_live_workspace().unwrap();
    std::fs::write(
        fixture.home.join("config.toml"),
        live_config("https://temporary.example/v1", "temporary"),
    )
    .unwrap();
    let refreshed = service.load_live_workspace().unwrap();

    let applied = service
        .apply_active_provider(ApplyActiveProvider {
            guard: guard(&refreshed),
        })
        .unwrap();
    assert!(
        applied
            .live
            .files
            .config_contents
            .contains("https://a.example/v1")
    );
    assert!(applied.backup_path.is_some());
    assert_ne!(refreshed.revision, applied.live.revision);
    assert_eq!(before_apply.revision, applied.live.revision);

    std::fs::write(
        fixture.home.join("config.toml"),
        live_config("https://backfill.example/v1", "backfilled"),
    )
    .unwrap();
    std::fs::write(
        fixture.home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"backfilled-key"}"#,
    )
    .unwrap();
    let before_backfill = service.load_live_workspace().unwrap();
    let backfilled = service
        .backfill_active_provider(BackfillActiveProvider {
            guard: guard(&before_backfill),
        })
        .unwrap();
    let active = backfilled
        .live
        .provider
        .document
        .profiles
        .iter()
        .find(|profile| profile.id() == "a")
        .unwrap()
        .ordinary()
        .unwrap();
    assert!(active.config_contents.contains("backfill.example"));
    assert!(active.auth_contents.contains("backfilled-key"));
    assert_eq!(
        backfilled.live.files.config_contents,
        before_backfill.files.config_contents
    );

    let cleared = service
        .clear_live_provider(ClearLiveProvider {
            guard: guard(&backfilled.live),
        })
        .unwrap();
    assert!(!cleared.live.status.configured);
    assert!(cleared.backup_path.is_some());
}

#[test]
fn raw_live_file_saves_validate_and_preserve_the_other_file() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let before = service.load_live_workspace().unwrap();
    let old_auth = before.files.auth_contents.clone();
    let new_config = live_config("https://raw.example/v1", "raw-model");

    let config_saved = service
        .save_live_file(SaveLiveFile {
            guard: guard(&before),
            kind: ProviderLiveFileKind::Config,
            contents: new_config.clone(),
        })
        .unwrap();
    assert_eq!(config_saved.live.files.config_contents, new_config);
    assert_eq!(config_saved.live.files.auth_contents, old_auth);
    assert!(config_saved.backup_path.is_some());

    let before_invalid = config_saved.live;
    let error = service
        .save_live_file(SaveLiveFile {
            guard: guard(&before_invalid),
            kind: ProviderLiveFileKind::Auth,
            contents: format!("{{{SECRET}"),
        })
        .unwrap_err();
    assert_eq!(error.kind(), ProviderActivationErrorKind::InvalidLiveFile);
    assert_eq!(error.rollback(), ProviderRollbackOutcome::NotRequired);
    assert!(!format!("{error:?}").contains(SECRET));
    assert_eq!(
        service.load_live_workspace().unwrap().files,
        before_invalid.files
    );
}

#[test]
fn disabled_or_missing_targets_fail_without_mutation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store
        .update(serde_json::json!({"relayProfilesEnabled": false}))
        .unwrap();
    let service = fixture.service();
    let disabled = service.load_live_workspace().unwrap();
    let disabled_error = service
        .switch_provider(SwitchProvider {
            guard: guard(&disabled),
            target_profile_id: "b".to_string(),
        })
        .unwrap_err();
    assert_eq!(disabled_error.kind(), ProviderActivationErrorKind::Disabled);

    store
        .update(serde_json::json!({"relayProfilesEnabled": true}))
        .unwrap();
    let enabled = service.load_live_workspace().unwrap();
    let missing_error = service
        .switch_provider(SwitchProvider {
            guard: guard(&enabled),
            target_profile_id: "missing".to_string(),
        })
        .unwrap_err();
    assert_eq!(
        missing_error.kind(),
        ProviderActivationErrorKind::ProfileNotFound
    );
    assert_eq!(store.load().unwrap().active_relay_id, "a");
}

#[test]
fn aggregate_to_official_switch_preserves_routing_and_restores_official_auth() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.load().unwrap();
    settings.relay_profiles.push(RelayProfile {
        id: "aggregate".to_string(),
        name: "Aggregate".to_string(),
        relay_mode: RelayMode::Aggregate,
        ..RelayProfile::default()
    });
    settings.aggregate_relay_profiles = vec![AggregateRelayProfile {
        id: "aggregate".to_string(),
        name: "Aggregate".to_string(),
        strategy: AggregateRelayStrategy::Failover,
        members: vec![AggregateRelayMember {
            relay_id: "a".to_string(),
            weight: 1,
        }],
    }];
    settings.relay_profiles.push(RelayProfile {
        id: "official".to_string(),
        name: "Official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official-token"}}"#
            .to_string(),
        ..RelayProfile::default()
    });
    store.save(&settings).unwrap();
    let service = fixture.service();
    let before = service.load_live_workspace().unwrap();

    let aggregate = service
        .switch_provider(SwitchProvider {
            guard: guard(&before),
            target_profile_id: "aggregate".to_string(),
        })
        .unwrap();
    assert_eq!(
        aggregate
            .live
            .provider
            .activation
            .active_profile_id
            .as_deref(),
        Some("aggregate")
    );
    assert_eq!(store.load().unwrap().active_aggregate_relay_id, "aggregate");
    assert!(
        aggregate
            .live
            .files
            .config_contents
            .contains("http://127.0.0.1:57321/v1")
    );

    let official = service
        .switch_provider(SwitchProvider {
            guard: guard(&aggregate.live),
            target_profile_id: "official".to_string(),
        })
        .unwrap();
    let stored = store.load().unwrap();
    assert_eq!(stored.active_relay_id, "official");
    assert!(stored.active_aggregate_relay_id.is_empty());
    assert_eq!(
        stored
            .relay_profiles
            .iter()
            .find(|profile| profile.id == "aggregate")
            .unwrap()
            .relay_mode,
        RelayMode::Aggregate
    );
    assert_eq!(stored.aggregate_relay_profiles.len(), 1);
    assert!(!official.live.status.configured);
    assert!(official.live.files.auth_contents.contains("official-token"));
    assert!(!official.live.files.auth_contents.contains("OPENAI_API_KEY"));
}
