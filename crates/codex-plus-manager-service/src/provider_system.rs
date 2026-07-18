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
    context_ownership_path: PathBuf,
    ccs_db_path: PathBuf,
    pending_import_path: PathBuf,
    backup_dir: PathBuf,
    process_only_env_cleanup: bool,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl SystemProviderEnvironment {
    pub fn for_settings_path(path: PathBuf) -> Self {
        let codex_home = isolated_codex_home_for_settings(&path);
        Self::for_paths(path, codex_home)
    }

    pub fn for_paths(settings_path: PathBuf, codex_home: PathBuf) -> Self {
        let state_dir = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Self::for_manager_paths(
            settings_path,
            codex_home,
            state_dir.join("cc-switch.db"),
            state_dir.join("pending-provider-import.json"),
            state_dir.join("backups"),
            true,
        )
    }

    pub fn for_manager_paths(
        settings_path: PathBuf,
        codex_home: PathBuf,
        ccs_db_path: PathBuf,
        pending_import_path: PathBuf,
        backup_dir: PathBuf,
        process_only_env_cleanup: bool,
    ) -> Self {
        let context_ownership_path = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("context-live-ownership.json");
        Self {
            settings: SettingsStore::new(settings_path),
            codex_home,
            context_ownership_path,
            ccs_db_path,
            pending_import_path,
            backup_dir,
            process_only_env_cleanup,
            runtime: Arc::new(provider_runtime()),
        }
    }

    pub fn with_context_ownership_path(mut self, path: PathBuf) -> Self {
        self.context_ownership_path = path;
        self
    }

    pub fn for_native_process() -> Self {
        let settings_path = std::env::var_os("CODEX_PLUS_NATIVE_SETTINGS_PATH")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let codex_home = std::env::var_os("CODEX_PLUS_NATIVE_CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let ccs_db_path = env_path("CODEX_PLUS_NATIVE_CCS_DB_PATH");
        let pending_import_path = env_path("CODEX_PLUS_NATIVE_PENDING_IMPORT_PATH");
        let backup_dir = env_path("CODEX_PLUS_NATIVE_BACKUP_DIR");
        let context_ownership_path = env_path("CODEX_PLUS_NATIVE_CONTEXT_OWNERSHIP_PATH");
        let process_only_env_cleanup =
            std::env::var("CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY").is_ok_and(|value| value == "1");
        if settings_path.is_none()
            && codex_home.is_none()
            && ccs_db_path.is_none()
            && pending_import_path.is_none()
            && backup_dir.is_none()
            && context_ownership_path.is_none()
        {
            return Self::default();
        }

        let settings_path =
            settings_path.unwrap_or_else(codex_plus_core::paths::default_settings_path);
        let state_dir = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let environment = Self::for_manager_paths(
            settings_path.clone(),
            codex_home.unwrap_or_else(|| isolated_codex_home_for_settings(&settings_path)),
            ccs_db_path.unwrap_or_else(|| state_dir.join("cc-switch.db")),
            pending_import_path.unwrap_or_else(|| state_dir.join("pending-provider-import.json")),
            backup_dir.unwrap_or_else(|| state_dir.join("backups")),
            process_only_env_cleanup,
        );
        context_ownership_path.map_or(environment.clone(), |path| {
            environment.with_context_ownership_path(path)
        })
    }
}

impl Default for SystemProviderEnvironment {
    fn default() -> Self {
        Self {
            settings: SettingsStore::default(),
            codex_home: codex_plus_core::relay_config::default_codex_home_dir(),
            context_ownership_path: codex_plus_core::paths::default_context_ownership_path(),
            ccs_db_path: codex_plus_core::ccs_import::default_ccs_db_path(),
            pending_import_path: codex_plus_core::paths::default_pending_provider_import_path(),
            backup_dir: codex_plus_core::paths::default_app_state_dir().join("backups"),
            process_only_env_cleanup: false,
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

impl crate::ContextToolsEnvironment for SystemProviderEnvironment {
    fn load_context_ownership(
        &self,
    ) -> anyhow::Result<codex_plus_core::context_ownership::ContextOwnershipManifest> {
        codex_plus_core::context_ownership::load_context_ownership_at(&self.context_ownership_path)
    }

    fn save_context_ownership(
        &self,
        manifest: &codex_plus_core::context_ownership::ContextOwnershipManifest,
    ) -> anyhow::Result<()> {
        codex_plus_core::context_ownership::save_context_ownership_at(
            &self.context_ownership_path,
            manifest,
        )
    }
}

impl crate::ProviderImportEnvironment for SystemProviderEnvironment {
    fn ccs_db_path(&self) -> &Path {
        &self.ccs_db_path
    }

    fn pending_import_path(&self) -> &Path {
        &self.pending_import_path
    }
}

impl crate::RelayEnvironmentEnvironment for SystemProviderEnvironment {
    fn environment_codex_home(&self) -> &Path {
        &self.codex_home
    }

    fn environment_backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    fn process_only_env_cleanup(&self) -> bool {
        self.process_only_env_cleanup
    }

    fn isolated_environment_inspection(&self) -> bool {
        self.process_only_env_cleanup
    }
}

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn isolated_codex_home_for_settings(settings_path: &Path) -> PathBuf {
    settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("codex")
}
