use std::fs;
use std::path::Path;

use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, BackendSettings, RelayMode, RelayProfile,
    SettingsStore,
};
use codex_plus_manager_service::{
    ProviderActivationSummary, ProviderDocument, ProviderErrorKind, ProviderField, ProviderKind,
    ProviderProfile, ProviderService, ProviderSource, ProviderValidationKind,
    SaveProviderWorkspace, SystemProviderEnvironment, ValidationSeverity,
    validate_provider_document,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn ordinary(id: &str, name: &str) -> RelayProfile {
    RelayProfile {
        id: id.to_string(),
        name: name.to_string(),
        relay_mode: RelayMode::Official,
        model_list: "gpt-test".to_string(),
        ..RelayProfile::default()
    }
}

fn aggregate(id: &str, members: &[(&str, u32)]) -> (RelayProfile, AggregateRelayProfile) {
    (
        RelayProfile {
            id: id.to_string(),
            name: format!("Aggregate {id}"),
            relay_mode: RelayMode::Aggregate,
            ..RelayProfile::default()
        },
        AggregateRelayProfile {
            id: id.to_string(),
            name: format!("Aggregate {id}"),
            strategy: Default::default(),
            members: members
                .iter()
                .map(|(relay_id, weight)| AggregateRelayMember {
                    relay_id: (*relay_id).to_string(),
                    weight: *weight,
                })
                .collect(),
        },
    )
}

fn write_settings(path: &Path, settings: &BackendSettings, unknown: Option<Value>) {
    let mut value = serde_json::to_value(settings).unwrap();
    if let Some(unknown) = unknown {
        value
            .as_object_mut()
            .unwrap()
            .insert("customField".to_string(), unknown);
    }
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn service(path: &Path) -> ProviderService<SystemProviderEnvironment> {
    ProviderService::new(SystemProviderEnvironment::for_settings_path(
        path.to_path_buf(),
    ))
}

#[test]
fn workspace_load_unifies_ordinary_and_aggregate_profiles_in_shell_order() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let (aggregate_shell, aggregate_routing) = aggregate("aggregate-a", &[("relay-a", 2)]);
    let settings = BackendSettings {
        relay_profiles: vec![
            ordinary("relay-a", "Relay A"),
            aggregate_shell,
            ordinary("relay-b", "Relay B"),
        ],
        aggregate_relay_profiles: vec![aggregate_routing],
        active_relay_id: "aggregate-a".to_string(),
        active_aggregate_relay_id: "aggregate-a".to_string(),
        ..BackendSettings::default()
    };
    SettingsStore::new(path.clone()).save(&settings).unwrap();

    let workspace = service(&path).load_workspace().unwrap();

    assert_eq!(workspace.document.profiles.len(), 3);
    assert_eq!(workspace.document.profiles[0].id(), "relay-a");
    assert_eq!(
        workspace.document.profiles[0].kind(),
        ProviderKind::Ordinary
    );
    assert_eq!(workspace.document.profiles[1].id(), "aggregate-a");
    assert_eq!(
        workspace.document.profiles[1].kind(),
        ProviderKind::Aggregate
    );
    assert_eq!(workspace.document.profiles[2].id(), "relay-b");
    assert_eq!(
        workspace.activation.active_profile_id.as_deref(),
        Some("aggregate-a")
    );
    assert_eq!(
        workspace.activation.active_profile_kind,
        Some(ProviderKind::Aggregate)
    );
    match &workspace.document.profiles[1] {
        ProviderProfile::Aggregate { routing, .. } => {
            assert_eq!(routing.members[0].relay_id, "relay-a");
            assert_eq!(routing.members[0].weight, 2);
        }
        ProviderProfile::Ordinary(_) => panic!("expected aggregate profile"),
    }
}

#[test]
fn provider_revision_is_deterministic_and_ignores_unrelated_settings() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let mut profile = ordinary("relay-a", "Relay A");
    profile.model_windows = r#"{"z":"200K","a":"1M"}"#.to_string();
    let settings = BackendSettings {
        relay_profiles: vec![profile],
        relay_test_model: "gpt-test".to_string(),
        provider_sync_enabled: false,
        ..BackendSettings::default()
    };
    SettingsStore::new(path.clone()).save(&settings).unwrap();
    let source = service(&path);

    let first = source.load_workspace().unwrap();
    let second = source.load_workspace().unwrap();
    assert_eq!(first.revision, second.revision);

    SettingsStore::new(path.clone())
        .update(json!({"providerSyncEnabled": true}))
        .unwrap();
    let unrelated = source.load_workspace().unwrap();
    assert_eq!(first.revision, unrelated.revision);

    SettingsStore::new(path.clone())
        .update(json!({"relayTestModel": "different-model"}))
        .unwrap();
    let owned = source.load_workspace().unwrap();
    assert_ne!(first.revision, owned.revision);

    let second_path = temp.path().join("settings-second.json");
    let mut reordered = settings;
    reordered.relay_profiles[0].model_windows = r#"{"a":"1M","z":"200K"}"#.to_string();
    SettingsStore::new(second_path.clone())
        .save(&reordered)
        .unwrap();
    assert_eq!(
        first.revision,
        service(&second_path).load_workspace().unwrap().revision
    );
}

