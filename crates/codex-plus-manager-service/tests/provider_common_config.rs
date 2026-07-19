use std::fs;
use std::path::Path;

use codex_plus_core::settings::{
    AggregateRelayProfile, BackendSettings, RelayMode, RelayProfile, SettingsStore,
};
use codex_plus_manager_service::{
    ExtractProviderCommonConfig, ProviderCommonConfigExtraction, ProviderErrorKind,
    ProviderService, ProviderSource, ProviderValidationKind, SystemProviderEnvironment,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn ordinary(id: &str, config_contents: &str) -> RelayProfile {
    RelayProfile {
        id: id.to_string(),
        name: format!("Relay {id}"),
        relay_mode: RelayMode::PureApi,
        model_list: "gpt-test".to_string(),
        config_contents: config_contents.to_string(),
        ..RelayProfile::default()
    }
}

fn write_settings(path: &Path, settings: &BackendSettings) {
    let mut value = serde_json::to_value(settings).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("customField".to_string(), json!({"nested": true}));
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn service(path: &Path) -> ProviderService<SystemProviderEnvironment> {
    ProviderService::new(SystemProviderEnvironment::for_settings_path(
        path.to_path_buf(),
    ))
}

fn extraction_request(
    workspace: &codex_plus_manager_service::ProviderWorkspace,
    profile_id: &str,
) -> ExtractProviderCommonConfig {
    ExtractProviderCommonConfig {
        expected_revision: workspace.revision.clone(),
        document: workspace.document.clone(),
        profile_id: profile_id.to_string(),
    }
}

#[test]
fn extraction_moves_structured_common_and_context_sections_in_one_provider_transaction() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let config = concat!(
        "model = \"gpt-test\"\n",
        "model_provider = \"proxy\"\n",
        "approval_policy = \"never\"\n",
        "[mcp_servers.alpha]\n",
        "command = \"alpha\"\n",
        "[skills.beta]\n",
        "enabled = true\n",
        "[model_providers.proxy]\n",
        "name = \"Proxy\"\n",
        "base_url = \"https://api.example/v1\"\n",
        "env_key = \"TOKEN\"\n",
    );
    let settings = BackendSettings {
        relay_profiles: vec![ordinary("relay-a", config)],
        relay_common_config_contents: "old_common = true\n".to_string(),
        relay_context_config_contents: "[plugins.existing]\nenabled = true\n".to_string(),
        provider_sync_enabled: true,
        ..BackendSettings::default()
    };
    write_settings(&path, &settings);
    let source = service(&path);
    let workspace = source.load_workspace().unwrap();

    let outcome = source
        .extract_common_config(extraction_request(&workspace, "relay-a"))
        .unwrap();
    let ProviderCommonConfigExtraction::Applied(saved) = outcome else {
        panic!("expected applied extraction");
    };
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let profile = saved.document.profiles[0].ordinary().unwrap();

    assert_eq!(
        saved.document.common_config_contents,
        "approval_policy = \"never\"\n"
    );
    assert_eq!(
        saved.document.context_config_contents,
        concat!(
            "[plugins.existing]\n",
            "enabled = true\n\n",
            "[mcp_servers.alpha]\n",
            "command = \"alpha\"\n",
            "[skills.beta]\n",
            "enabled = true\n",
        )
    );
    assert_eq!(
        profile.config_contents,
        concat!(
            "model = \"gpt-test\"\n",
            "model_provider = \"proxy\"\n",
            "[model_providers.proxy]\n",
            "name = \"Proxy\"\n",
            "base_url = \"https://api.example/v1\"\n",
            "env_key = \"TOKEN\"\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = true\n",
        )
    );
    assert_eq!(raw["customField"], json!({"nested": true}));
    assert_eq!(raw["providerSyncEnabled"], json!(true));
    assert_eq!(
        raw["relayCommonConfigContents"],
        json!(saved.document.common_config_contents)
    );
    assert_eq!(
        raw["relayContextConfigContents"],
        json!(saved.document.context_config_contents)
    );
    assert_eq!(
        raw["relayProfiles"][0]["configContents"],
        json!(profile.config_contents)
    );
}

#[test]
fn extraction_with_no_common_content_is_a_no_op() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let config = concat!(
        "model = \"gpt-test\"\n",
        "model_provider = \"proxy\"\n",
        "[model_providers.proxy]\n",
        "name = \"Proxy\"\n",
    );
    write_settings(
        &path,
        &BackendSettings {
            relay_profiles: vec![ordinary("relay-a", config)],
            ..BackendSettings::default()
        },
    );
    let source = service(&path);
    let workspace = source.load_workspace().unwrap();
    let before = fs::read(&path).unwrap();

    let outcome = source
        .extract_common_config(extraction_request(&workspace, "relay-a"))
        .unwrap();

    assert_eq!(outcome, ProviderCommonConfigExtraction::NoContent);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn invalid_profile_toml_returns_stable_validation_without_writing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    write_settings(
        &path,
        &BackendSettings {
            relay_profiles: vec![ordinary("relay-a", "[invalid")],
            ..BackendSettings::default()
        },
    );
    let source = service(&path);
    let workspace = source.load_workspace().unwrap();
    let before = fs::read(&path).unwrap();

    let error = source
        .extract_common_config(extraction_request(&workspace, "relay-a"))
        .unwrap_err();

    assert_eq!(error.kind(), ProviderErrorKind::Validation);
    assert!(error.validation_issues().iter().any(|issue| {
        issue.profile_id.as_deref() == Some("relay-a")
            && issue.kind == ProviderValidationKind::InvalidConfigToml
    }));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn stale_extraction_conflicts_before_reporting_no_content() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    write_settings(
        &path,
        &BackendSettings {
            relay_profiles: vec![ordinary("relay-a", "model = \"gpt-test\"\n")],
            ..BackendSettings::default()
        },
    );
    let source = service(&path);
    let stale = source.load_workspace().unwrap();
    SettingsStore::new(path.clone())
        .update(json!({"relayTestModel": "concurrent-model"}))
        .unwrap();
    let after_concurrent_write = fs::read(&path).unwrap();

    let error = source
        .extract_common_config(extraction_request(&stale, "relay-a"))
        .unwrap_err();

    assert_eq!(error.kind(), ProviderErrorKind::Conflict);
    assert_eq!(fs::read(&path).unwrap(), after_concurrent_write);
}

