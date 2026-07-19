use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use codex_plus_core::install::{EntryPointState, ShortcutState};
use codex_plus_core::relay_config::{relay_profile_base_url, test_relay_profile};
use codex_plus_core::settings::{BackendSettings, SettingsStore};
use codex_plus_core::settings::{RelayProfile, RelayProtocol};
use codex_plus_data::{BackupStore, SQLiteStorageAdapter};
use serde_json::Value;

use crate::{
    NetworkModelsResponse, NetworkTestResponse, PluginMarketplaceCompatibilityWorkspace,
    PluginMarketplaceEnvironment, PluginMarketplaceError, PluginMarketplaceErrorKind,
    PluginMarketplaceKind, PluginMarketplaceRepair, PluginMarketplaceRepairOutcome,
    PluginMarketplaceRevision, ProviderActivationEnvironment, ProviderEnvironment,
    ProviderEnvironmentNetworkError, ProviderNetworkEnvironment, ProviderNetworkFailureKind,
};

#[derive(Clone)]
pub struct SystemProviderEnvironment {
    settings: SettingsStore,
    settings_path: PathBuf,
    codex_home: PathBuf,
    context_ownership_path: PathBuf,
    ccs_db_path: PathBuf,
    pending_import_path: PathBuf,
    backup_dir: PathBuf,
    user_script_builtin_dir: PathBuf,
    user_script_user_dir: PathBuf,
    user_script_config_path: PathBuf,
    zed_global_state_path: PathBuf,
    zed_registry_path: PathBuf,
    zed_sqlite_paths: Vec<PathBuf>,
    zed_launcher: Arc<dyn crate::ZedLaunchExecutor>,
    diagnostic_log_path: PathBuf,
    latest_status_path: PathBuf,
    watcher_disabled_flag_path: PathBuf,
    entrypoint_override: Option<codex_plus_core::install::EntryPointState>,
    codex_launcher: Arc<dyn crate::CodexLaunchExecutor>,
    stepwise_tester: Arc<dyn crate::StepwiseConnectionTester>,
    maintenance_isolated: bool,
    script_market_index_url: String,
    script_market_policy: codex_plus_core::script_market::MarketFetchPolicy,
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
        let state_dir = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let zed_global_state_path = codex_home.join(".codex-global-state.json");
        let zed_sqlite_paths =
            codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(&codex_home);
        let runtime = Arc::new(provider_runtime());
        Self {
            settings: SettingsStore::new(settings_path.clone()),
            settings_path,
            codex_home,
            context_ownership_path,
            ccs_db_path,
            pending_import_path,
            backup_dir,
            user_script_builtin_dir: default_builtin_user_scripts_dir(),
            user_script_user_dir: state_dir.join("user_scripts"),
            user_script_config_path: state_dir.join("user_scripts.json"),
            zed_global_state_path,
            zed_registry_path: state_dir.join("zed_remote_projects.json"),
            zed_sqlite_paths,
            zed_launcher: Arc::new(SystemZedLaunchExecutor),
            diagnostic_log_path: state_dir.join("diagnostic.jsonl"),
            latest_status_path: state_dir.join("latest-status.json"),
            watcher_disabled_flag_path: state_dir.join("watcher.disabled"),
            entrypoint_override: None,
            codex_launcher: Arc::new(SystemCodexLaunchExecutor),
            stepwise_tester: Arc::new(SystemStepwiseConnectionTester {
                runtime: runtime.clone(),
            }),
            maintenance_isolated: false,
            script_market_index_url: configured_script_market_index_url(),
            script_market_policy: codex_plus_core::script_market::MarketFetchPolicy::https_only(),
            process_only_env_cleanup,
            runtime,
        }
    }

    pub fn with_context_ownership_path(mut self, path: PathBuf) -> Self {
        self.context_ownership_path = path;
        self
    }

    pub fn with_user_script_paths(
        mut self,
        builtin_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        config_path: impl Into<PathBuf>,
    ) -> Self {
        self.user_script_builtin_dir = builtin_dir.into();
        self.user_script_user_dir = user_dir.into();
        self.user_script_config_path = config_path.into();
        self
    }

    pub fn with_zed_remote_paths(
        mut self,
        global_state_path: impl Into<PathBuf>,
        registry_path: impl Into<PathBuf>,
        sqlite_paths: Vec<PathBuf>,
    ) -> Self {
        self.zed_global_state_path = global_state_path.into();
        self.zed_registry_path = registry_path.into();
        self.zed_sqlite_paths = sqlite_paths;
        self
    }

    pub fn with_zed_launch_record_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.zed_launcher = Arc::new(RecordingZedLaunchExecutor { path: path.into() });
        self
    }

    pub fn with_zed_launch_executor(mut self, launcher: Arc<dyn crate::ZedLaunchExecutor>) -> Self {
        self.zed_launcher = launcher;
        self
    }

    pub fn with_maintenance_paths(
        mut self,
        diagnostic_log_path: impl Into<PathBuf>,
        latest_status_path: impl Into<PathBuf>,
        watcher_disabled_flag_path: impl Into<PathBuf>,
    ) -> Self {
        self.diagnostic_log_path = diagnostic_log_path.into();
        self.latest_status_path = latest_status_path.into();
        self.watcher_disabled_flag_path = watcher_disabled_flag_path.into();
        self.maintenance_isolated = true;
        self
    }

    pub fn with_entrypoint_override(
        mut self,
        silent_installed: bool,
        management_installed: bool,
    ) -> Self {
        use codex_plus_core::install::{EntryPointState, ShortcutState};

        self.entrypoint_override = Some(EntryPointState {
            silent_shortcut: ShortcutState {
                installed: silent_installed,
                path: None,
            },
            management_shortcut: ShortcutState {
                installed: management_installed,
                path: None,
            },
        });
        self.maintenance_isolated = true;
        self
    }

    pub fn with_codex_launch_record_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.codex_launcher = Arc::new(RecordingCodexLaunchExecutor {
            path: path.into(),
            calls: AtomicU64::new(0),
        });
        self.maintenance_isolated = true;
        self
    }

    pub fn with_codex_launch_executor(
        mut self,
        launcher: Arc<dyn crate::CodexLaunchExecutor>,
    ) -> Self {
        self.codex_launcher = launcher;
        self
    }

    pub fn with_stepwise_connection_tester(
        mut self,
        tester: Arc<dyn crate::StepwiseConnectionTester>,
    ) -> Self {
        self.stepwise_tester = tester;
        self
    }

    pub fn with_script_market_index_url(mut self, url: impl Into<String>) -> Self {
        self.script_market_index_url = url.into();
        self.script_market_policy = codex_plus_core::script_market::MarketFetchPolicy::https_only();
        self
    }

    pub fn with_loopback_script_market_for_tests(mut self, url: impl Into<String>) -> Self {
        self.script_market_index_url = url.into();
        self.script_market_policy =
            codex_plus_core::script_market::MarketFetchPolicy::loopback_http_for_tests();
        self
    }

    pub fn script_market_index_url(&self) -> &str {
        &self.script_market_index_url
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
        let user_script_builtin_dir = env_path("CODEX_PLUS_NATIVE_USER_SCRIPT_BUILTIN_DIR");
        let user_script_user_dir = env_path("CODEX_PLUS_NATIVE_USER_SCRIPT_USER_DIR");
        let user_script_config_path = env_path("CODEX_PLUS_NATIVE_USER_SCRIPT_CONFIG_PATH");
        let zed_global_state_path = env_path("CODEX_PLUS_NATIVE_ZED_GLOBAL_STATE_PATH");
        let zed_registry_path = env_path("CODEX_PLUS_NATIVE_ZED_REGISTRY_PATH");
        let zed_launch_record_path = env_path("CODEX_PLUS_NATIVE_ZED_LAUNCH_RECORD_PATH");
        let zed_override_present = zed_global_state_path.is_some()
            || zed_registry_path.is_some()
            || zed_launch_record_path.is_some();
        let diagnostic_log_path = env_path("CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH");
        let latest_status_path = env_path("CODEX_PLUS_NATIVE_LATEST_STATUS_PATH");
        let watcher_disabled_flag_path = env_path("CODEX_PLUS_NATIVE_WATCHER_DISABLED_FLAG_PATH");
        let entrypoint_silent_installed = env_bool("CODEX_PLUS_NATIVE_ENTRYPOINT_SILENT_INSTALLED");
        let entrypoint_management_installed =
            env_bool("CODEX_PLUS_NATIVE_ENTRYPOINT_MANAGEMENT_INSTALLED");
        let codex_launch_record_path = env_path("CODEX_PLUS_NATIVE_CODEX_LAUNCH_RECORD_PATH");
        let maintenance_override_present = diagnostic_log_path.is_some()
            || latest_status_path.is_some()
            || watcher_disabled_flag_path.is_some()
            || entrypoint_silent_installed.is_some()
            || entrypoint_management_installed.is_some()
            || codex_launch_record_path.is_some();
        let stepwise_test_record_path = env_path("CODEX_PLUS_NATIVE_STEPWISE_TEST_RECORD_PATH");
        let stepwise_test_result = std::env::var("CODEX_PLUS_NATIVE_STEPWISE_TEST_RESULT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let stepwise_override_present =
            stepwise_test_record_path.is_some() || stepwise_test_result.is_some();
        let script_market_index_url = std::env::var("CODEX_PLUS_SCRIPT_MARKET_INDEX_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let allow_loopback_script_market =
            std::env::var("CODEX_PLUS_NATIVE_SCRIPT_MARKET_ALLOW_LOOPBACK")
                .is_ok_and(|value| value == "1");
        let process_only_env_cleanup =
            std::env::var("CODEX_PLUS_NATIVE_ENV_PROCESS_ONLY").is_ok_and(|value| value == "1");
        if settings_path.is_none()
            && codex_home.is_none()
            && ccs_db_path.is_none()
            && pending_import_path.is_none()
            && backup_dir.is_none()
            && context_ownership_path.is_none()
            && user_script_builtin_dir.is_none()
            && user_script_user_dir.is_none()
            && user_script_config_path.is_none()
            && !zed_override_present
            && !maintenance_override_present
            && !stepwise_override_present
            && script_market_index_url.is_none()
            && !allow_loopback_script_market
        {
            return Self::default();
        }

        let zed_isolation_root = || {
            zed_global_state_path
                .as_deref()
                .or(zed_registry_path.as_deref())
                .or(zed_launch_record_path.as_deref())
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::temp_dir().join("codex-plus-native-zed"))
        };
        let maintenance_isolation_root = diagnostic_log_path
            .as_deref()
            .or(latest_status_path.as_deref())
            .or(watcher_disabled_flag_path.as_deref())
            .or(codex_launch_record_path.as_deref())
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| {
                settings_path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| std::env::temp_dir().join("codex-plus-native-maintenance"));
        let stepwise_isolation_root = stepwise_test_record_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| {
                settings_path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| std::env::temp_dir().join("codex-plus-native-stepwise"));
        let settings_path = settings_path.unwrap_or_else(|| {
            if zed_override_present {
                zed_isolation_root().join("settings.json")
            } else if maintenance_override_present {
                maintenance_isolation_root.join("settings.json")
            } else if stepwise_override_present {
                stepwise_isolation_root.join("settings.json")
            } else {
                codex_plus_core::paths::default_settings_path()
            }
        });
        let state_dir = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut environment = Self::for_manager_paths(
            settings_path.clone(),
            codex_home.unwrap_or_else(|| {
                if zed_override_present {
                    zed_isolation_root().join("codex")
                } else if maintenance_override_present {
                    maintenance_isolation_root.join("codex")
                } else if stepwise_override_present {
                    stepwise_isolation_root.join("codex")
                } else {
                    isolated_codex_home_for_settings(&settings_path)
                }
            }),
            ccs_db_path.unwrap_or_else(|| state_dir.join("cc-switch.db")),
            pending_import_path.unwrap_or_else(|| state_dir.join("pending-provider-import.json")),
            backup_dir.unwrap_or_else(|| state_dir.join("backups")),
            process_only_env_cleanup,
        );
        if zed_override_present {
            let global_state_path = zed_global_state_path
                .unwrap_or_else(|| environment.codex_home.join(".codex-global-state.json"));
            let registry_path =
                zed_registry_path.unwrap_or_else(|| state_dir.join("zed_remote_projects.json"));
            let sqlite_paths = environment.zed_sqlite_paths.clone();
            environment =
                environment.with_zed_remote_paths(global_state_path, registry_path, sqlite_paths);
            if let Some(path) = zed_launch_record_path {
                environment = environment.with_zed_launch_record_path(path);
            }
        }
        if maintenance_override_present {
            environment = environment.with_maintenance_paths(
                diagnostic_log_path
                    .unwrap_or_else(|| maintenance_isolation_root.join("diagnostic.jsonl")),
                latest_status_path
                    .unwrap_or_else(|| maintenance_isolation_root.join("latest-status.json")),
                watcher_disabled_flag_path
                    .unwrap_or_else(|| maintenance_isolation_root.join("watcher.disabled")),
            );
            if let (Some(silent), Some(management)) =
                (entrypoint_silent_installed, entrypoint_management_installed)
            {
                environment = environment.with_entrypoint_override(silent, management);
            }
            if let Some(path) = codex_launch_record_path {
                environment = environment.with_codex_launch_record_path(path);
            }
        }
        if stepwise_override_present {
            environment.stepwise_tester = Arc::new(RecordingStepwiseConnectionTester::new(
                stepwise_test_record_path,
                parse_recording_stepwise_result(stepwise_test_result.as_deref()),
            ));
        }
        if let Some(path) = context_ownership_path {
            environment = environment.with_context_ownership_path(path);
        }
        environment = environment.with_user_script_paths(
            user_script_builtin_dir.unwrap_or_else(default_builtin_user_scripts_dir),
            user_script_user_dir.unwrap_or_else(|| state_dir.join("user_scripts")),
            user_script_config_path.unwrap_or_else(|| state_dir.join("user_scripts.json")),
        );
        if let Some(url) = script_market_index_url {
            environment = if allow_loopback_script_market {
                environment.with_loopback_script_market_for_tests(url)
            } else {
                environment.with_script_market_index_url(url)
            };
        }
        environment
    }
}