#[test]
fn save_updates_only_provider_fields_and_never_touches_live_codex_files() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let live_config = temp.path().join("config.toml");
    let live_auth = temp.path().join("auth.json");
    fs::write(&live_config, b"model = \"live-sentinel\"\n").unwrap();
    fs::write(&live_auth, br#"{"token":"live-secret-sentinel"}"#).unwrap();
    let config_before = fs::read(&live_config).unwrap();
    let auth_before = fs::read(&live_auth).unwrap();

    let settings = BackendSettings {
        relay_profiles: vec![ordinary("relay-a", "Relay A")],
        active_relay_id: "relay-a".to_string(),
        provider_sync_enabled: true,
        relay_base_url: "https://legacy.example/v1".to_string(),
        relay_api_key: "legacy-key-must-stay".to_string(),
        ..BackendSettings::default()
    };
    write_settings(&path, &settings, Some(json!({"nested": true})));
    let source = service(&path);
    let mut workspace = source.load_workspace().unwrap();
    match &mut workspace.document.profiles[0] {
        ProviderProfile::Ordinary(profile) => profile.name = "Renamed Relay".to_string(),
        ProviderProfile::Aggregate { .. } => panic!("expected ordinary profile"),
    }

    let saved = source
        .save_workspace(SaveProviderWorkspace {
            expected_revision: workspace.revision,
            document: workspace.document,
        })
        .unwrap();
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

    assert_eq!(saved.document.profiles[0].name(), "Renamed Relay");
    assert_eq!(raw["providerSyncEnabled"], json!(true));
    assert_eq!(raw["activeRelayId"], json!("relay-a"));
    assert_eq!(raw["relayBaseUrl"], json!("https://legacy.example/v1"));
    assert_eq!(raw["relayApiKey"], json!("legacy-key-must-stay"));
    assert_eq!(raw["customField"], json!({"nested": true}));
    assert_eq!(fs::read(&live_config).unwrap(), config_before);
    assert_eq!(fs::read(&live_auth).unwrap(), auth_before);
}

#[test]
fn stale_provider_save_returns_conflict_without_writing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    SettingsStore::new(path.clone())
        .save(&BackendSettings {
            relay_profiles: vec![ordinary("relay-a", "Relay A")],
            ..BackendSettings::default()
        })
        .unwrap();
    let source = service(&path);
    let stale = source.load_workspace().unwrap();
    SettingsStore::new(path.clone())
        .update(json!({"relayTestModel": "concurrent-model"}))
        .unwrap();
    let bytes_after_concurrent_change = fs::read(&path).unwrap();

    let error = source
        .save_workspace(SaveProviderWorkspace {
            expected_revision: stale.revision,
            document: stale.document,
        })
        .unwrap_err();

    assert_eq!(error.kind(), ProviderErrorKind::Conflict);
    assert_eq!(fs::read(&path).unwrap(), bytes_after_concurrent_change);
}

