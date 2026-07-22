use crate::settings::{BackendSettings, RelayMode, RelayProfile, RelayProtocol, SettingsStore};
use anyhow::Context;
use base64::Engine;
use std::fmt;
use std::path::Path;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportRequest {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default = "default_wire_api")]
    pub wire_api: String,
    #[serde(default = "default_relay_mode")]
    pub relay_mode: String,
    #[serde(default)]
    pub config_contents: String,
    #[serde(default)]
    pub auth_contents: String,
}

impl fmt::Debug for ProviderImportRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderImportRequest")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("wire_api", &self.wire_api)
            .field("relay_mode", &self.relay_mode)
            .field("api_key_present", &!self.api_key.trim().is_empty())
            .field(
                "config_contents_present",
                &!self.config_contents.trim().is_empty(),
            )
            .field(
                "auth_contents_present",
                &!self.auth_contents.trim().is_empty(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportResult {
    pub imported: bool,
    pub profile_id: String,
    pub profile_name: String,
}

pub fn apply_provider_import_to_settings(
    settings: &BackendSettings,
    request: &ProviderImportRequest,
) -> anyhow::Result<(BackendSettings, ProviderImportResult)> {
    let request = normalize_request(request.clone())?;
    let identity = provider_identity(&request.name, &request.base_url);
    if let Some(existing) = settings
        .relay_profiles
        .iter()
        .find(|profile| provider_identity(&profile.name, &profile.upstream_base_url) == identity)
    {
        return Ok((
            settings.clone(),
            ProviderImportResult {
                imported: false,
                profile_id: existing.id.clone(),
                profile_name: existing.name.clone(),
            },
        ));
    }

    let existing_ids = settings
        .relay_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let profile = relay_profile_from_request(&request, &existing_ids);
    let result = ProviderImportResult {
        imported: true,
        profile_id: profile.id.clone(),
        profile_name: profile.name.clone(),
    };
    let mut next = settings.clone();
    next.relay_profiles.push(profile);
    next.active_relay_id = result.profile_id.clone();
    Ok((next, result))
}

pub fn import_provider_from_url(url: &str) -> anyhow::Result<ProviderImportResult> {
    let request = request_from_url(url)?;
    import_provider(request)
}

pub fn save_pending_provider_import_from_url(url: &str) -> anyhow::Result<ProviderImportRequest> {
    let request = request_from_url(url)?;
    save_pending_provider_import(&request)?;
    Ok(request)
}

pub fn save_pending_provider_import(request: &ProviderImportRequest) -> anyhow::Result<()> {
    save_pending_provider_import_at(
        &crate::paths::default_pending_provider_import_path(),
        request,
    )
}

pub fn load_pending_provider_import() -> anyhow::Result<Option<ProviderImportRequest>> {
    load_pending_provider_import_at(&crate::paths::default_pending_provider_import_path())
}

pub fn clear_pending_provider_import() -> anyhow::Result<()> {
    clear_pending_provider_import_at(&crate::paths::default_pending_provider_import_path())
}

pub struct PendingProviderImportLock {
    path: std::path::PathBuf,
    _lock: crate::coordination_lock::CoordinationLock,
}

impl PendingProviderImportLock {
    pub fn load(&self) -> anyhow::Result<Option<ProviderImportRequest>> {
        load_pending_provider_import_unlocked(&self.path)
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        clear_pending_provider_import_unlocked(&self.path)
    }
}

pub fn acquire_pending_provider_import_lock(
    path: &Path,
) -> anyhow::Result<PendingProviderImportLock> {
    let lock =
        crate::coordination_lock::acquire_exclusive(&crate::coordination_lock::sidecar_path(path))?;
    Ok(PendingProviderImportLock {
        path: path.to_path_buf(),
        _lock: lock,
    })
}

pub fn confirm_pending_provider_import() -> anyhow::Result<Option<ProviderImportResult>> {
    let path = crate::paths::default_pending_provider_import_path();
    if !path.exists() {
        return Ok(None);
    }
    confirm_pending_provider_import_at(&path, SettingsStore::default()).map(Some)
}

pub fn save_pending_provider_import_at(
    path: &Path,
    request: &ProviderImportRequest,
) -> anyhow::Result<()> {
    let _lock = acquire_pending_provider_import_lock(path)?;
    save_pending_provider_import_unlocked(path, request)
}

fn save_pending_provider_import_unlocked(
    path: &Path,
    request: &ProviderImportRequest,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(request)?;
    crate::settings::atomic_write(path, format!("{contents}\n").as_bytes())?;
    Ok(())
}

pub fn load_pending_provider_import_at(
    path: &Path,
) -> anyhow::Result<Option<ProviderImportRequest>> {
    let _lock = acquire_pending_provider_import_lock(path)?;
    load_pending_provider_import_unlocked(path)
}

fn load_pending_provider_import_unlocked(
    path: &Path,
) -> anyhow::Result<Option<ProviderImportRequest>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("读取待确认供应商导入失败：{}", path.to_string_lossy()))?;
    let request = serde_json::from_str(&contents).context("待确认供应商导入内容无效")?;
    Ok(Some(request))
}