impl Default for SystemProviderEnvironment {
    fn default() -> Self {
        let state_dir = codex_plus_core::paths::default_app_state_dir();
        let codex_home = codex_plus_core::relay_config::default_codex_home_dir();
        let user_script_config_dir =
            codex_plus_core::user_scripts::default_user_scripts_config_dir();
        let settings_path = codex_plus_core::paths::default_settings_path();
        let runtime = Arc::new(provider_runtime());
        Self {
            settings: SettingsStore::new(settings_path.clone()),
            settings_path,
            zed_global_state_path: codex_home.join(".codex-global-state.json"),
            zed_registry_path: state_dir.join("zed_remote_projects.json"),
            zed_sqlite_paths: codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(
                &codex_home,
            ),
            zed_launcher: Arc::new(SystemZedLaunchExecutor),
            diagnostic_log_path: codex_plus_core::paths::default_diagnostic_log_path(),
            latest_status_path: codex_plus_core::paths::default_latest_status_path(),
            watcher_disabled_flag_path: codex_plus_core::watcher::default_watcher_disabled_flag(),
            entrypoint_override: None,
            codex_launcher: Arc::new(SystemCodexLaunchExecutor),
            stepwise_tester: Arc::new(SystemStepwiseConnectionTester {
                runtime: runtime.clone(),
            }),
            maintenance_isolated: false,
            codex_home,
            context_ownership_path: codex_plus_core::paths::default_context_ownership_path(),
            ccs_db_path: codex_plus_core::ccs_import::default_ccs_db_path(),
            pending_import_path: codex_plus_core::paths::default_pending_provider_import_path(),
            backup_dir: state_dir.join("backups"),
            user_script_builtin_dir: default_builtin_user_scripts_dir(),
            user_script_user_dir: user_script_config_dir.join("user_scripts"),
            user_script_config_path: user_script_config_dir.join("user_scripts.json"),
            script_market_index_url: configured_script_market_index_url(),
            script_market_policy: codex_plus_core::script_market::MarketFetchPolicy::https_only(),
            process_only_env_cleanup: false,
            runtime,
        }
    }
}

