use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_core::relay_config::{relay_profile_base_url, test_relay_profile};
use codex_plus_core::settings::{BackendSettings, SettingsStore};
use codex_plus_core::settings::{RelayProfile, RelayProtocol};
use serde_json::Value;

use crate::{
    NetworkModelsResponse, NetworkTestResponse, ProviderEnvironment,
    ProviderEnvironmentNetworkError, ProviderNetworkEnvironment, ProviderNetworkFailureKind,
};

#[derive(Clone)]
pub struct SystemProviderEnvironment {
    settings: SettingsStore,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl SystemProviderEnvironment {
    pub fn for_settings_path(path: PathBuf) -> Self {
        Self {
            settings: SettingsStore::new(path),
            runtime: Arc::new(provider_runtime()),
        }
    }

    pub fn for_native_process() -> Self {
        std::env::var_os("CODEX_PLUS_NATIVE_SETTINGS_PATH")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(Self::for_settings_path)
            .unwrap_or_default()
    }
}

impl Default for SystemProviderEnvironment {
    fn default() -> Self {
        Self {
            settings: SettingsStore::default(),
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
        self.settings.update_if(payload, predicate)
    }
}