pub fn clear_pending_provider_import_at(path: &Path) -> anyhow::Result<()> {
    let _lock = acquire_pending_provider_import_lock(path)?;
    clear_pending_provider_import_unlocked(path)
}

fn clear_pending_provider_import_unlocked(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("清理待确认供应商导入失败：{}", path.to_string_lossy())),
    }
}

pub fn confirm_pending_provider_import_at(
    path: &Path,
    store: SettingsStore,
) -> anyhow::Result<ProviderImportResult> {
    let _lock = acquire_pending_provider_import_lock(path)?;
    let request = load_pending_provider_import_unlocked(path)?.context("没有待确认的供应商导入")?;
    let result = import_provider_with_store(request, store)?;
    clear_pending_provider_import_unlocked(path)?;
    Ok(result)
}

pub fn import_provider(request: ProviderImportRequest) -> anyhow::Result<ProviderImportResult> {
    import_provider_with_store(request, SettingsStore::default())
}

pub fn import_provider_with_store(
    request: ProviderImportRequest,
    store: SettingsStore,
) -> anyhow::Result<ProviderImportResult> {
    let current = store.load().unwrap_or_default();
    let (next, result) = apply_provider_import_to_settings(&current, &request)?;
    if result.imported {
        let payload = serde_json::json!({
            "relayProfiles": next.relay_profiles,
            "activeRelayId": next.active_relay_id,
        });
        let updated = store.update_if(payload, |fresh| fresh == &current)?;
        if updated.is_none() {
            anyhow::bail!("供应商设置已变更，请重试");
        }
    }
    Ok(result)
}

pub fn request_from_url(url: &str) -> anyhow::Result<ProviderImportRequest> {
    let (_, query) = url.split_once('?').context("导入链接缺少查询参数")?;
    let mut values = std::collections::BTreeMap::<String, String>::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    let config_contents = values
        .get("configContents")
        .map(|value| decode_base64_utf8(value))
        .transpose()?
        .unwrap_or_default();
    let auth_contents = values
        .get("authContents")
        .map(|value| decode_base64_utf8(value))
        .transpose()?
        .unwrap_or_default();
    Ok(ProviderImportRequest {
        name: required_value(&values, "name")?,
        base_url: required_value(&values, "baseUrl")?,
        api_key: required_value(&values, "apiKey")?,
        wire_api: values
            .get("wireApi")
            .cloned()
            .unwrap_or_else(default_wire_api),
        relay_mode: values
            .get("relayMode")
            .cloned()
            .unwrap_or_else(default_relay_mode),
        config_contents,
        auth_contents,
    })
}