impl crate::OverviewEnvironment for SystemProviderEnvironment {
    fn saved_codex_app_path(&self) -> String {
        self.settings.load().unwrap_or_default().codex_app_path
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        if self.maintenance_isolated && saved.trim().is_empty() {
            return None;
        }
        codex_plus_core::app_paths::resolve_codex_app_dir_with_saved(None, Some(saved))
    }

    fn codex_app_version(&self, path: &Path) -> Option<String> {
        codex_plus_core::app_paths::codex_app_version(path)
    }

    fn entrypoints(&self) -> EntryPointState {
        if let Some(state) = &self.entrypoint_override {
            return state.clone();
        }
        if self.maintenance_isolated {
            let missing = || ShortcutState {
                installed: false,
                path: None,
            };
            return EntryPointState {
                silent_shortcut: missing(),
                management_shortcut: missing(),
            };
        }
        codex_plus_core::install::inspect_entrypoints()
    }

    fn latest_launch(&self) -> Option<codex_plus_core::status::LaunchStatus> {
        codex_plus_core::status::StatusStore::new(self.latest_status_path.clone())
            .load_latest()
            .unwrap_or(None)
    }

    fn current_version(&self) -> String {
        codex_plus_core::version::VERSION.to_owned()
    }

    fn settings_path(&self) -> PathBuf {
        self.settings_path.clone()
    }

