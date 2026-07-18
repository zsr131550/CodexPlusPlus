use std::path::{Path, PathBuf};
use std::sync::Arc;

use codex_plus_core::relay_config::{relay_profile_base_url, test_relay_profile};
use codex_plus_core::settings::{BackendSettings, SettingsStore};
use codex_plus_core::settings::{RelayProfile, RelayProtocol};
use serde_json::Value;

use crate::{
    NetworkModelsResponse, NetworkTestResponse, ProviderActivationEnvironment, ProviderEnvironment,
    ProviderEnvironmentNetworkError, ProviderNetworkEnvironment, ProviderNetworkFailureKind,
};

#[derive(Clone)]
pub struct SystemProviderEnvironment {
    settings: SettingsStore,
    codex_home: PathBuf,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl SystemProviderEnvironment {
    pub fn for_settings_path(path: PathBuf) -> Self {
        let codex_home = isolated_codex_home_for_settings(&path);
        Self::for_paths(path, codex_home)
    }

    pub fn for_paths(settings_path: PathBuf, codex_home: PathBuf) -> Self {
        Self {
            settings: SettingsStore::new(settings_path),
            codex_home,
            runtime: Arc::new(provider_runtime()),
        }
    }

    pub fn for_native_process() -> Self {
        let settings_path = std::env::var_os("CODEX_PLUS_NATIVE_SETTINGS_PATH")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let codex_home = std::env::var_os("CODEX_PLUS_NATIVE_CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        match (settings_path, codex_home) {
            (None, None) => Self::default(),
            (Some(settings_path), None) => Self::for_settings_path(settings_path),
            (settings_path, Some(codex_home)) => Self::for_paths(
                settings_path.unwrap_or_else(codex_plus_core::paths::default_settings_path),
                codex_home,
            ),
        }
    }
}

impl Default for SystemProviderEnvironment {
    fn default() -> Self {
        Self {
            settings: SettingsStore::default(),
            codex_home: codex_plus_core::relay_config::default_codex_home_dir(),
            runtime: Arc::new(provider_runtime()),
        }
    }
}

impl ProviderNetworkEnvironment for SystemProviderEnvironment {
    fn test_provider(
        &self,
        profile: &RelayProfile,
        model: &str,
    ) -> Result<NetworkTestResponse, ProviderEnvironmentNetworkError> {
        let base_url = relay_profile_base_url(profile);
        let endpoint = test_endpoint(&base_url, profile.protocol);
        self.runtime
            .block_on(test_relay_profile(profile, model))
            .map(|result| NetworkTestResponse {
                http_status: result.http_status,
                endpoint: result.endpoint,
            })
            .map_err(|error| environment_error(error.to_string(), Some(endpoint)))
    }

    fn fetch_provider_models(
        &self,
        profile: &RelayProfile,
    ) -> Result<NetworkModelsResponse, ProviderEnvironmentNetworkError> {
        let base_url = relay_profile_base_url(profile);
        let endpoint = codex_plus_core::protocol_proxy::models_url(&base_url);
        self.runtime
            .block_on(codex_plus_core::model_catalog::fetch_relay_profile_model_ids(profile))
            .map(|(models, endpoint)| NetworkModelsResponse { models, endpoint })
            .map_err(|error| environment_error(error.to_string(), Some(endpoint)))
    }
}

fn provider_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("codex-plus-provider")
        .build()
        .expect("provider runtime should initialize before the UI event loop")
}

fn test_endpoint(base_url: &str, protocol: RelayProtocol) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    match protocol {
        RelayProtocol::Responses => format!("{base_url}/responses"),
        RelayProtocol::ChatCompletions => format!("{base_url}/chat/completions"),
    }
}

fn environment_error(
    technical_detail: String,
    endpoint: Option<String>,
) -> ProviderEnvironmentNetworkError {
    let lower = technical_detail.to_ascii_lowercase();
    let (kind, status) = if lower.contains("http 401") || lower.contains("http 403") {
        (ProviderNetworkFailureKind::Unauthorized, Some(401))
    } else if lower.contains("http 404") {
        (ProviderNetworkFailureKind::NotFound, Some(404))
    } else if lower.contains("http 429") {
        (ProviderNetworkFailureKind::RateLimited, Some(429))
    } else if (500..=599).any(|status| lower.contains(&format!("http {status}"))) {
        (ProviderNetworkFailureKind::UpstreamFailure, None)
    } else if lower.contains("timed out") || lower.contains("timeout") {
        (ProviderNetworkFailureKind::Timeout, None)
    } else {
        (ProviderNetworkFailureKind::Network, None)
    };
    ProviderEnvironmentNetworkError::new(kind, status, endpoint, technical_detail)
}

impl ProviderEnvironment for SystemProviderEnvironment {
    fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        self.settings.load()
    }

    fn update_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        let _lock =
            codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&self.codex_home)?;
        self.settings.update_if(payload, predicate)
    }
}

impl ProviderActivationEnvironment for SystemProviderEnvironment {
    fn settings_store(&self) -> &SettingsStore {
        &self.settings
    }

    fn codex_home(&self) -> &Path {
        &self.codex_home
    }
}

fn isolated_codex_home_for_settings(settings_path: &Path) -> PathBuf {
    settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("codex")
}