fn relay_profile_from_request(
    request: &ProviderImportRequest,
    existing_ids: &[String],
) -> RelayProfile {
    RelayProfile {
        id: unique_profile_id(
            &format!("import-{}", sanitize_id(&request.name)),
            existing_ids,
        ),
        name: request.name.clone(),
        model: String::new(),
        base_url: request.base_url.clone(),
        upstream_base_url: request.base_url.clone(),
        api_key: request.api_key.clone(),
        protocol: relay_protocol(&request.wire_api),
        relay_mode: relay_mode(&request.relay_mode),
        official_mix_api_key: false,
        test_model: String::new(),
        config_contents: request.config_contents.clone(),
        auth_contents: request.auth_contents.clone(),
        use_common_config: true,
        context_selection: crate::settings::RelayContextSelection::default(),
        context_selection_initialized: false,
        context_window: String::new(),
        auto_compact_limit: String::new(),
        model_insert_mode: Default::default(),
        model_list: String::new(),
        model_windows: String::new(),
        user_agent: String::new(),
    }
}

fn normalize_request(mut request: ProviderImportRequest) -> anyhow::Result<ProviderImportRequest> {
    request.name = request.name.trim().to_string();
    request.base_url = request.base_url.trim().trim_end_matches('/').to_string();
    request.api_key = request.api_key.trim().to_string();
    request.wire_api = request.wire_api.trim().to_ascii_lowercase();
    request.relay_mode = request.relay_mode.trim().to_ascii_lowercase();
    if request.name.is_empty() {
        anyhow::bail!("供应商名称为空");
    }
    if request.base_url.is_empty() {
        anyhow::bail!("Base URL 为空");
    }
    if request.api_key.is_empty() {
        anyhow::bail!("API Key 为空");
    }
    if request.config_contents.trim().is_empty() {
        request.config_contents = build_config_toml(
            &request.base_url,
            &request.api_key,
            relay_protocol(&request.wire_api),
        );
    }
    if request.auth_contents.trim().is_empty() {
        request.auth_contents = build_auth_json(&request.api_key);
    }
    Ok(request)
}

fn relay_protocol(value: &str) -> RelayProtocol {
    match value.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completions" | "chat-completions" => RelayProtocol::ChatCompletions,
        _ => RelayProtocol::Responses,
    }
}

fn relay_mode(value: &str) -> RelayMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "official" => RelayMode::Official,
        "mixedapi" | "mixed-api" | "mixed_api" => RelayMode::MixedApi,
        "aggregate" => RelayMode::Aggregate,
        _ => RelayMode::PureApi,
    }
}

fn build_config_toml(base_url: &str, api_key: &str, protocol: RelayProtocol) -> String {
    let wire_api = match protocol {
        RelayProtocol::Responses => "responses",
        RelayProtocol::ChatCompletions => "chat",
    };
    [
        "model_provider = \"CodexPlusPlus\"".to_string(),
        String::new(),
        "[model_providers.CodexPlusPlus]".to_string(),
        "name = \"CodexPlusPlus\"".to_string(),
        format!("wire_api = \"{wire_api}\""),
        "requires_openai_auth = true".to_string(),
        format!("base_url = \"{}\"", toml_string(base_url)),
        format!("experimental_bearer_token = \"{}\"", toml_string(api_key)),
        String::new(),
    ]
    .join("\n")
}

fn build_auth_json(api_key: &str) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({ "OPENAI_API_KEY": api_key }))
            .unwrap_or_else(|_| "{\"OPENAI_API_KEY\":\"\"}".to_string())
    )
}

fn required_value(
    values: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> anyhow::Result<String> {
    values
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("导入链接缺少 {key}"))
}

fn decode_base64_utf8(value: &str) -> anyhow::Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value))
        .context("导入链接包含无效 base64 内容")?;
    String::from_utf8(bytes).context("导入链接内容不是 UTF-8")
}