    fn logs_path(&self) -> PathBuf {
        self.diagnostic_log_path.clone()
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

impl crate::ZedRemoteEnvironment for SystemProviderEnvironment {
    fn load_zed_settings(&self) -> anyhow::Result<BackendSettings> {
        self.settings.load()
    }

    fn update_zed_settings_if<F>(
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

    fn load_zed_global_state(
        &self,
    ) -> Result<Option<Value>, codex_plus_core::zed_remote::ZedRemoteError> {
        let data = match fs::read_to_string(&self.zed_global_state_path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(codex_plus_core::zed_remote::ZedRemoteError::StateRead(
                    error,
                ));
            }
        };
        serde_json::from_str(&data)
            .map(Some)
            .map_err(codex_plus_core::zed_remote::ZedRemoteError::StateParse)
    }

    fn zed_request_context(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }

    fn zed_registry_store(&self) -> codex_plus_core::zed_remote::ZedRemoteRegistryStore {
        codex_plus_core::zed_remote::ZedRemoteRegistryStore::new(self.zed_registry_path.clone())
    }

    fn zed_sqlite_paths(&self) -> Vec<PathBuf> {
        self.zed_sqlite_paths.clone()
    }

    fn zed_availability(&self) -> codex_plus_core::zed_remote::ZedAvailability {
        self.zed_launcher
            .availability_override()
            .unwrap_or_else(codex_plus_core::zed_remote::zed_availability)
    }

    fn launch_zed_remote(
        &self,
        plan: &codex_plus_core::zed_remote::ZedLaunchPlan,
    ) -> Result<(), codex_plus_core::zed_remote::ZedRemoteError> {
        self.zed_launcher.launch(plan)
    }
}

struct SystemZedLaunchExecutor;

struct SystemCodexLaunchExecutor;

struct SystemStepwiseConnectionTester {
    runtime: Arc<tokio::runtime::Runtime>,
}

impl crate::StepwiseConnectionTester for SystemStepwiseConnectionTester {
    fn test(&self, settings: &BackendSettings) -> Result<usize, crate::StepwiseTestFailure> {
        match self
            .runtime
            .block_on(codex_plus_core::stepwise::test_connection(settings))
        {
            Ok(value) => map_stepwise_response(&value),
            Err(error) => {
                let detail = error.to_string().to_ascii_lowercase();
                if detail.contains("timed out") || detail.contains("timeout") {
                    Err(crate::StepwiseTestFailure::Timeout)
                } else {
                    Err(crate::StepwiseTestFailure::Network)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RecordingStepwiseResult {
    Ok(usize),
    Failed(crate::StepwiseTestFailure),
}

struct RecordingStepwiseConnectionTester {
    path: Option<PathBuf>,
    result: RecordingStepwiseResult,
    calls: AtomicU64,
}

impl RecordingStepwiseConnectionTester {
    fn new(path: Option<PathBuf>, result: RecordingStepwiseResult) -> Self {
        Self {
            path,
            result,
            calls: AtomicU64::new(0),
        }
    }
}

impl crate::StepwiseConnectionTester for RecordingStepwiseConnectionTester {
    fn test(&self, settings: &BackendSettings) -> Result<usize, crate::StepwiseTestFailure> {
        let call_count = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec_pretty(&serde_json::json!({
                "operation": "stepwise_test",
                "callCount": call_count,
                "enabled": settings.codex_app_stepwise_enabled,
                "baseUrlConfigured": !settings.codex_app_stepwise_base_url.trim().is_empty(),
                "directKeyConfigured": !settings.codex_app_stepwise_api_key.trim().is_empty(),
                "environmentNameConfigured": !settings.codex_app_stepwise_api_key_env.trim().is_empty(),
                "modelLength": settings.codex_app_stepwise_model.chars().count(),
                "maxItems": settings.codex_app_stepwise_max_items,
                "maxInputChars": settings.codex_app_stepwise_max_input_chars,
                "maxOutputTokens": settings.codex_app_stepwise_max_output_tokens,
                "timeoutMs": settings.codex_app_stepwise_timeout_ms,
            }))
            .map_err(|_| crate::StepwiseTestFailure::Network)?;
            write_recording_launch_atomically(path, &bytes)
                .map_err(|_| crate::StepwiseTestFailure::Network)?;
        }
        match self.result {
            RecordingStepwiseResult::Ok(item_count) => Ok(item_count),
            RecordingStepwiseResult::Failed(failure) => Err(failure),
        }
    }
}

fn map_stepwise_response(value: &Value) -> Result<usize, crate::StepwiseTestFailure> {
    if value.get("status").and_then(Value::as_str) == Some("ok") {
        return Ok(value
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len));
    }
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if error.contains("401")
        || error.contains("403")
        || error.contains("unauthorized")
        || error.contains("forbidden")
    {
        Err(crate::StepwiseTestFailure::Unauthorized)
    } else if error.contains("timed out") || error.contains("timeout") {
        Err(crate::StepwiseTestFailure::Timeout)
    } else {
        Err(crate::StepwiseTestFailure::Rejected)
    }
}

fn parse_recording_stepwise_result(value: Option<&str>) -> RecordingStepwiseResult {
    let value = value.unwrap_or("ok:0").trim().to_ascii_lowercase();
    if let Some(count) = value
        .strip_prefix("ok:")
        .and_then(|count| count.parse::<usize>().ok())
    {
        return RecordingStepwiseResult::Ok(count);
    }
    match value.as_str() {
        "ok" => RecordingStepwiseResult::Ok(0),
        "unauthorized" => RecordingStepwiseResult::Failed(crate::StepwiseTestFailure::Unauthorized),
        "timeout" => RecordingStepwiseResult::Failed(crate::StepwiseTestFailure::Timeout),
        "rejected" => RecordingStepwiseResult::Failed(crate::StepwiseTestFailure::Rejected),
        _ => RecordingStepwiseResult::Failed(crate::StepwiseTestFailure::Network),
    }
}

impl crate::CodexLaunchExecutor for SystemCodexLaunchExecutor {
    fn launch(&self, plan: &crate::CodexLaunchPlan) -> anyhow::Result<()> {
        codex_plus_core::install::spawn_companion(
            codex_plus_core::install::SILENT_BINARY,
            plan.arguments(),
        )
        .map(|_| ())
    }
}

struct RecordingCodexLaunchExecutor {
    path: PathBuf,
    calls: AtomicU64,
}

impl crate::CodexLaunchExecutor for RecordingCodexLaunchExecutor {
    fn launch(&self, plan: &crate::CodexLaunchPlan) -> anyhow::Result<()> {
        let call_count = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "operation": "launch",
            "callCount": call_count,
            "debugPort": plan.debug_port(),
            "helperPort": plan.helper_port(),
            "pathConfigured": plan.path_configured(),
            "argumentCount": plan.argument_count(),
        }))?;
        write_recording_launch_atomically(&self.path, &bytes)
            .map_err(|_| anyhow::anyhow!("failed to record Codex launch"))
    }
}

impl crate::ZedLaunchExecutor for SystemZedLaunchExecutor {
    fn launch(
        &self,
        plan: &codex_plus_core::zed_remote::ZedLaunchPlan,
    ) -> Result<(), codex_plus_core::zed_remote::ZedRemoteError> {
        codex_plus_core::zed_remote::launch_zed_remote_plan(plan)
    }
}

struct RecordingZedLaunchExecutor {
    path: PathBuf,
}

impl crate::ZedLaunchExecutor for RecordingZedLaunchExecutor {
    fn launch(
        &self,
        plan: &codex_plus_core::zed_remote::ZedLaunchPlan,
    ) -> Result<(), codex_plus_core::zed_remote::ZedRemoteError> {
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "strategy": plan.strategy(),
            "argumentCount": plan.args().len(),
        }))
        .map_err(|_| safe_recording_launch_error())?;
        write_recording_launch_atomically(&self.path, &bytes)
            .map_err(|_| safe_recording_launch_error())
    }

    fn availability_override(&self) -> Option<codex_plus_core::zed_remote::ZedAvailability> {
        Some(codex_plus_core::zed_remote::ZedAvailability {
            platform_supported: true,
            cli_found: true,
            app_found: false,
        })
    }
}

fn write_recording_launch_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temp_path = path.as_os_str().to_os_string();
    temp_path.push(".tmp");
    let temp_path = PathBuf::from(temp_path);
    fs::write(&temp_path, bytes)?;
    fs::rename(temp_path, path)
}

fn safe_recording_launch_error() -> codex_plus_core::zed_remote::ZedRemoteError {
    codex_plus_core::zed_remote::ZedRemoteError::Launch(std::io::Error::other(
        "failed to record Zed launch",
    ))
}

