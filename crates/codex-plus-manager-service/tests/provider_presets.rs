use codex_plus_core::settings::{RelayContextSelection, RelayMode, RelayProfile};
use codex_plus_manager_service::{
    ProviderPresetCatalogErrorKind, apply_provider_preset, parse_provider_presets, provider_presets,
};

#[test]
fn embedded_provider_presets_are_typed_unique_and_ordered() {
    let presets = provider_presets().unwrap();

    assert_eq!(presets.len(), 25);
    assert_eq!(presets.first().unwrap().id, "openai");
    assert_eq!(presets.last().unwrap().id, "azure");
    assert!(presets.iter().all(|preset| !preset.base_url.is_empty()));
}

#[test]
fn provider_preset_parser_rejects_duplicate_ids_and_invalid_protocols() {
    let duplicate = r#"[
        {"id":"same","name":"A","category":"official","baseUrl":"https://a.test/v1","protocol":"responses","model":"a"},
        {"id":"same","name":"B","category":"official","baseUrl":"https://b.test/v1","protocol":"responses","model":"b"}
    ]"#;
    let invalid_protocol = r#"[
        {"id":"bad","name":"Bad","category":"official","baseUrl":"https://bad.test/v1","protocol":"legacy","model":"a"}
    ]"#;

    assert_eq!(
        parse_provider_presets(duplicate).unwrap_err().kind(),
        ProviderPresetCatalogErrorKind::DuplicateId
    );
    assert_eq!(
        parse_provider_presets(invalid_protocol).unwrap_err().kind(),
        ProviderPresetCatalogErrorKind::InvalidProtocol
    );
}

#[test]
fn applying_provider_preset_preserves_secrets_context_and_advanced_fields() {
    let preset = provider_presets()
        .unwrap()
        .iter()
        .find(|preset| preset.id == "deepseek")
        .unwrap();
    let mut profile = RelayProfile {
        id: "existing-id".to_string(),
        name: "Existing".to_string(),
        api_key: "secret-api-key".to_string(),
        config_contents: "secret-config".to_string(),
        auth_contents: "secret-auth".to_string(),
        context_selection: RelayContextSelection {
            mcp_servers: vec!["mcp-a".to_string()],
            skills: vec!["skill-a".to_string()],
            plugins: vec!["plugin-a".to_string()],
        },
        context_selection_initialized: true,
        context_window: "500000".to_string(),
        auto_compact_limit: "400000".to_string(),
        model_windows: r#"{"old":"200K"}"#.to_string(),
        user_agent: "custom-agent".to_string(),
        ..RelayProfile::default()
    };

    apply_provider_preset(&mut profile, preset);

    assert_eq!(profile.id, "existing-id");
    assert_eq!(profile.name, "DeepSeek");
    assert_eq!(profile.upstream_base_url, "https://api.deepseek.com");
    assert_eq!(profile.model, "deepseek-v4-flash");
    assert_eq!(profile.test_model, "deepseek-v4-flash");
    assert_eq!(profile.model_list, "deepseek-v4-flash\ndeepseek-v4-pro");
    assert_eq!(profile.relay_mode, RelayMode::PureApi);
    assert_eq!(profile.api_key, "secret-api-key");
    assert_eq!(profile.config_contents, "secret-config");
    assert_eq!(profile.auth_contents, "secret-auth");
    assert_eq!(profile.context_selection.mcp_servers, ["mcp-a"]);
    assert!(profile.context_selection_initialized);
    assert_eq!(profile.context_window, "500000");
    assert_eq!(profile.auto_compact_limit, "400000");
    assert_eq!(profile.model_windows, r#"{"old":"200K"}"#);
    assert_eq!(profile.user_agent, "custom-agent");
}