#[test]
fn validation_reports_structural_and_field_errors_without_panicking() {
    let mut invalid = ordinary("duplicate", "");
    invalid.model_windows = r#"{"gpt":"0K"}"#.to_string();
    invalid.context_window = "1.5M".to_string();
    invalid.auto_compact_limit = "0".to_string();
    invalid.config_contents = "[invalid".to_string();
    invalid.auth_contents = "[]".to_string();
    let (shell, mut routing) = aggregate(
        "aggregate-a",
        &[
            ("aggregate-a", 1),
            ("missing", 1),
            ("duplicate", 0),
            ("duplicate", 2),
        ],
    );
    routing.id = "different-id".to_string();
    let document = ProviderDocument {
        profiles: vec![
            ProviderProfile::Ordinary(invalid),
            ProviderProfile::Ordinary(ordinary("duplicate", "Duplicate")),
            ProviderProfile::Aggregate { shell, routing },
        ],
        common_config_contents: String::new(),
        context_config_contents: String::new(),
        default_test_model: String::new(),
    };
    let activation = ProviderActivationSummary {
        enabled: true,
        active_profile_id: Some("deleted-active".to_string()),
        active_profile_kind: Some(ProviderKind::Ordinary),
    };

    let issues = validate_provider_document(&document, &activation);
    let kinds = issues.iter().map(|issue| issue.kind).collect::<Vec<_>>();

    assert!(kinds.contains(&ProviderValidationKind::DuplicateId));
    assert!(kinds.contains(&ProviderValidationKind::ActiveProfileDeleted));
    assert!(kinds.contains(&ProviderValidationKind::AggregateIdMismatch));
    assert!(kinds.contains(&ProviderValidationKind::AggregateMemberSelfReference));
    assert!(kinds.contains(&ProviderValidationKind::AggregateMemberMissing));
    assert!(kinds.contains(&ProviderValidationKind::AggregateMemberDuplicate));
    assert!(kinds.contains(&ProviderValidationKind::AggregateMemberWeightOutOfRange));
    assert!(kinds.contains(&ProviderValidationKind::AggregateHasNoValidMember));
    assert!(kinds.contains(&ProviderValidationKind::InvalidModelWindowToken));
    assert!(kinds.contains(&ProviderValidationKind::InvalidPositiveInteger));
    assert!(kinds.contains(&ProviderValidationKind::InvalidConfigToml));
    assert!(kinds.contains(&ProviderValidationKind::InvalidAuthJson));
    assert!(issues.iter().any(|issue| {
        issue.field == ProviderField::Name
            && issue.severity == ValidationSeverity::Warning
            && issue.kind == ProviderValidationKind::MissingName
    }));
}

#[test]
fn validation_blocks_zero_ordinary_profiles_and_protected_active_deletion() {
    let (shell, routing) = aggregate("aggregate-a", &[]);
    let document = ProviderDocument {
        profiles: vec![ProviderProfile::Aggregate { shell, routing }],
        common_config_contents: String::new(),
        context_config_contents: String::new(),
        default_test_model: String::new(),
    };
    let activation = ProviderActivationSummary {
        enabled: true,
        active_profile_id: Some("relay-a".to_string()),
        active_profile_kind: Some(ProviderKind::Ordinary),
    };

    let issues = validate_provider_document(&document, &activation);

    assert!(issues.iter().any(|issue| {
        issue.kind == ProviderValidationKind::NoOrdinaryProfiles
            && issue.severity == ValidationSeverity::Error
    }));
    assert!(
        issues
            .iter()
            .any(|issue| issue.kind == ProviderValidationKind::ActiveProfileDeleted)
    );
}