impl ProviderActivationEnvironment for SystemProviderEnvironment {
    fn settings_store(&self) -> &SettingsStore {
        &self.settings
    }

    fn codex_home(&self) -> &Path {
        &self.codex_home
    }
}

impl crate::SessionEnvironment for SystemProviderEnvironment {
    fn session_db_paths(&self) -> Vec<PathBuf> {
        codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(&self.codex_home)
    }

    fn list_local_sessions(
        &self,
        db_path: &Path,
    ) -> anyhow::Result<Vec<codex_plus_data::LocalSession>> {
        SQLiteStorageAdapter::new(db_path, BackupStore::new(&self.backup_dir)).list_local_sessions()
    }

    fn delete_local_from_paths(
        &self,
        db_paths: Vec<PathBuf>,
        session: &codex_plus_core::models::SessionRef,
    ) -> codex_plus_core::models::DeleteResult {
        codex_plus_data::delete_local_from_paths(
            db_paths,
            BackupStore::new(&self.backup_dir),
            session,
        )
    }
}

impl crate::UserScriptEnvironment for SystemProviderEnvironment {
    type Prepared = codex_plus_core::script_market::PreparedMarketScript;

    fn inspect_local(&self) -> Result<crate::UserScriptWorkspace, crate::UserScriptError> {
        let inspection = self.user_script_manager().inspect().map_err(|error| {
            crate::UserScriptError::with_compatibility_detail(
                crate::UserScriptErrorKind::InspectFailed,
                format!("{error:#}"),
            )
        })?;
        Ok(crate::user_scripts::workspace_from_core(inspection))
    }

    fn refresh_market(
        &self,
    ) -> Result<crate::ScriptMarketCompatibilityWorkspace, crate::UserScriptError> {
        self.runtime
            .block_on(
                codex_plus_core::script_market::fetch_market_manifest_with_policy(
                    &self.script_market_index_url,
                    self.script_market_policy,
                ),
            )
            .map(crate::ScriptMarketCompatibilityWorkspace::from_manifest)
            .map_err(|error| map_script_market_error(error, true))
    }

    fn prepare_market_script(
        &self,
        script: &codex_plus_core::script_market::MarketScript,
    ) -> Result<Self::Prepared, crate::UserScriptError> {
        let content = self
            .runtime
            .block_on(codex_plus_core::script_market::download_script_with_policy(
                &script.script_url,
                self.script_market_policy,
            ))
            .map_err(|error| map_script_market_error(error, false))?;
        codex_plus_core::script_market::prepare_market_script_content(script, &content)
            .map_err(|error| map_script_market_error(error, false))
    }

    fn commit_market_script(
        &self,
        expected_revision: crate::UserScriptRevision,
        prepared: Self::Prepared,
    ) -> Result<crate::UserScriptMutationOutcome, crate::UserScriptError> {
        let manager = self.user_script_manager();
        let current = current_core_user_script_inspection(&manager, &expected_revision)?;
        manager
            .commit_market_script(&current.revision, &prepared)
            .map(service_outcome_from_core)
            .map_err(map_user_script_mutation_error)
    }

    fn set_global_enabled(
        &self,
        expected_revision: crate::UserScriptRevision,
        enabled: bool,
    ) -> Result<crate::UserScriptMutationOutcome, crate::UserScriptError> {
        let manager = self.user_script_manager();
        let current = current_core_user_script_inspection(&manager, &expected_revision)?;
        manager
            .set_global_enabled_if_revision(&current.revision, enabled)
            .map(service_outcome_from_core)
            .map_err(map_user_script_mutation_error)
    }

    fn set_script_enabled(
        &self,
        expected_revision: crate::UserScriptRevision,
        key: &str,
        enabled: bool,
    ) -> Result<crate::UserScriptMutationOutcome, crate::UserScriptError> {
        let manager = self.user_script_manager();
        let current = current_core_user_script_inspection(&manager, &expected_revision)?;
        manager
            .set_script_enabled_if_revision(&current.revision, key, enabled)
            .map(service_outcome_from_core)
            .map_err(map_user_script_mutation_error)
    }

    fn delete_user_script(
        &self,
        expected_revision: crate::UserScriptRevision,
        key: &str,
    ) -> Result<crate::UserScriptMutationOutcome, crate::UserScriptError> {
        let manager = self.user_script_manager();
        let current = current_core_user_script_inspection(&manager, &expected_revision)?;
        manager
            .delete_user_script_with_backup(&current.revision, key)
            .map(service_outcome_from_core)
            .map_err(map_user_script_mutation_error)
    }
}

impl SystemProviderEnvironment {
    fn user_script_manager(&self) -> codex_plus_core::user_scripts::UserScriptManager {
        codex_plus_core::user_scripts::UserScriptManager::new(
            &self.user_script_builtin_dir,
            &self.user_script_user_dir,
            &self.user_script_config_path,
        )
        .with_backup_root(&self.backup_dir)
    }
}

fn current_core_user_script_inspection(
    manager: &codex_plus_core::user_scripts::UserScriptManager,
    expected: &crate::UserScriptRevision,
) -> Result<codex_plus_core::user_scripts::UserScriptInspection, crate::UserScriptError> {
    let inspection = manager.inspect().map_err(|error| {
        crate::UserScriptError::with_compatibility_detail(
            crate::UserScriptErrorKind::InspectFailed,
            format!("{error:#}"),
        )
    })?;
    let current = crate::UserScriptRevision::from_digest(inspection.revision.digest());
    if &current != expected {
        return Err(crate::UserScriptError::new(
            crate::UserScriptErrorKind::Conflict,
        ));
    }
    Ok(inspection)
}

fn service_outcome_from_core(
    outcome: codex_plus_core::user_scripts::UserScriptMutationOutcome,
) -> crate::UserScriptMutationOutcome {
    crate::UserScriptMutationOutcome {
        workspace: crate::user_scripts::workspace_from_core(outcome.inspection),
        backup: crate::UserScriptBackupEvidence {
            id: outcome.backup.id,
            created: outcome.backup.created,
        },
    }
}

fn map_user_script_mutation_error(
    error: codex_plus_core::user_scripts::UserScriptMutationError,
) -> crate::UserScriptError {
    use codex_plus_core::user_scripts::UserScriptMutationErrorKind as CoreKind;

    let kind = match error.kind() {
        CoreKind::InspectFailed => crate::UserScriptErrorKind::InspectFailed,
        CoreKind::Conflict => crate::UserScriptErrorKind::Conflict,
        CoreKind::InvalidTarget => crate::UserScriptErrorKind::InvalidTarget,
        CoreKind::BackupFailed => crate::UserScriptErrorKind::BackupFailed,
        CoreKind::WriteFailed => crate::UserScriptErrorKind::WriteFailed,
        CoreKind::RollbackFailed => crate::UserScriptErrorKind::RollbackFailed,
    };
    crate::UserScriptError::with_compatibility_detail(kind, error.to_string())
}