#[test]
fn extraction_requires_an_existing_ordinary_profile() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    let aggregate_shell = RelayProfile {
        id: "aggregate-a".to_string(),
        name: "Aggregate A".to_string(),
        relay_mode: RelayMode::Aggregate,
        ..RelayProfile::default()
    };
    write_settings(
        &path,
        &BackendSettings {
            relay_profiles: vec![ordinary("relay-a", ""), aggregate_shell],
            aggregate_relay_profiles: vec![AggregateRelayProfile {
                id: "aggregate-a".to_string(),
                name: "Aggregate A".to_string(),
                strategy: Default::default(),
                members: Vec::new(),
            }],
            ..BackendSettings::default()
        },
    );
    let source = service(&path);
    let workspace = source.load_workspace().unwrap();

    for profile_id in ["missing", "aggregate-a"] {
        let error = source
            .extract_common_config(extraction_request(&workspace, profile_id))
            .unwrap_err();
        assert_eq!(error.kind(), ProviderErrorKind::Validation);
        assert!(error.validation_issues().iter().any(|issue| {
            issue.profile_id.as_deref() == Some(profile_id)
                && issue.kind == ProviderValidationKind::CommonConfigProfileUnavailable
        }));
    }
}

#[test]
fn extraction_request_debug_does_not_expose_profile_contents() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("settings.json");
    write_settings(
        &path,
        &BackendSettings {
            relay_profiles: vec![ordinary(
                "relay-a",
                "secret_marker = \"must-not-appear-in-debug\"\n",
            )],
            ..BackendSettings::default()
        },
    );
    let workspace = service(&path).load_workspace().unwrap();

    let debug = format!("{:?}", extraction_request(&workspace, "relay-a"));

    assert!(!debug.contains("must-not-appear-in-debug"));
    assert!(!debug.contains("secret_marker"));
    assert!(debug.contains("relay-a"));
}