fn percent_decode(value: &str) -> String {
    let value = value.replace('+', " ");
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(Ok(hex)) = (bytes[index] == b'%' && index + 2 < bytes.len())
            .then(|| u8::from_str_radix(&value[index + 1..index + 3], 16))
        {
            output.push(hex);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

fn provider_identity(name: &str, base_url: &str) -> String {
    format!(
        "{}\n{}",
        name.trim().to_ascii_lowercase(),
        base_url.trim().trim_end_matches('/').to_ascii_lowercase()
    )
}

fn sanitize_id(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches('-').to_string();
    if result.is_empty() {
        "provider".to_string()
    } else {
        result
    }
}

fn unique_profile_id(base: &str, existing_ids: &[String]) -> String {
    if !existing_ids.iter().any(|id| id == base) {
        return base.to_string();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing_ids.iter().any(|id| id == &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn default_wire_api() -> String {
    "responses".to_string()
}

fn default_relay_mode() -> String {
    "pureApi".to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn parses_codexplusplus_provider_url() {
        let url = "codexplusplus://v1/import/provider?resource=provider&name=JOJO%20Code&baseUrl=https%3A%2F%2Fjojocode.com%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi&configContents=bW9kZWxfcHJvdmlkZXIgPSAiQ29kZXhQbHVzUGx1cyIK&authContents=eyJPUEVOQUlfQVBJX0tFWSI6InNrLXRlc3QifQo%3D";

        let request = request_from_url(url).unwrap();

        assert_eq!(request.name, "JOJO Code");
        assert_eq!(request.base_url, "https://jojocode.com/v1");
        assert_eq!(request.api_key, "sk-test");
        assert_eq!(request.wire_api, "responses");
        assert_eq!(request.relay_mode, "pureApi");
        assert!(request.config_contents.contains("model_provider"));
        assert!(request.auth_contents.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn imports_provider_once_and_selects_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let request = ProviderImportRequest {
            name: "JOJO Code".to_string(),
            base_url: "https://jojocode.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: String::new(),
            auth_contents: String::new(),
        };

        let first = import_provider_with_store(request.clone(), store.clone()).unwrap();
        let second = import_provider_with_store(request, store.clone()).unwrap();
        let settings = store.load().unwrap();

        assert!(first.imported);
        assert!(!second.imported);
        assert_eq!(first.profile_id, second.profile_id);
        assert_eq!(settings.active_relay_id, first.profile_id);
        assert_eq!(settings.relay_profiles.len(), 2);
        assert_eq!(
            settings.relay_profiles[1].protocol,
            RelayProtocol::Responses
        );
        assert_eq!(settings.relay_profiles[1].relay_mode, RelayMode::PureApi);
        assert_eq!(
            settings.relay_profiles[1].upstream_base_url,
            "https://jojocode.com/v1"
        );
    }

    #[test]
    fn pending_provider_import_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending-provider-import.json");
        let request = ProviderImportRequest {
            name: "JOJO Code".to_string(),
            base_url: "https://jojocode.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: String::new(),
            auth_contents: String::new(),
        };

        save_pending_provider_import_at(&path, &request).unwrap();
        let pending = load_pending_provider_import_at(&path).unwrap().unwrap();
        clear_pending_provider_import_at(&path).unwrap();

        assert_eq!(pending.name, "JOJO Code");
        assert_eq!(pending.base_url, "https://jojocode.com/v1");
        assert!(load_pending_provider_import_at(&path).unwrap().is_none());
    }

    #[test]
    fn pending_save_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending-provider-import.json");
        let request = |name: &str| ProviderImportRequest {
            name: name.to_owned(),
            base_url: "https://pending.invalid/v1".to_owned(),
            api_key: "pending-secret".to_owned(),
            wire_api: "responses".to_owned(),
            relay_mode: "pureApi".to_owned(),
            config_contents: String::new(),
            auth_contents: String::new(),
        };

        save_pending_provider_import_at(&path, &request("First")).unwrap();
        save_pending_provider_import_at(&path, &request("Second")).unwrap();

        let pending = load_pending_provider_import_at(&path).unwrap().unwrap();
        assert_eq!(pending.name, "Second");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn confirms_pending_provider_import_and_removes_pending_file() {
        let dir = tempfile::tempdir().unwrap();
        let pending_path = dir.path().join("pending-provider-import.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        save_pending_provider_import_at(
            &pending_path,
            &ProviderImportRequest {
                name: "JOJO Code".to_string(),
                base_url: "https://jojocode.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                wire_api: "responses".to_string(),
                relay_mode: "pureApi".to_string(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
        )
        .unwrap();

        let result = confirm_pending_provider_import_at(&pending_path, store.clone()).unwrap();
        let settings = store.load().unwrap();

        assert!(result.imported);
        assert_eq!(settings.relay_profiles.len(), 2);
        assert!(
            load_pending_provider_import_at(&pending_path)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn pending_import_transform_sets_active_id() {
        let settings = BackendSettings::default();
        let request = ProviderImportRequest {
            name: "Imported".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "pending-secret".to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: "config-secret".to_string(),
            auth_contents: "auth-secret".to_string(),
        };

        let (next, result) = apply_provider_import_to_settings(&settings, &request).unwrap();

        assert!(result.imported);
        assert_eq!(next.active_relay_id, result.profile_id);
        assert!(!format!("{request:?}").contains("pending-secret"));
    }

    #[test]
    fn pending_duplicate_transform_does_not_write_settings() {
        let settings = BackendSettings::default();
        let request = ProviderImportRequest {
            name: "Imported".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "pending-secret".to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: String::new(),
            auth_contents: String::new(),
        };
        let (saved, first) = apply_provider_import_to_settings(&settings, &request).unwrap();
        let (unchanged, second) = apply_provider_import_to_settings(&saved, &request).unwrap();

        assert!(first.imported);
        assert!(!second.imported);
        assert_eq!(unchanged, saved);
    }

    #[test]
    fn provider_import_debug_output_redacts_secret_fields() {
        let request = ProviderImportRequest {
            name: "Imported".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "pending-secret".to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: "config-secret".to_string(),
            auth_contents: "auth-secret".to_string(),
        };

        let debug = format!("{request:?}");
        assert!(!debug.contains("pending-secret"));
        assert!(!debug.contains("config-secret"));
        assert!(!debug.contains("auth-secret"));
        assert!(debug.contains("api_key_present: true"));
    }

    #[test]
    fn import_provider_with_store_preserves_unknown_settings_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let mut raw = serde_json::to_value(BackendSettings::default()).unwrap();
        raw.as_object_mut().unwrap().insert(
            "futureField".to_owned(),
            serde_json::json!({ "keep": true }),
        );
        std::fs::write(&path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();

        import_provider_with_store(
            ProviderImportRequest {
                name: "Future-safe fixture".to_owned(),
                base_url: "https://future.invalid/v1".to_owned(),
                api_key: "pending-secret".to_owned(),
                wire_api: "responses".to_owned(),
                relay_mode: "pureApi".to_owned(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
            store,
        )
        .unwrap();

        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["futureField"]["keep"], true);
    }

    #[test]
    fn pending_operations_exclude_two_handles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending-provider-import.json");
        let first = acquire_pending_provider_import_lock(&path).unwrap();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let path_for_thread = path.clone();
        let thread = thread::spawn(move || {
            let _second = acquire_pending_provider_import_lock(&path_for_thread).unwrap();
            acquired_tx.send(()).unwrap();
        });

        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "second pending handle must wait for the stable sidecar lock"
        );
        drop(first);
        acquired_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second pending handle should acquire after release");
        thread.join().unwrap();
    }

    #[test]
    fn pending_confirm_keeps_file_when_settings_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let pending_path = dir.path().join("pending-provider-import.json");
        let invalid_settings_path = dir.path().join("settings-as-directory");
        std::fs::create_dir(&invalid_settings_path).unwrap();
        save_pending_provider_import_at(
            &pending_path,
            &ProviderImportRequest {
                name: "Failure fixture".to_owned(),
                base_url: "https://failure.invalid/v1".to_owned(),
                api_key: "pending-secret".to_owned(),
                wire_api: "responses".to_owned(),
                relay_mode: "pureApi".to_owned(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
        )
        .unwrap();

        let result = confirm_pending_provider_import_at(
            &pending_path,
            SettingsStore::new(invalid_settings_path),
        );

        assert!(result.is_err());
        assert!(
            load_pending_provider_import_at(&pending_path)
                .unwrap()
                .is_some(),
            "failed settings writes must not consume the pending request"
        );
    }
}