fn map_script_market_error(
    error: codex_plus_core::script_market::ScriptMarketError,
    refreshing_manifest: bool,
) -> crate::UserScriptError {
    use codex_plus_core::script_market::ScriptMarketErrorKind as CoreKind;

    let kind = match error.kind() {
        CoreKind::ResponseTooLarge => crate::UserScriptErrorKind::DownloadTooLarge,
        CoreKind::InvalidIntegrity => crate::UserScriptErrorKind::InvalidIntegrity,
        CoreKind::IntegrityMismatch => crate::UserScriptErrorKind::IntegrityMismatch,
        CoreKind::InvalidUrl
        | CoreKind::InsecureTransport
        | CoreKind::RequestFailed
        | CoreKind::DecodeFailed => {
            if refreshing_manifest {
                crate::UserScriptErrorKind::MarketRefreshFailed
            } else {
                crate::UserScriptErrorKind::DownloadFailed
            }
        }
    };
    crate::UserScriptError::with_compatibility_detail(kind, error.to_string())
}

impl crate::ProviderSyncEnvironment for SystemProviderEnvironment {
    fn load_provider_sync_settings(&self) -> anyhow::Result<BackendSettings> {
        self.settings.load()
    }

    fn load_provider_sync_targets(&self) -> codex_plus_data::ProviderSyncTargetList {
        codex_plus_data::load_provider_sync_targets(Some(&self.codex_home))
    }

    fn run_provider_sync(&self, target: &str) -> codex_plus_data::ProviderSyncResult {
        codex_plus_data::run_provider_sync_with_target(Some(&self.codex_home), Some(target))
    }

    fn save_provider_sync_enabled(
        &self,
        expected: &crate::ProviderSyncRevision,
        enabled: bool,
    ) -> Result<(), crate::ProviderSyncError> {
        let _lock =
            codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&self.codex_home)
                .map_err(|error| {
                    crate::ProviderSyncError::with_compatibility_detail(
                        crate::ProviderSyncErrorKind::SettingsConflict,
                        format!("{error:#}"),
                    )
                })?;
        let expected = expected.clone();
        let updated = self
            .settings
            .update_if(
                serde_json::json!({"providerSyncEnabled": enabled}),
                move |current| crate::provider_sync::provider_sync_revision(current) == expected,
            )
            .map_err(|error| {
                crate::ProviderSyncError::with_compatibility_detail(
                    crate::ProviderSyncErrorKind::SettingsConflict,
                    format!("{error:#}"),
                )
            })?;
        if updated.is_none() {
            return Err(crate::ProviderSyncError::new(
                crate::ProviderSyncErrorKind::SettingsConflict,
            ));
        }
        Ok(())
    }

    fn save_provider_sync_target(&self, target: &str) -> Result<(), crate::ProviderSyncError> {
        let target = target.trim();
        if target.is_empty() {
            return Err(crate::ProviderSyncError::new(
                crate::ProviderSyncErrorKind::SyncFailed,
            ));
        }
        let _lock =
            codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&self.codex_home)
                .map_err(|error| {
                    crate::ProviderSyncError::with_compatibility_detail(
                        crate::ProviderSyncErrorKind::SyncFailed,
                        format!("{error:#}"),
                    )
                })?;
        let mut settings = self.settings.load().map_err(|error| {
            crate::ProviderSyncError::with_compatibility_detail(
                crate::ProviderSyncErrorKind::SyncFailed,
                format!("{error:#}"),
            )
        })?;
        settings.provider_sync_last_selected_provider = target.to_owned();
        let mut saved =
            crate::provider_sync::normalized_provider_ids(&settings.provider_sync_saved_providers);
        if !saved.iter().any(|item| item == target) {
            saved.push(target.to_owned());
        }
        saved.sort();
        settings.provider_sync_saved_providers = saved;
        self.settings.save(&settings).map_err(|error| {
            crate::ProviderSyncError::with_compatibility_detail(
                crate::ProviderSyncErrorKind::SyncFailed,
                format!("{error:#}"),
            )
        })
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

impl PluginMarketplaceEnvironment for SystemProviderEnvironment {
    type Preparation = codex_plus_core::plugin_marketplace::PreparedPluginMarketplace;

    fn inspect_plugin_marketplaces(
        &self,
    ) -> Result<PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceError> {
        let _lock = codex_plus_core::relay_config::acquire_relay_live_read_lock(&self.codex_home)
            .map_err(|error| {
            marketplace_error(PluginMarketplaceErrorKind::InspectFailed, error)
        })?;
        self.inspect_plugin_marketplaces_locked()
    }

    fn prepare_plugin_marketplace(
        &self,
        kind: PluginMarketplaceKind,
    ) -> Result<Self::Preparation, PluginMarketplaceError> {
        let result = match kind {
            PluginMarketplaceKind::Local => self.runtime.block_on(
                codex_plus_core::plugin_marketplace::prepare_local_plugin_marketplace(
                    &self.codex_home,
                ),
            ),
            PluginMarketplaceKind::Remote => {
                codex_plus_core::plugin_marketplace::prepare_remote_plugin_marketplace(
                    &self.codex_home,
                )
            }
        };
        result.map_err(|error| marketplace_preparation_error(kind, error))
    }

    fn commit_plugin_marketplace(
        &self,
        expected_revision: PluginMarketplaceRevision,
        kind: PluginMarketplaceKind,
        prepared: Self::Preparation,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        if prepared.kind() != kind {
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::Conflict,
            ));
        }
        let _lock =
            codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&self.codex_home)
                .map_err(|error| {
                    marketplace_error(PluginMarketplaceErrorKind::WriteFailed, error)
                })?;
        let current = self.inspect_plugin_marketplaces_locked()?;
        if current.workspace.revision != expected_revision {
            if !current.workspace.status(kind).needs_repair {
                return Ok(PluginMarketplaceRepair {
                    outcome: PluginMarketplaceRepairOutcome::AlreadyHealthy,
                    initialized: false,
                    configured: false,
                    workspace: current.workspace,
                });
            }
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::Conflict,
            ));
        }

        let result = codex_plus_core::plugin_marketplace::commit_prepared_plugin_marketplace(
            &self.codex_home,
            prepared,
        )
        .map_err(marketplace_commit_error)?;
        let fresh = self.inspect_plugin_marketplaces_locked()?;
        if fresh.workspace.status(kind).needs_repair {
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::WriteFailed,
            ));
        }
        let outcome = if result.initialized {
            PluginMarketplaceRepairOutcome::Initialized
        } else if result.configured {
            PluginMarketplaceRepairOutcome::Configured
        } else {
            PluginMarketplaceRepairOutcome::AlreadyHealthy
        };
        Ok(PluginMarketplaceRepair {
            outcome,
            initialized: result.initialized,
            configured: result.configured,
            workspace: fresh.workspace,
        })
    }
}

impl SystemProviderEnvironment {
    fn inspect_plugin_marketplaces_locked(
        &self,
    ) -> Result<PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceError> {
        let inspection =
            codex_plus_core::plugin_marketplace::inspect_plugin_marketplaces(&self.codex_home)
                .map_err(|error| {
                    marketplace_error(PluginMarketplaceErrorKind::InspectFailed, error)
                })?;
        Ok(
            crate::plugin_marketplace::compatibility_workspace_from_core(
                &self.codex_home,
                inspection,
            ),
        )
    }
}

fn marketplace_preparation_error(
    kind: PluginMarketplaceKind,
    error: anyhow::Error,
) -> PluginMarketplaceError {
    let detail = format!("{error:#}");
    let lower = detail.to_ascii_lowercase();
    let error_kind = if lower.contains("too large") {
        PluginMarketplaceErrorKind::DownloadTooLarge
    } else if lower.contains("zip")
        || lower.contains("archive")
        || lower.contains("escapes destination")
        || lower.contains("marketplace is invalid")
        || lower.contains("root mismatch")
    {
        PluginMarketplaceErrorKind::ArchiveInvalid
    } else if kind == PluginMarketplaceKind::Local
        && (lower.contains("download")
            || lower.contains("http")
            || lower.contains("network")
            || lower.contains("timed out")
            || lower.contains("timeout"))
    {
        PluginMarketplaceErrorKind::DownloadFailed
    } else {
        PluginMarketplaceErrorKind::WriteFailed
    };
    PluginMarketplaceError::with_compatibility_detail(error_kind, None, detail)
}

fn marketplace_commit_error(error: anyhow::Error) -> PluginMarketplaceError {
    let detail = format!("{error:#}");
    let lower = detail.to_ascii_lowercase();
    let kind = if lower.contains("zip")
        || lower.contains("archive")
        || lower.contains("marketplace is invalid")
        || lower.contains("root mismatch")
    {
        PluginMarketplaceErrorKind::ArchiveInvalid
    } else {
        PluginMarketplaceErrorKind::WriteFailed
    };
    PluginMarketplaceError::with_compatibility_detail(kind, None, detail)
}

fn marketplace_error(
    kind: PluginMarketplaceErrorKind,
    error: anyhow::Error,
) -> PluginMarketplaceError {
    PluginMarketplaceError::with_compatibility_detail(kind, None, format!("{error:#}"))
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

fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim() {
            "1" | "true" | "TRUE" => Some(true),
            "0" | "false" | "FALSE" => Some(false),
            _ => None,
        })
}

fn isolated_codex_home_for_settings(settings_path: &Path) -> PathBuf {
    settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("codex")
}

fn configured_script_market_index_url() -> String {
    std::env::var("CODEX_PLUS_SCRIPT_MARKET_INDEX_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| codex_plus_core::script_market::DEFAULT_MARKET_INDEX_URL.to_string())
}

impl crate::MaintenanceEnvironment for SystemProviderEnvironment {
    fn load_maintenance_settings(&self) -> anyhow::Result<BackendSettings> {
        self.settings.load()
    }

    fn update_maintenance_settings_if<F>(
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

    fn inspect_path(&self, path: &Path) -> anyhow::Result<crate::PathKind> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(crate::PathKind::File),
            Ok(metadata) if metadata.is_dir() => Ok(crate::PathKind::Directory),
            Ok(_) => Ok(crate::PathKind::Other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(crate::PathKind::Missing)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        if self.maintenance_isolated && saved.trim().is_empty() {
            return None;
        }
        codex_plus_core::app_paths::resolve_codex_app_dir_with_saved(None, Some(saved))
    }

    fn codex_app_version(&self, path: &Path) -> Option<String> {
        codex_plus_core::app_paths::codex_app_version(path)
    }

    fn inspect_entrypoints(&self) -> anyhow::Result<codex_plus_core::install::EntryPointState> {
        if let Some(state) = &self.entrypoint_override {
            return Ok(state.clone());
        }
        if self.maintenance_isolated {
            anyhow::bail!("entrypoint inspection unavailable in isolated maintenance mode")
        }
        Ok(codex_plus_core::install::inspect_entrypoints())
    }

    fn watcher_disabled(&self) -> anyhow::Result<bool> {
        self.watcher_disabled_flag_path
            .try_exists()
            .map_err(Into::into)
    }

    fn load_latest_launch(&self) -> anyhow::Result<Option<codex_plus_core::status::LaunchStatus>> {
        codex_plus_core::status::StatusStore::new(self.latest_status_path.clone()).load_latest()
    }

    fn read_diagnostic_tail(&self, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut file = fs::File::open(&self.diagnostic_log_path)?;
        let length = file.metadata()?.len();
        let max_bytes_u64 = u64::try_from(max_bytes).unwrap_or(u64::MAX);
        file.seek(SeekFrom::Start(length.saturating_sub(max_bytes_u64)))?;
        let capacity = usize::try_from(length.min(max_bytes_u64)).unwrap_or(max_bytes);
        let mut bytes = Vec::with_capacity(capacity);
        file.take(max_bytes_u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn diagnostic_path_presence(&self) -> crate::DiagnosticPathPresence {
        crate::DiagnosticPathPresence {
            settings: self.settings_path.try_exists().unwrap_or(false),
            logs: self.diagnostic_log_path.try_exists().unwrap_or(false),
            latest_status: self.latest_status_path.try_exists().unwrap_or(false),
        }
    }

    fn launch_codex(&self, plan: &crate::CodexLaunchPlan) -> anyhow::Result<()> {
        self.codex_launcher.launch(plan)
    }
}

impl crate::ManagerSettingsEnvironment for SystemProviderEnvironment {
    fn load_manager_settings(&self) -> anyhow::Result<BackendSettings> {
        self.settings.load()
    }

    fn update_manager_settings_if<F>(
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

    fn inspect_path(&self, path: &Path) -> anyhow::Result<crate::PathKind> {
        crate::MaintenanceEnvironment::inspect_path(self, path)
    }

    fn environment_value_present(&self, name: &str) -> bool {
        std::env::var_os(name).is_some_and(|value| !value.to_string_lossy().trim().is_empty())
    }

    fn test_stepwise_candidate(
        &self,
        settings: &BackendSettings,
    ) -> Result<usize, crate::StepwiseTestFailure> {
        self.stepwise_tester.test(settings)
    }
}

fn default_builtin_user_scripts_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.join("user_scripts"))
        .unwrap_or_else(|| PathBuf::from("user_scripts"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use codex_plus_core::settings::BackendSettings;

    use crate::{
        LaunchCodex, MaintenanceEnvironment, MaintenanceService, ManagerSettingsEnvironment,
        OverviewEnvironment, PrivatePath,
    };

    use super::{
        RecordingStepwiseConnectionTester, RecordingStepwiseResult, SystemProviderEnvironment,
    };

    #[test]
    fn provider_system_maintenance_tail_is_bounded_and_isolation_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let settings_path = directory.path().join("settings.json");
        let log_path = directory.path().join("diagnostic.jsonl");
        let status_path = directory.path().join("latest-status.json");
        let watcher_path = directory.path().join("watcher.disabled");
        let mut log = std::fs::File::create(&log_path).unwrap();
        log.write_all(&vec![b'x'; 300 * 1024]).unwrap();
        log.write_all(b"\nlast-line\n").unwrap();
        drop(log);

        let environment = SystemProviderEnvironment::for_settings_path(settings_path)
            .with_maintenance_paths(log_path, status_path, watcher_path);

        let tail = environment.read_diagnostic_tail(256 * 1024).unwrap();
        assert_eq!(tail.len(), 256 * 1024);
        assert!(tail.ends_with(b"\nlast-line\n"));
        assert!(MaintenanceEnvironment::resolve_codex_app(&environment, "").is_none());
        assert!(environment.inspect_entrypoints().is_err());
    }

    #[test]
    fn provider_system_overview_uses_the_same_isolated_paths_and_entrypoint_override() {
        let directory = tempfile::tempdir().unwrap();
        let settings_path = directory.path().join("settings.json");
        let log_path = directory.path().join("diagnostic.jsonl");
        let status_path = directory.path().join("latest-status.json");
        let watcher_path = directory.path().join("watcher.disabled");
        let app_path = directory.path().join("Codex.exe");
        std::fs::write(&app_path, b"fixture").unwrap();
        std::fs::write(
            &settings_path,
            serde_json::to_vec(&serde_json::json!({
                "codexAppPath": app_path.to_string_lossy()
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(&log_path, b"").unwrap();
        codex_plus_core::status::StatusStore::new(status_path.clone())
            .save_latest(&codex_plus_core::status::LaunchStatus {
                status: "running".to_owned(),
                message: "fixture".to_owned(),
                started_at_ms: 123,
                debug_port: Some(9229),
                helper_port: Some(57321),
                codex_app: None,
            })
            .unwrap();

        let environment = SystemProviderEnvironment::for_settings_path(settings_path.clone())
            .with_maintenance_paths(log_path.clone(), status_path, watcher_path)
            .with_entrypoint_override(true, false);

        assert_eq!(
            OverviewEnvironment::saved_codex_app_path(&environment),
            app_path.to_string_lossy()
        );
        assert_eq!(
            OverviewEnvironment::settings_path(&environment),
            settings_path
        );
        assert_eq!(OverviewEnvironment::logs_path(&environment), log_path);
        let entrypoints = OverviewEnvironment::entrypoints(&environment);
        assert!(entrypoints.silent_shortcut.installed);
        assert!(!entrypoints.management_shortcut.installed);
        assert_eq!(
            OverviewEnvironment::latest_launch(&environment)
                .expect("isolated latest status")
                .debug_port,
            Some(9229)
        );
    }

    #[test]
    fn provider_system_recording_launcher_counts_calls_and_writes_only_safe_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let settings_path = directory.path().join("settings.json");
        let log_path = directory.path().join("diagnostic.jsonl");
        let status_path = directory.path().join("latest-status.json");
        let watcher_path = directory.path().join("watcher.disabled");
        let launch_record_path = directory.path().join("launch-record.json");
        let private_app_path = directory.path().join("private-app-path-sentinel");
        std::fs::create_dir_all(&private_app_path).unwrap();
        std::fs::write(&watcher_path, b"disabled").unwrap();

        let environment = SystemProviderEnvironment::for_settings_path(settings_path)
            .with_maintenance_paths(log_path, status_path, watcher_path)
            .with_entrypoint_override(true, false)
            .with_codex_launch_record_path(&launch_record_path);
        let entrypoints = environment.inspect_entrypoints().unwrap();
        assert!(entrypoints.silent_shortcut.installed);
        assert!(!entrypoints.management_shortcut.installed);
        assert!(environment.watcher_disabled().unwrap());
        let service = MaintenanceService::new(environment);

        service
            .launch(LaunchCodex::native(
                PrivatePath::new(private_app_path.to_string_lossy()),
                9229,
                57321,
            ))
            .unwrap();
        service
            .launch(LaunchCodex::native(
                PrivatePath::new(private_app_path.to_string_lossy()),
                9229,
                57321,
            ))
            .unwrap();

        let text = std::fs::read_to_string(&launch_record_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "operation": "launch",
                "callCount": 2,
                "debugPort": 9229,
                "helperPort": 57321,
                "pathConfigured": true,
                "argumentCount": 6,
            })
        );
        assert!(!text.contains("private-app-path-sentinel"));
    }

    #[test]
    fn provider_system_stepwise_recorder_excludes_credentials_urls_and_model_text() {
        let directory = tempfile::tempdir().unwrap();
        let record_path = directory.path().join("stepwise-record.json");
        let mut environment =
            SystemProviderEnvironment::for_settings_path(directory.path().join("settings.json"));
        environment.stepwise_tester = Arc::new(RecordingStepwiseConnectionTester::new(
            Some(record_path.clone()),
            RecordingStepwiseResult::Ok(2),
        ));
        let settings = BackendSettings {
            codex_app_stepwise_enabled: true,
            codex_app_stepwise_base_url: "https://private-stepwise.invalid/body-sentinel"
                .to_owned(),
            codex_app_stepwise_api_key: "private-key-sentinel".to_owned(),
            codex_app_stepwise_model: "private-model-sentinel".to_owned(),
            ..BackendSettings::default()
        };

        assert_eq!(environment.test_stepwise_candidate(&settings).unwrap(), 2);

        let text = std::fs::read_to_string(record_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["callCount"], 1);
        assert_eq!(value["enabled"], true);
        assert_eq!(value["directKeyConfigured"], true);
        assert_eq!(value["modelLength"], "private-model-sentinel".len());
        for forbidden in [
            "private-stepwise.invalid",
            "body-sentinel",
            "private-key-sentinel",
            "private-model-sentinel",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}
