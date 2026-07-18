use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use codex_plus_core::install::SILENT_BINARY;
use codex_plus_core::models::DeleteResult;
use codex_plus_core::relay_environment::RelayEnvironmentReport;
use codex_plus_core::script_market::{self, MarketScript, ScriptMarketManifest};
use codex_plus_core::settings::{BackendSettings, RelayProfile, SettingsStore};
use codex_plus_core::status::LaunchStatus;
use codex_plus_core::user_scripts::UserScriptManager;
use codex_plus_core::zed_remote::{ZedOpenStrategy, ZedRemoteProject};
use codex_plus_manager_service::{
    CompatContextDeleteRequest, CompatContextEntryRequest, ConfirmPendingImport,
    ContextToolsEnvironment, ContextToolsService, DiagnoseProviderProfile, DismissPendingImport,
    DoctorCheckStatus, DoctorDetailKind, DoctorOutcome, DoctorRecommendation, FetchProviderModels,
    ImportCcsProviders, OverviewSnapshot, OverviewSource, PluginMarketplaceCompatibilityWorkspace,
    PluginMarketplaceEnvironment, PluginMarketplaceKind, PluginMarketplaceRepairOutcome,
    PluginMarketplaceService, ProviderDoctorCheck as ServiceProviderDoctorCheck,
    ProviderDoctorCheckId, ProviderDoctorReport, ProviderImportService, ProviderImportSource,
    ProviderModelsResult, ProviderNetworkError, ProviderNetworkFailureKind,
    ProviderProfile as ServiceProviderProfile, ProviderService, ProviderSource,
    ProviderSyncService, ProviderSyncSource, ProviderTestOutcome, ProviderTestResult,
    RelayEnvironmentService, RelayEnvironmentSource, RemoveEnvironmentConflicts,
    RepairPluginMarketplace, ResourcePresence, RunProviderSync, SessionEnvironment, SessionService,
    SessionSource, SystemOverviewSource, SystemProviderEnvironment, TestProviderProfile,
    UpdateCheckState,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::install::{self, InstallActionResult, InstallOptions};

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult<T>
where
    T: Serialize,
{
    pub status: String,
    pub message: String,
    #[serde(flatten)]
    pub payload: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionPayload {
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathState {
    pub status: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewPayload {
    pub codex_app: PathState,
    pub codex_version: Option<String>,
    pub silent_shortcut: PathState,
    pub management_shortcut: PathState,
    pub latest_launch: Option<LaunchStatus>,
    pub current_version: String,
    pub update_status: String,
    pub settings_path: String,
    pub logs_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsPayload {
    pub settings: BackendSettings,
    pub settings_path: String,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceRepairPayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub initialized: bool,
    pub configured: bool,
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceStatusPayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub config_registered: bool,
    pub needs_repair: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePluginMarketplacePayload {
    pub codex_home: String,
    pub marketplace_root: Option<String>,
    pub config_registered: bool,
    pub needs_repair: bool,
    pub plugin_count: usize,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcsProvidersPayload {
    pub db_path: String,
    pub providers: Vec<codex_plus_core::ccs_import::CcsProviderImport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingProviderImportPayload {
    pub pending: Option<codex_plus_core::provider_import::ProviderImportRequest>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionsPayload {
    pub db_path: String,
    pub db_paths: Vec<String>,
    pub sessions: Vec<codex_plus_data::LocalSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteProjectsPayload {
    pub projects: Vec<ZedRemoteProject>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteOpenPayload {
    pub url: String,
    pub strategy: ZedOpenStrategy,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLocalSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPayload {
    pub authenticated: bool,
    pub auth_source: String,
    pub account_label: Option<String>,
    pub config_path: String,
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayFilesPayload {
    pub config_path: String,
    pub auth_path: String,
    pub config_contents: String,
    pub auth_contents: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySwitchPayload {
    pub settings: BackendSettings,
    pub relay: RelayPayload,
    pub settings_path: String,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBackfillPayload {
    pub settings: BackendSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntriesPayload {
    pub settings: BackendSettings,
    pub entries: codex_plus_core::relay_config::CodexContextEntries,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveContextEntriesPayload {
    pub entries: codex_plus_core::relay_config::CodexContextEntries,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRelayCommonConfigPayload {
    pub common_config_contents: String,
    pub profile_config_contents: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileTestPayload {
    pub http_status: u16,
    pub endpoint: String,
    pub response_preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepwiseTestPayload {
    pub item_count: usize,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileModelsPayload {
    pub models: Vec<String>,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorCheck {
    pub id: String,
    pub title: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDoctorPayload {
    pub profile_name: String,
    pub model: String,
    pub summary: String,
    pub recommendation: String,
    pub checks: Vec<ProviderDoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflictsPayload {
    pub conflicts: Vec<codex_plus_core::env_conflicts::EnvConflict>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEnvConflictsRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEnvConflictsPayload {
    pub removed: Vec<codex_plus_core::env_conflicts::EnvConflictRemoval>,
    pub backup_path: Option<String>,
    pub remaining: Vec<codex_plus_core::env_conflicts::EnvConflict>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRelayFileRequest {
    pub kind: String,
    pub contents: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillRelayProfileRequest {
    pub settings: BackendSettings,
    pub profile_id: String,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSettingsRequest {
    pub settings: BackendSettings,
}

impl std::fmt::Debug for ContextSettingsRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextSettingsRequest")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntryRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
    pub toml_body: String,
}

impl std::fmt::Debug for ContextEntryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextEntryRequest")
            .field("kind", &self.kind)
            .field("id", &self.id)
            .field("body_present", &!self.toml_body.is_empty())
            .field("body_length", &self.toml_body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDeleteRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
}

impl std::fmt::Debug for ContextDeleteRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextDeleteRequest")
            .field("kind", &self.kind)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRelayCommonConfigRequest {
    pub config_contents: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    #[serde(default)]
    pub app_path: String,
    #[serde(default = "default_debug_port")]
    pub debug_port: u16,
    #[serde(default = "default_helper_port")]
    pub helper_port: u16,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRequest {
    #[serde(default = "default_log_lines")]
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogsPayload {
    pub path: String,
    pub text: String,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsPayload {
    pub report: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatcherPayload {
    pub enabled: bool,
    pub disabled_flag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptMarketPayload {
    pub market: Value,
    pub user_scripts: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPayload {
    pub show_update: bool,
}

#[tauri::command]
pub fn backend_version() -> CommandResult<VersionPayload> {
    ok(
        "后端版本已读取。",
        VersionPayload {
            version: codex_plus_core::version::VERSION.to_string(),
        },
    )
}

#[tauri::command]
pub fn startup_options() -> CommandResult<StartupPayload> {
    ok(
        "启动参数已读取。",
        StartupPayload {
            show_update: startup_should_show_update(),
        },
    )
}

pub fn startup_should_show_update() -> bool {
    should_show_update(
        std::env::args(),
        std::env::var("CODEX_PLUS_SHOW_UPDATE").ok().as_deref(),
    )
}

fn should_show_update<I, S>(args: I, env_value: Option<&str>) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == "--show-update") || env_value == Some("1")
}

#[tauri::command]
pub async fn load_overview() -> CommandResult<OverviewPayload> {
    let payload = tauri::async_runtime::spawn_blocking(load_overview_payload).await;
    match payload {
        Ok(Ok(payload)) => ok("概览已加载。", payload),
        Ok(Err(_)) | Err(_) => failed("概览后台任务失败。", overview_failure_payload()),
    }
}

#[tauri::command]
pub fn launch_codex_plus(request: LaunchRequest) -> CommandResult<Value> {
    spawn_codex_plus_launch(request, "启动任务已在后台开始，可稍后查看概览状态。")
}

#[tauri::command]
pub fn restart_codex_plus(request: LaunchRequest) -> CommandResult<Value> {
    codex_plus_core::watcher::stop_launcher_processes_and_wait();
    codex_plus_core::watcher::stop_codex_processes_and_wait();
    spawn_codex_plus_launch(request, "Codex 已请求重启，启动任务正在后台运行。")
}

fn spawn_codex_plus_launch(request: LaunchRequest, accepted_message: &str) -> CommandResult<Value> {
    let debug_port = request.debug_port;
    let helper_port = request.helper_port;
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "manager.launch_requested",
        json!({
            "debug_port": debug_port,
            "helper_port": helper_port,
            "app_path": request.app_path.trim()
        }),
    );
    match spawn_silent_launcher(&request) {
        Ok(()) => CommandResult {
            status: "accepted".to_string(),
            message: accepted_message.to_string(),
            payload: json!({
                "debugPort": debug_port,
                "helperPort": helper_port
            }),
        },
        Err(error) => failed(
            &format!("启动静默入口失败：{error}"),
            json!({
                "debugPort": debug_port,
                "helperPort": helper_port
            }),
        ),
    }
}

fn spawn_silent_launcher(request: &LaunchRequest) -> anyhow::Result<()> {
    let mut args = Vec::new();
    if !request.app_path.trim().is_empty() {
        args.push("--app-path".to_string());
        args.push(request.app_path.trim().to_string());
    }
    args.push("--debug-port".to_string());
    args.push(request.debug_port.to_string());
    args.push("--helper-port".to_string());
    args.push(request.helper_port.to_string());
    codex_plus_core::install::spawn_companion(SILENT_BINARY, &args).map(|_| ())
}

#[tauri::command]
pub fn load_settings() -> CommandResult<SettingsPayload> {
    settings_payload("设置已加载。", "设置读取失败")
}

#[tauri::command]
pub fn save_settings(settings: BackendSettings) -> CommandResult<SettingsPayload> {
    let settings = normalize_settings_before_save(settings);
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match with_relay_live_mutation_lock(&home, || SettingsStore::default().save(&settings)) {
        Ok(()) => settings_payload("设置已保存。", "设置保存后重新读取失败"),
        Err(error) => failed(
            &format!("保存设置失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn load_ccs_providers() -> CommandResult<CcsProvidersPayload> {
    let service = system_provider_import_source();
    let db_path = service.ccs_source_path().to_path_buf();
    match service.load_ccs_records() {
        Ok(providers) => ok(
            &format!(
                "已读取 cc-switch Codex 供应商配置：{} 个。",
                providers.len()
            ),
            CcsProvidersPayload {
                db_path: db_path.to_string_lossy().to_string(),
                providers,
            },
        ),
        Err(error) => failed(
            &format!("读取 cc-switch 供应商配置失败：{error}"),
            CcsProvidersPayload {
                db_path: db_path.to_string_lossy().to_string(),
                providers: Vec::new(),
            },
        ),
    }
}

#[tauri::command]
pub fn import_ccs_providers() -> CommandResult<SettingsPayload> {
    let service = system_provider_import_source();
    let discovery = match service.discover_ccs() {
        Ok(discovery) => discovery,
        Err(error) => {
            let payload = settings_payload_value().unwrap_or_else(|(_, payload)| payload);
            return failed(&format!("读取 cc-switch 供应商配置失败：{error}"), payload);
        }
    };

    if discovery.importable_count == 0 {
        return settings_payload("没有新的 cc-switch 供应商配置需要导入。", "设置读取失败");
    }

    match service.import_ccs(ImportCcsProviders {
        source_revision: discovery.source_revision,
        provider_revision: discovery.provider_revision,
    }) {
        Ok(outcome) => settings_payload(
            &format!("已从 cc-switch 导入供应商配置：{} 个。", outcome.imported),
            "导入供应商配置后重新读取设置失败",
        ),
        Err(error) => failed(
            &format!("保存 cc-switch 供应商配置失败：{error}"),
            settings_payload_value().unwrap_or_else(|(_, payload)| payload),
        ),
    }
}

#[tauri::command]
pub fn load_pending_provider_import() -> CommandResult<PendingProviderImportPayload> {
    let service = system_provider_import_source();
    match service.load_pending_record() {
        Ok(pending) => ok(
            "待确认供应商导入已读取。",
            PendingProviderImportPayload { pending },
        ),
        Err(error) => failed(
            &format!("读取待确认供应商导入失败：{error}"),
            PendingProviderImportPayload { pending: None },
        ),
    }
}

#[tauri::command]
pub fn confirm_pending_provider_import() -> CommandResult<SettingsPayload> {
    let service = system_provider_import_source();
    let pending = match service.load_pending() {
        Ok(snapshot) => snapshot.pending,
        Err(error) => {
            let payload = settings_payload_value().unwrap_or_else(|(_, payload)| payload);
            return failed(&format!("读取待确认供应商导入失败：{error}"), payload);
        }
    };
    let Some(pending) = pending else {
        return settings_payload("没有待确认的供应商导入。", "设置读取失败");
    };
    let provider_revision = match service.current_provider_revision() {
        Ok(revision) => revision,
        Err(error) => {
            let payload = settings_payload_value().unwrap_or_else(|(_, payload)| payload);
            return failed(&format!("读取供应商配置失败：{error}"), payload);
        }
    };
    match service.confirm_pending(ConfirmPendingImport {
        pending_revision: pending.revision,
        provider_revision,
    }) {
        Ok(outcome) => {
            let profile_name = outcome.profile_name.unwrap_or_default();
            let message = if outcome.imported > 0 {
                format!("已导入供应商配置：{profile_name}。")
            } else {
                format!("供应商配置已存在：{profile_name}。")
            };
            settings_payload(&message, "供应商导入后重新读取设置失败")
        }
        Err(error) => failed(
            &format!("导入供应商配置失败：{error}"),
            settings_payload_value().unwrap_or_else(|(_, payload)| payload),
        ),
    }
}

#[tauri::command]
pub fn dismiss_pending_provider_import() -> CommandResult<PendingProviderImportPayload> {
    let service = system_provider_import_source();
    let pending = match service.load_pending() {
        Ok(snapshot) => snapshot.pending,
        Err(error) => {
            return failed(
                &format!("取消供应商导入失败：{error}"),
                PendingProviderImportPayload { pending: None },
            );
        }
    };
    let Some(pending) = pending else {
        return ok(
            "已取消供应商导入。",
            PendingProviderImportPayload { pending: None },
        );
    };
    match service.dismiss_pending(DismissPendingImport {
        pending_revision: pending.revision,
    }) {
        Ok(_) => ok(
            "已取消供应商导入。",
            PendingProviderImportPayload { pending: None },
        ),
        Err(error) => failed(
            &format!("取消供应商导入失败：{error}"),
            PendingProviderImportPayload { pending: None },
        ),
    }
}

#[tauri::command]
pub fn list_local_sessions() -> CommandResult<LocalSessionsPayload> {
    let service = SessionService::new(SystemProviderEnvironment::default());
    list_local_sessions_with_service(&service)
}

fn list_local_sessions_with_service(
    service: &dyn SessionSource,
) -> CommandResult<LocalSessionsPayload> {
    match service.load_workspace() {
        Ok(workspace) => {
            let payload = LocalSessionsPayload {
                db_path: workspace.db_paths.first().cloned().unwrap_or_default(),
                db_paths: workspace.db_paths.clone(),
                sessions: workspace
                    .sessions
                    .iter()
                    .map(|session| session.compatibility_local_session())
                    .collect(),
            };
            if workspace.read_issues.is_empty() {
                ok(
                    &format!("已读取 {} 个本地会话。", payload.sessions.len()),
                    payload,
                )
            } else {
                failed(
                    &format!(
                        "读取部分本地会话失败：{} 个数据库不可读。",
                        workspace.read_issues.len()
                    ),
                    payload,
                )
            }
        }
        Err(error) => failed(
            &format!("读取本地会话失败：{}", error.detail()),
            LocalSessionsPayload {
                db_path: String::new(),
                db_paths: Vec::new(),
                sessions: Vec::new(),
            },
        ),
    }
}

#[tauri::command]
pub fn list_zed_remote_projects() -> CommandResult<ZedRemoteProjectsPayload> {
    let result = codex_plus_core::zed_remote::list_zed_remote_projects_response(&json!({}));
    if result.get("status").and_then(Value::as_str) == Some("ok") {
        let projects = serde_json::from_value::<Vec<ZedRemoteProject>>(
            result
                .get("projects")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        )
        .unwrap_or_default();
        return ok(
            &format!("已读取 {} 个 Zed 远程项目。", projects.len()),
            ZedRemoteProjectsPayload { projects },
        );
    }
    failed(
        result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("读取 Zed 远程项目失败。"),
        ZedRemoteProjectsPayload {
            projects: Vec::new(),
        },
    )
}

#[tauri::command]
pub fn open_zed_remote(payload: Value) -> CommandResult<ZedRemoteOpenPayload> {
    let result = codex_plus_core::zed_remote::open_zed_remote(&payload);
    let strategy = result
        .get("strategy")
        .cloned()
        .and_then(|value| serde_json::from_value::<ZedOpenStrategy>(value).ok())
        .unwrap_or_default();
    let url = result
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if result.get("status").and_then(Value::as_str) == Some("ok") {
        return ok(
            "已在 Zed Remote 打开项目。",
            ZedRemoteOpenPayload { url, strategy },
        );
    }
    failed(
        result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("无法在 Zed Remote 打开项目。"),
        ZedRemoteOpenPayload { url, strategy },
    )
}

#[tauri::command]
pub fn forget_zed_remote_project(id: String) -> CommandResult<ZedRemoteProjectsPayload> {
    let result =
        codex_plus_core::zed_remote::forget_zed_remote_project_response(&json!({ "id": id }));
    if result.get("status").and_then(Value::as_str) != Some("ok") {
        return failed(
            result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("移除 Zed 远程项目失败。"),
            ZedRemoteProjectsPayload {
                projects: Vec::new(),
            },
        );
    }
    list_zed_remote_projects()
}

#[tauri::command]
pub fn delete_local_session(request: DeleteLocalSessionRequest) -> CommandResult<DeleteResult> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return failed(
            "会话 ID 不能为空。",
            DeleteResult {
                status: codex_plus_core::models::DeleteStatus::Failed,
                session_id: String::new(),
                message: "会话 ID 不能为空。".to_string(),
                undo_token: None,
                backup_path: None,
            },
        );
    }
    let environment = TauriSessionEnvironment::new(
        SystemProviderEnvironment::default(),
        request.db_path.map(PathBuf::from),
    );
    let service = SessionService::new(environment);
    log_manager_event(
        "manager.delete_local_session.start",
        json!({
            "session_id": session_id,
        }),
    );
    let result = delete_local_session_with_service(&service, session_id);
    log_manager_event(
        "manager.delete_local_session.finish",
        json!({
            "session_id": session_id,
            "final_status": format!("{:?}", result.payload.status),
        }),
    );
    result
}

fn delete_local_session_with_service(
    service: &dyn SessionSource,
    session_id: &str,
) -> CommandResult<DeleteResult> {
    let workspace = match service.load_workspace() {
        Ok(workspace) => workspace,
        Err(error) => return failed_delete_result(session_id, error.detail()),
    };
    let Some(session) = workspace
        .sessions
        .iter()
        .find(|session| session.id == session_id)
    else {
        return failed_delete_result(session_id, "Thread not found in local storage");
    };
    let outcome = match service.delete_sessions(codex_plus_manager_service::DeleteSessions {
        selections: vec![codex_plus_manager_service::DeleteSessionSelection {
            id: session.id.clone(),
            expected_revision: session.revision.clone(),
        }],
        confirmed_ids: vec![session.id.clone()],
    }) {
        Ok(outcome) => outcome,
        Err(error) => return failed_delete_result(session_id, error.detail()),
    };
    let Some(result) = outcome
        .outcomes
        .first()
        .map(|item| item.compatibility_delete_result())
    else {
        return failed_delete_result(session_id, "Session deletion produced no result");
    };
    let status = if result.status == codex_plus_core::models::DeleteStatus::LocalDeleted {
        "ok"
    } else {
        "failed"
    };
    CommandResult {
        status: status.to_owned(),
        message: result.message.clone(),
        payload: result,
    }
}

fn failed_delete_result(session_id: &str, message: &str) -> CommandResult<DeleteResult> {
    failed(
        message,
        DeleteResult {
            status: codex_plus_core::models::DeleteStatus::Failed,
            session_id: session_id.to_owned(),
            message: message.to_owned(),
            undo_token: None,
            backup_path: None,
        },
    )
}

#[derive(Clone)]
struct TauriSessionEnvironment {
    system: SystemProviderEnvironment,
    requested_db_path: Option<PathBuf>,
}

impl TauriSessionEnvironment {
    fn new(system: SystemProviderEnvironment, requested_db_path: Option<PathBuf>) -> Self {
        Self {
            system,
            requested_db_path,
        }
    }
}

impl SessionEnvironment for TauriSessionEnvironment {
    fn session_db_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.requested_db_path.iter().cloned().collect::<Vec<_>>();
        for path in SessionEnvironment::session_db_paths(&self.system) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    fn list_local_sessions(
        &self,
        db_path: &Path,
    ) -> anyhow::Result<Vec<codex_plus_data::LocalSession>> {
        SessionEnvironment::list_local_sessions(&self.system, db_path)
    }

    fn delete_local_from_paths(
        &self,
        db_paths: Vec<PathBuf>,
        session: &codex_plus_core::models::SessionRef,
    ) -> DeleteResult {
        SessionEnvironment::delete_local_from_paths(&self.system, db_paths, session)
    }
}

fn normalize_settings_before_save(mut settings: BackendSettings) -> BackendSettings {
    if let Some(path) =
        codex_plus_core::app_paths::normalize_codex_app_path(Path::new(&settings.codex_app_path))
    {
        settings.codex_app_path = path.to_string_lossy().to_string();
    }
    settings.relay_common_config_contents =
        codex_plus_core::relay_config::sanitize_common_config_contents(
            &settings.relay_common_config_contents,
        );
    let (common_without_context, extracted_context) =
        split_relay_context_config_sections(&settings.relay_common_config_contents);
    settings.relay_common_config_contents = common_without_context;
    settings.relay_context_config_contents =
        relay_join_config_sections(&[&settings.relay_context_config_contents, &extracted_context]);
    settings.relay_context_config_contents =
        codex_plus_core::relay_config::sanitize_common_config_contents(
            &settings.relay_context_config_contents,
        );
    for profile in &mut settings.relay_profiles {
        if let Err(error) =
            codex_plus_core::relay_config::normalize_relay_profile_for_storage(profile)
        {
            log_manager_event(
                "manager.normalize_relay_profile_for_storage.failed",
                json!({
                    "profileId": profile.id,
                    "profileName": profile.name,
                    "error": error.to_string()
                }),
            );
        }
    }
    let common_config = relay_combined_common_config(&settings);
    if !common_config.trim().is_empty() {
        for profile in &mut settings.relay_profiles {
            if !profile.use_common_config || profile.config_contents.trim().is_empty() {
                continue;
            }
            match codex_plus_core::relay_config::strip_common_config_from_config(
                &profile.config_contents,
                &common_config,
            ) {
                Ok(stripped) => {
                    profile.config_contents =
                        strip_common_config_text_fallback(&stripped, &common_config);
                }
                Err(_) => {
                    profile.config_contents =
                        strip_common_config_text_fallback(&profile.config_contents, &common_config);
                }
            }
        }
    }
    settings.provider_sync_saved_providers =
        normalize_provider_sync_provider_list(settings.provider_sync_saved_providers);
    settings.provider_sync_manual_providers =
        normalize_provider_sync_provider_list(settings.provider_sync_manual_providers);
    settings.provider_sync_last_selected_provider = settings
        .provider_sync_last_selected_provider
        .trim()
        .to_string();
    settings
}

fn normalize_provider_sync_provider_list(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }
    result.sort();
    result
}

fn relay_combined_common_config(settings: &BackendSettings) -> String {
    relay_join_config_sections(&[
        &settings.relay_common_config_contents,
        &settings.relay_context_config_contents,
    ])
}

fn relay_join_config_sections(sections: &[&str]) -> String {
    let sections = sections
        .iter()
        .map(|section| section.trim())
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>();
    if sections.is_empty() {
        String::new()
    } else {
        codex_plus_core::relay_config::normalize_config_text(&format!(
            "{}\n",
            sections.join("\n\n")
        ))
    }
}

fn split_relay_context_config_sections(config: &str) -> (String, String) {
    let mut common = Vec::new();
    let mut context = Vec::new();
    let mut in_context_table = false;

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_context_table = trimmed.starts_with("[mcp_servers.")
                || trimmed.starts_with("[skills.")
                || trimmed.starts_with("[plugins.");
        }
        if in_context_table {
            context.push(line);
        } else {
            common.push(line);
        }
    }

    (
        relay_join_config_sections(&[&common.join("\n")]),
        relay_join_config_sections(&[&context.join("\n")]),
    )
}

fn strip_common_config_text_fallback(config_contents: &str, common_config: &str) -> String {
    let common = common_config_anchors(common_config);
    if common.root_keys.is_empty() && common.table_headers.is_empty() {
        return ensure_text_newline(config_contents.trim_end());
    }

    let mut kept = Vec::new();
    let mut skipping_table = false;
    let mut in_root_section = true;
    let mut removed_root_keys = std::collections::HashSet::new();
    let source_root_keys = toml_root_keys_before_first_table(config_contents);

    for line in config_contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root_section = false;
            let header = trimmed.to_string();
            skipping_table = common.table_headers.contains(&header);
            if skipping_table {
                continue;
            }
        }

        if skipping_table {
            continue;
        }

        if in_root_section && let Some(key) = toml_key_from_line(trimmed) {
            if common.root_keys.contains(key) {
                let is_duplicate_common_key = removed_root_keys.contains(key)
                    || source_root_keys.contains(key)
                    || common.table_headers.contains("[features]")
                    || common
                        .table_headers
                        .contains("[marketplaces.openai-bundled]")
                    || common
                        .table_headers
                        .contains("[plugins.\"superpowers@openai-curated\"]");
                if is_duplicate_common_key {
                    removed_root_keys.insert(key.to_string());
                    continue;
                }
            }
        }

        kept.push(line);
    }

    ensure_text_newline(kept.join("\n").trim_end())
}

fn toml_root_keys_before_first_table(config_contents: &str) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    for line in config_contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if let Some(key) = toml_key_from_line(trimmed) {
            keys.insert(key.to_string());
        }
    }
    keys
}

struct CommonConfigAnchors {
    root_keys: std::collections::HashSet<String>,
    table_headers: std::collections::HashSet<String>,
}

fn common_config_anchors(common_config: &str) -> CommonConfigAnchors {
    let mut root_keys = std::collections::HashSet::new();
    let mut table_headers = std::collections::HashSet::new();
    let mut in_table = false;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_table = true;
            table_headers.insert(trimmed.to_string());
            continue;
        }
        if !in_table {
            if let Some(key) = toml_key_from_line(trimmed) {
                root_keys.insert(key.to_string());
            }
        }
    }

    CommonConfigAnchors {
        root_keys,
        table_headers,
    }
}

fn toml_key_from_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() { None } else { Some(key) }
}

fn ensure_text_newline(value: &str) -> String {
    if value.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n", value.trim_end())
    }
}

#[tauri::command]
pub async fn load_provider_sync_targets() -> CommandResult<Value> {
    let service = ProviderSyncService::new(SystemProviderEnvironment::default());
    let result =
        tauri::async_runtime::spawn_blocking(move || service.load_provider_sync_workspace()).await;
    match result {
        Ok(Ok(workspace)) => ok(
            "Provider 同步目标已加载。",
            serde_json::to_value(workspace.targets).unwrap_or_else(|_| json!({})),
        ),
        Ok(Err(error)) => failed(
            &format!("Provider 同步目标加载失败：{}", error.detail()),
            json!({}),
        ),
        Err(error) => failed(&format!("Provider 同步目标加载失败：{error}"), json!({})),
    }
}

#[tauri::command]
pub async fn sync_providers_now(target_provider: Option<String>) -> CommandResult<Value> {
    let target_provider = target_provider
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let service = ProviderSyncService::new(SystemProviderEnvironment::default());
    let result = tauri::async_runtime::spawn_blocking(move || {
        sync_providers_now_with_service(&service, target_provider)
    })
    .await;
    match result {
        Ok(result) => result,
        Err(error) => failed(&format!("供应商同步失败：{error}"), json!({})),
    }
}

fn sync_providers_now_with_service(
    service: &dyn ProviderSyncSource,
    target_provider: Option<String>,
) -> CommandResult<Value> {
    let workspace = match service.load_provider_sync_workspace() {
        Ok(workspace) => workspace,
        Err(error) => {
            return failed(&format!("供应商同步失败：{}", error.detail()), json!({}));
        }
    };
    let target = target_provider.unwrap_or(workspace.targets.current_provider);
    match service.run_provider_sync(RunProviderSync {
        target_provider: target.clone(),
        confirmed_target_provider: target,
    }) {
        Ok(outcome) => ok(
            &format!(
                "供应商已同步一次：{} 个会话文件，{} 行索引，跳过 {} 个占用文件。",
                outcome.result.changed_session_files,
                outcome.result.sqlite_rows_updated,
                outcome.result.skipped_locked_rollout_files.len()
            ),
            provider_sync_result_payload(&outcome.result),
        ),
        Err(error) => failed(&format!("供应商同步失败：{}", error.detail()), json!({})),
    }
}

fn provider_sync_result_payload(sync: &codex_plus_data::ProviderSyncResult) -> Value {
    json!({
        "syncStatus": sync.status,
        "targetProvider": sync.target_provider,
        "changedSessionFiles": sync.changed_session_files,
        "skippedLockedRolloutFiles": sync.skipped_locked_rollout_files,
        "sqliteRowsUpdated": sync.sqlite_rows_updated,
        "sqliteProviderRowsUpdated": sync.sqlite_provider_rows_updated,
        "sqliteUserEventRowsUpdated": sync.sqlite_user_event_rows_updated,
        "sqliteCwdRowsUpdated": sync.sqlite_cwd_rows_updated,
        "updatedWorkspaceRoots": sync.updated_workspace_roots,
        "encryptedContentWarning": sync.encrypted_content_warning,
        "backupDir": sync.backup_dir,
        "syncMessage": sync.message,
    })
}

#[tauri::command]
pub async fn refresh_script_market() -> CommandResult<ScriptMarketPayload> {
    match script_market::fetch_market_manifest(script_market::DEFAULT_MARKET_INDEX_URL).await {
        Ok(manifest) => ok(
            "脚本市场已刷新。",
            script_market_payload_from_manifest(&manifest, "ok", "脚本市场已刷新。"),
        ),
        Err(error) => failed(
            &format!("脚本市场加载失败：{error}"),
            failed_script_market_payload(&format!("脚本市场加载失败：{error}")),
        ),
    }
}

#[tauri::command]
pub async fn install_market_script(id: String) -> CommandResult<ScriptMarketPayload> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return failed(
            "脚本 id 不能为空。",
            failed_script_market_payload("脚本 id 不能为空。"),
        );
    }
    let manifest =
        match script_market::fetch_market_manifest(script_market::DEFAULT_MARKET_INDEX_URL).await {
            Ok(manifest) => manifest,
            Err(error) => {
                return failed(
                    &format!("脚本市场加载失败：{error}"),
                    failed_script_market_payload(&format!("脚本市场加载失败：{error}")),
                );
            }
        };
    let Some(script) = manifest.scripts.iter().find(|script| script.id == trimmed) else {
        return failed(
            "市场清单中未找到该脚本。",
            script_market_payload_from_manifest(&manifest, "failed", "市场清单中未找到该脚本。"),
        );
    };
    let manager = default_user_script_manager();
    match script_market::install_market_script(&manager, script).await {
        Ok(()) => ok(
            "脚本已安装。",
            script_market_payload_from_manifest(&manifest, "ok", "脚本已安装。"),
        ),
        Err(error) => failed(
            &format!("安装脚本失败：{error}"),
            script_market_payload_from_manifest(
                &manifest,
                "failed",
                &format!("安装脚本失败：{error}"),
            ),
        ),
    }
}

#[tauri::command]
pub fn set_user_script_enabled(key: String, enabled: bool) -> CommandResult<SettingsPayload> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return failed("脚本 key 不能为空。", fallback_settings_payload());
    }
    let manager = default_user_script_manager();
    match manager.set_script_enabled(trimmed, enabled) {
        Ok(_) => settings_payload(
            if enabled {
                "脚本已启用。"
            } else {
                "脚本已禁用。"
            },
            "脚本启停失败",
        ),
        Err(error) => failed(
            &format!("脚本启停失败：{error}"),
            fallback_settings_payload(),
        ),
    }
}

#[tauri::command]
pub fn delete_user_script(key: String) -> CommandResult<SettingsPayload> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return failed("脚本 key 不能为空。", fallback_settings_payload());
    }
    let manager = default_user_script_manager();
    match manager.delete_user_script(trimmed) {
        Ok(_) => settings_payload("脚本已删除。", "脚本删除失败"),
        Err(error) => failed(
            &format!("脚本删除失败：{error}"),
            fallback_settings_payload(),
        ),
    }
}

#[tauri::command]
pub fn open_external_url(url: String) -> CommandResult<Value> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return failed("只允许打开 http 或 https 链接。", json!({}));
    }
    match open_url(trimmed) {
        Ok(()) => ok("已在系统浏览器打开链接。", json!({ "url": trimmed })),
        Err(error) => failed(&format!("打开链接失败：{error}"), json!({ "url": trimmed })),
    }
}

#[tauri::command]
pub async fn install_entrypoints() -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(install::install_entrypoints)
        .await
        .unwrap_or_else(|error| install_background_failure("安装入口", error))
}

#[tauri::command]
pub async fn uninstall_entrypoints(options: InstallOptions) -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(move || install::uninstall_entrypoints(options))
        .await
        .unwrap_or_else(|error| install_background_failure("卸载入口", error))
}

#[tauri::command]
pub async fn repair_shortcuts() -> InstallActionResult {
    tauri::async_runtime::spawn_blocking(install::repair_shortcuts)
        .await
        .unwrap_or_else(|error| install_background_failure("修复快捷方式", error))
}

#[tauri::command]
pub fn plugin_marketplace_status() -> CommandResult<PluginMarketplaceStatusPayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    plugin_marketplace_status_with_service(system_plugin_marketplace_source(), &home)
}

fn plugin_marketplace_status_with_service<E: PluginMarketplaceEnvironment>(
    service: &PluginMarketplaceService<E>,
    fallback_home: &Path,
) -> CommandResult<PluginMarketplaceStatusPayload> {
    let snapshot = service.inspect_compatibility().ok();
    let config_registered = snapshot
        .as_ref()
        .is_some_and(local_compatibility_config_registered);
    let marketplace_root =
        compatibility_marketplace_root(snapshot.as_ref(), PluginMarketplaceKind::Local);
    let needs_repair = marketplace_root.is_none() || !config_registered;
    ok(
        if needs_repair {
            "插件市场需要初始化或注册。"
        } else {
            "插件市场已可用。"
        },
        PluginMarketplaceStatusPayload {
            codex_home: compatibility_codex_home(snapshot.as_ref(), fallback_home),
            marketplace_root,
            config_registered,
            needs_repair,
        },
    )
}

#[tauri::command]
pub async fn repair_plugin_marketplace() -> CommandResult<PluginMarketplaceRepairPayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    let worker_home = home.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repair_plugin_marketplace_with_service(system_plugin_marketplace_source(), &worker_home)
    })
    .await
    .unwrap_or_else(|error| {
        failed(
            &format!("插件市场修复失败：{error}"),
            PluginMarketplaceRepairPayload {
                codex_home: home.to_string_lossy().to_string(),
                marketplace_root: None,
                initialized: false,
                configured: false,
                needs_repair: true,
            },
        )
    })
}

fn repair_plugin_marketplace_with_service<E: PluginMarketplaceEnvironment>(
    service: &PluginMarketplaceService<E>,
    fallback_home: &Path,
) -> CommandResult<PluginMarketplaceRepairPayload> {
    let initial = match service.inspect_compatibility() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return failed(
                &format!("插件市场修复失败：{}", compatibility_error_detail(&error)),
                PluginMarketplaceRepairPayload {
                    codex_home: fallback_home.to_string_lossy().to_string(),
                    marketplace_root: None,
                    initialized: false,
                    configured: false,
                    needs_repair: true,
                },
            );
        }
    };
    let request = RepairPluginMarketplace {
        expected_revision: initial.workspace.revision.clone(),
        kind: PluginMarketplaceKind::Local,
        confirmed_kind: PluginMarketplaceKind::Local,
    };
    match service.repair(request) {
        Ok(result) => {
            let fresh = service.inspect_compatibility().ok();
            ok(
                if result.initialized {
                    "插件市场已从 openai/plugins 初始化并注册。"
                } else if result.configured {
                    "已注册本地插件市场。"
                } else {
                    "插件市场已可用，无需修复。"
                },
                PluginMarketplaceRepairPayload {
                    codex_home: compatibility_codex_home(Some(&initial), fallback_home),
                    marketplace_root: compatibility_marketplace_root(
                        fresh.as_ref().or(Some(&initial)),
                        PluginMarketplaceKind::Local,
                    ),
                    initialized: result.initialized,
                    configured: result.configured,
                    needs_repair: false,
                },
            )
        }
        Err(error) => {
            let fresh = service.inspect_compatibility().ok();
            failed(
                &format!("插件市场修复失败：{}", compatibility_error_detail(&error)),
                PluginMarketplaceRepairPayload {
                    codex_home: compatibility_codex_home(Some(&initial), fallback_home),
                    marketplace_root: compatibility_marketplace_root(
                        fresh.as_ref().or(Some(&initial)),
                        PluginMarketplaceKind::Local,
                    ),
                    initialized: false,
                    configured: false,
                    needs_repair: true,
                },
            )
        }
    }
}

#[tauri::command]
pub fn remote_plugin_marketplace_status() -> CommandResult<RemotePluginMarketplacePayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    remote_plugin_marketplace_status_with_service(system_plugin_marketplace_source(), &home)
}

fn remote_plugin_marketplace_status_with_service<E: PluginMarketplaceEnvironment>(
    service: &PluginMarketplaceService<E>,
    fallback_home: &Path,
) -> CommandResult<RemotePluginMarketplacePayload> {
    let snapshot = service.inspect_compatibility().ok();
    let status = snapshot.as_ref().map(|snapshot| &snapshot.workspace.remote);
    let needs_repair = status.is_none_or(|status| status.needs_repair);
    ok(
        if needs_repair {
            "官方远端插件缓存需要释放或注册。"
        } else {
            "官方远端插件缓存已可用。"
        },
        RemotePluginMarketplacePayload {
            codex_home: compatibility_codex_home(snapshot.as_ref(), fallback_home),
            marketplace_root: compatibility_marketplace_root(
                snapshot.as_ref(),
                PluginMarketplaceKind::Remote,
            ),
            config_registered: status.is_some_and(|status| status.config_registered),
            needs_repair,
            plugin_count: status.map_or(0, |status| status.plugin_count),
            skill_count: status.map_or(0, |status| status.skill_count),
        },
    )
}

#[tauri::command]
pub fn repair_remote_plugin_marketplace() -> CommandResult<RemotePluginMarketplacePayload> {
    let home = codex_plus_core::codex_home::default_codex_home_dir();
    repair_remote_plugin_marketplace_with_service(system_plugin_marketplace_source(), &home)
}

fn repair_remote_plugin_marketplace_with_service<E: PluginMarketplaceEnvironment>(
    service: &PluginMarketplaceService<E>,
    fallback_home: &Path,
) -> CommandResult<RemotePluginMarketplacePayload> {
    let initial = match service.inspect_compatibility() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return failed(
                &format!(
                    "官方远端插件缓存修复失败：{}",
                    compatibility_error_detail(&error)
                ),
                empty_remote_marketplace_payload(fallback_home),
            );
        }
    };
    let request = RepairPluginMarketplace {
        expected_revision: initial.workspace.revision.clone(),
        kind: PluginMarketplaceKind::Remote,
        confirmed_kind: PluginMarketplaceKind::Remote,
    };
    match service.repair(request) {
        Ok(result) => {
            let fresh = service.inspect_compatibility().ok();
            let snapshot = fresh.as_ref().unwrap_or(&initial);
            let status = &result.workspace.remote;
            ok(
                if result.outcome == PluginMarketplaceRepairOutcome::Initialized {
                    "已释放并注册内置官方远端插件缓存。"
                } else if result.outcome == PluginMarketplaceRepairOutcome::Configured {
                    "已注册官方远端插件缓存。"
                } else {
                    "官方远端插件缓存已可用，无需修复。"
                },
                RemotePluginMarketplacePayload {
                    codex_home: compatibility_codex_home(Some(snapshot), fallback_home),
                    marketplace_root: compatibility_marketplace_root(
                        Some(snapshot),
                        PluginMarketplaceKind::Remote,
                    ),
                    config_registered: status.config_registered,
                    needs_repair: status.needs_repair,
                    plugin_count: status.plugin_count,
                    skill_count: status.skill_count,
                },
            )
        }
        Err(error) => {
            let fresh = service.inspect_compatibility().ok();
            let snapshot = fresh.as_ref().unwrap_or(&initial);
            let status = &snapshot.workspace.remote;
            failed(
                &format!(
                    "官方远端插件缓存修复失败：{}",
                    compatibility_error_detail(&error)
                ),
                RemotePluginMarketplacePayload {
                    codex_home: compatibility_codex_home(Some(snapshot), fallback_home),
                    marketplace_root: compatibility_marketplace_root(
                        Some(snapshot),
                        PluginMarketplaceKind::Remote,
                    ),
                    config_registered: status.config_registered,
                    needs_repair: status.needs_repair,
                    plugin_count: status.plugin_count,
                    skill_count: status.skill_count,
                },
            )
        }
    }
}

fn local_compatibility_config_registered(
    snapshot: &PluginMarketplaceCompatibilityWorkspace,
) -> bool {
    snapshot.workspace.local.config_registered
        && (!snapshot.workspace.remote.available || snapshot.workspace.remote.config_registered)
}

fn compatibility_codex_home(
    snapshot: Option<&PluginMarketplaceCompatibilityWorkspace>,
    fallback_home: &Path,
) -> String {
    snapshot
        .map(PluginMarketplaceCompatibilityWorkspace::codex_home)
        .unwrap_or(fallback_home)
        .to_string_lossy()
        .to_string()
}

fn compatibility_marketplace_root(
    snapshot: Option<&PluginMarketplaceCompatibilityWorkspace>,
    kind: PluginMarketplaceKind,
) -> Option<String> {
    snapshot
        .and_then(|snapshot| snapshot.marketplace_root(kind))
        .map(|path| path.to_string_lossy().to_string())
}

fn compatibility_error_detail(error: &codex_plus_manager_service::PluginMarketplaceError) -> &str {
    error
        .compatibility_detail()
        .unwrap_or_else(|| error.detail())
}

fn empty_remote_marketplace_payload(fallback_home: &Path) -> RemotePluginMarketplacePayload {
    RemotePluginMarketplacePayload {
        codex_home: fallback_home.to_string_lossy().to_string(),
        marketplace_root: None,
        config_registered: false,
        needs_repair: true,
        plugin_count: 0,
        skill_count: 0,
    }
}

#[tauri::command]
pub async fn check_update() -> CommandResult<Value> {
    match codex_plus_core::update::check_for_update(codex_plus_core::version::VERSION).await {
        Ok(update) => {
            let status = if update.update_available {
                "ok"
            } else {
                "not_checked"
            };
            CommandResult {
                status: status.to_string(),
                message: if update.update_available {
                    "发现可用更新。".to_string()
                } else {
                    "当前已是最新版本。".to_string()
                },
                payload: json!({
                    "currentVersion": update.current_version,
                    "latestVersion": update.latest_version,
                    "releaseSummary": update.release_summary,
                    "assetName": update.asset_name,
                    "assetUrl": update.asset_url,
                    "updateAvailable": update.update_available,
                    "progress": 0
                }),
            }
        }
        Err(error) => failed(
            &format!("检查更新失败：{error}"),
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": Value::Null,
                "releaseSummary": "",
                "assetName": Value::Null,
                "assetUrl": Value::Null,
                "updateAvailable": false,
                "progress": 0
            }),
        ),
    }
}

#[tauri::command]
pub async fn perform_update(
    release: Option<codex_plus_core::update::Release>,
) -> CommandResult<Value> {
    let Some(release) = release else {
        return failed(
            "请先检查更新并选择可下载的 Release asset。",
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "progress": 0
            }),
        );
    };
    let download_dir = codex_plus_core::paths::default_app_state_dir().join("updates");
    match codex_plus_core::update::perform_update(&release, &download_dir).await {
        Ok(result) => ok(
            "安装包已下载并启动，请按安装向导完成更新。",
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": result.release.version,
                "releaseSummary": result.release.body,
                "installedPath": result.installer_path.to_string_lossy(),
                "launched": result.launched,
                "progress": 100
            }),
        ),
        Err(error) => failed(
            &format!("安装更新失败：{error}"),
            json!({
                "currentVersion": codex_plus_core::version::VERSION,
                "latestVersion": release.version,
                "releaseSummary": release.body,
                "progress": 0
            }),
        ),
    }
}

#[tauri::command]
pub fn load_watcher_state() -> CommandResult<WatcherPayload> {
    ok("watcher 状态已加载。", watcher_payload())
}

#[tauri::command]
pub fn install_watcher() -> CommandResult<WatcherPayload> {
    let launcher_path =
        codex_plus_core::install::companion_binary_path(codex_plus_core::install::SILENT_BINARY);
    match codex_plus_core::watcher::install_watcher(&launcher_path, default_debug_port()) {
        Ok(()) => ok("watcher 已安装。", watcher_payload()),
        Err(error) => failed(&format!("安装 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn uninstall_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::uninstall_watcher() {
        Ok(()) => ok("watcher 已移除。", watcher_payload()),
        Err(error) => failed(&format!("移除 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn enable_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::enable_watcher() {
        Ok(()) => ok("watcher 已启用。", watcher_payload()),
        Err(error) => failed(&format!("启用 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn disable_watcher() -> CommandResult<WatcherPayload> {
    match codex_plus_core::watcher::disable_watcher() {
        Ok(()) => ok("watcher 已禁用。", watcher_payload()),
        Err(error) => failed(&format!("禁用 watcher 失败：{error}"), watcher_payload()),
    }
}

#[tauri::command]
pub fn read_latest_logs(request: LogRequest) -> CommandResult<LogsPayload> {
    let path = codex_plus_core::paths::default_diagnostic_log_path();
    match read_tail(&path, request.lines) {
        Ok(text) => ok(
            "日志已读取。",
            LogsPayload {
                path: path.to_string_lossy().to_string(),
                text,
                lines: request.lines,
            },
        ),
        Err(error) => failed(
            &format!("读取日志失败：{error}"),
            LogsPayload {
                path: path.to_string_lossy().to_string(),
                text: String::new(),
                lines: request.lines,
            },
        ),
    }
}

#[tauri::command]
pub fn copy_diagnostics() -> CommandResult<DiagnosticsPayload> {
    ok(
        "诊断报告已生成。",
        DiagnosticsPayload {
            report: diagnostics_report(),
        },
    )
}

#[tauri::command]
pub fn reset_settings() -> CommandResult<SettingsPayload> {
    let settings = BackendSettings::default();
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match with_relay_live_mutation_lock(&home, || SettingsStore::default().save(&settings)) {
        Ok(()) => settings_payload("设置已重置为默认值。", "设置重置后重新读取失败"),
        Err(error) => failed(
            &format!("重置设置失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn reset_image_overlay_settings() -> CommandResult<SettingsPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 写锁失败：{error}"),
                settings_payload_value().unwrap_or_else(|(_, payload)| payload),
            );
        }
    };
    let store = SettingsStore::default();
    let mut settings = store.load().unwrap_or_default();
    let defaults = BackendSettings::default();
    settings.codex_app_image_overlay_enabled = defaults.codex_app_image_overlay_enabled;
    settings.codex_app_image_overlay_path = defaults.codex_app_image_overlay_path;
    settings.codex_app_image_overlay_opacity = defaults.codex_app_image_overlay_opacity;
    settings.codex_app_image_overlay_fit_mode = defaults.codex_app_image_overlay_fit_mode;
    let settings = normalize_settings_before_save(settings);
    match store.save(&settings) {
        Ok(()) => settings_payload("图片覆盖层设置已重置。", "图片覆盖层重置后重新读取失败"),
        Err(error) => failed(
            &format!("重置图片覆盖层失败：{error}"),
            SettingsPayload {
                settings,
                settings_path: codex_plus_core::paths::default_settings_path()
                    .to_string_lossy()
                    .to_string(),
                user_scripts: user_script_inventory(),
            },
        ),
    }
}

#[tauri::command]
pub fn relay_status() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let status = match with_relay_live_read_lock(&home, || {
        Ok(codex_plus_core::relay_config::relay_status_from_home(&home))
    }) {
        Ok(status) => status,
        Err(error) => {
            return failed(
                &format!("读取供应商 live 状态失败：{error}"),
                relay_payload(unavailable_relay_status(&home), None),
            );
        }
    };
    let message = if status.authenticated {
        "已检测到 ChatGPT 登录状态。"
    } else {
        "未检测到 ChatGPT 登录状态，请先在 Codex/ChatGPT 中正常登录。"
    };
    ok(message, relay_payload(status, None))
}

#[tauri::command]
pub fn read_relay_files() -> CommandResult<RelayFilesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match with_relay_live_read_lock(&home, || relay_files_payload_from_home(&home)) {
        Ok(payload) => ok("配置文件内容已读取。", payload),
        Err(error) => failed(
            &format!("读取配置文件失败：{error}"),
            RelayFilesPayload {
                config_path: home.join("config.toml").to_string_lossy().to_string(),
                auth_path: home.join("auth.json").to_string_lossy().to_string(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
        ),
    }
}

#[tauri::command]
pub fn check_env_conflicts() -> CommandResult<EnvConflictsPayload> {
    let service = system_relay_environment_source();
    match service.inspect() {
        Ok(workspace) => {
            let message = if workspace.conflicts.is_empty() {
                "未检测到会覆盖 Codex 供应商配置的 OPENAI 环境变量。"
            } else {
                "检测到可能覆盖 Codex 供应商配置的 OPENAI 环境变量。"
            };
            ok(
                message,
                EnvConflictsPayload {
                    conflicts: workspace.conflicts,
                },
            )
        }
        Err(error) => failed(
            &format!("检查环境变量失败：{error}"),
            EnvConflictsPayload {
                conflicts: codex_plus_core::env_conflicts::detect_env_conflicts(),
            },
        ),
    }
}

#[tauri::command]
pub fn check_relay_environment() -> CommandResult<RelayEnvironmentReport> {
    let service = system_relay_environment_source();
    match service.inspect() {
        Ok(workspace) => {
            let message = if workspace.report.all_passed() {
                "中转站环境配置检测全部通过。"
            } else {
                "检测到可能影响中转站配置的环境问题。"
            };
            ok(message, workspace.report)
        }
        Err(error) => failed(
            &format!("检查中转站环境失败：{error}"),
            codex_plus_core::relay_environment::inspect_relay_environment(),
        ),
    }
}

#[tauri::command]
pub fn remove_env_conflicts(
    request: RemoveEnvConflictsRequest,
) -> CommandResult<RemoveEnvConflictsPayload> {
    let service = system_relay_environment_source();
    let workspace = match service.inspect() {
        Ok(workspace) => workspace,
        Err(error) => {
            return failed(
                &format!("删除环境变量失败：{error}"),
                RemoveEnvConflictsPayload {
                    removed: Vec::new(),
                    backup_path: None,
                    remaining: codex_plus_core::env_conflicts::detect_env_conflicts(),
                },
            );
        }
    };
    match service.remove_conflicts(RemoveEnvironmentConflicts {
        expected_revision: workspace.revision,
        names: request.names,
    }) {
        Ok(result) => {
            let message = if result.failures.is_empty() {
                "环境变量已按确认项删除；重新启动 Codex 后生效。"
            } else {
                "部分环境变量未能删除；已保留失败项并完成其余清理。"
            };
            ok(
                message,
                RemoveEnvConflictsPayload {
                    removed: result.removed,
                    backup_path: result.backup_path,
                    remaining: result.remaining,
                },
            )
        }
        Err(error) => failed(
            &format!("删除环境变量失败：{error}"),
            RemoveEnvConflictsPayload {
                removed: Vec::new(),
                backup_path: None,
                remaining: codex_plus_core::env_conflicts::detect_env_conflicts(),
            },
        ),
    }
}

#[tauri::command]
pub fn save_relay_file(request: SaveRelayFileRequest) -> CommandResult<RelayFilesPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    match with_relay_live_mutation_lock(&home, || {
        save_relay_file_in_home(&home, &request.kind, &request.contents)
            .and_then(|_| relay_files_payload_from_home(&home))
    }) {
        Ok(payload) => ok("配置文件已保存。", payload),
        Err(error) => failed(
            &format!("保存配置文件失败：{error}"),
            with_relay_live_read_lock(&home, || relay_files_payload_from_home(&home))
                .unwrap_or_else(|_| RelayFilesPayload {
                    config_path: home.join("config.toml").to_string_lossy().to_string(),
                    auth_path: home.join("auth.json").to_string_lossy().to_string(),
                    config_contents: String::new(),
                    auth_contents: String::new(),
                }),
        ),
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfileSwitchRequest {
    pub settings: BackendSettings,
    #[serde(default)]
    pub previous_active_relay_id: String,
}

#[tauri::command]
pub fn switch_relay_profile(
    request: RelayProfileSwitchRequest,
) -> CommandResult<RelaySwitchPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let Ok(_guard) = relay_switch_mutex().lock() else {
        let status = with_relay_live_read_lock(&home, || {
            Ok(codex_plus_core::relay_config::relay_status_from_home(&home))
        })
        .unwrap_or_else(|_| unavailable_relay_status(&home));
        return failed(
            "供应商切换锁已损坏，请重启管理器后再试。",
            relay_switch_payload(
                SettingsStore::default().load().unwrap_or_default(),
                status,
                None,
            ),
        );
    };
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 写锁失败：{error}"),
                relay_switch_payload(
                    SettingsStore::default().load().unwrap_or_default(),
                    unavailable_relay_status(&home),
                    None,
                ),
            );
        }
    };
    let store = SettingsStore::default();
    let previous_active_relay_id = request.previous_active_relay_id;
    let settings = normalize_settings_before_save(request.settings);
    log_manager_event(
        "manager.switch_relay_profile.start",
        json!({
            "previousActiveRelayId": previous_active_relay_id,
            "targetRelayId": settings.active_relay_id
        }),
    );
    match codex_plus_core::relay_switch::switch_relay_profile_in_home(
        &store,
        &home,
        settings,
        &previous_active_relay_id,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.switch_relay_profile.ok",
                json!({
                    "targetRelayId": result.settings.active_relay_id,
                    "configured": status.configured,
                    "backupPath": result.backup_path.as_ref()
                }),
            );
            ok(
                "供应商已切换。",
                relay_switch_payload(result.settings, status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            let settings = store.load().unwrap_or_default();
            log_manager_event(
                "manager.switch_relay_profile.failed",
                json!({
                    "previousActiveRelayId": previous_active_relay_id,
                    "activeRelayId": settings.active_relay_id,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("供应商切换失败：{error}"),
                relay_switch_payload(settings, status, None),
            )
        }
    }
}

#[tauri::command]
pub fn write_diagnostic_event(event: String, detail: Value) -> CommandResult<Value> {
    let event = sanitize_manager_event(&event);
    match codex_plus_core::diagnostic_log::append_diagnostic_log(&event, detail) {
        Ok(()) => ok("诊断日志已写入。", json!({})),
        Err(error) => failed(&format!("写入诊断日志失败：{error}"), json!({})),
    }
}

#[tauri::command]
pub fn backfill_relay_profile_from_live(
    request: BackfillRelayProfileRequest,
) -> CommandResult<SettingsBackfillPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let mut settings = request.settings;
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_read_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 读锁失败：{error}"),
                SettingsBackfillPayload { settings },
            );
        }
    };
    let requested_profile_id = request.profile_id.clone();
    log_manager_event(
        "manager.backfill_relay_profile_from_live.start",
        json!({
            "profileId": requested_profile_id,
            "activeRelayId": settings.active_relay_id
        }),
    );
    let Some(profile) = settings
        .relay_profiles
        .iter_mut()
        .find(|profile| profile.id == request.profile_id)
    else {
        log_manager_event(
            "manager.backfill_relay_profile_from_live.missing_profile",
            json!({
                "profileId": requested_profile_id
            }),
        );
        return failed(
            "当前供应商已不在配置列表中，已停止切换以避免覆盖用户改动。",
            SettingsBackfillPayload { settings },
        );
    };

    match codex_plus_core::relay_config::backfill_relay_profile_from_home_with_common(
        &home,
        profile,
        &mut settings.relay_context_config_contents,
    ) {
        Ok(()) => {
            log_manager_event(
                "manager.backfill_relay_profile_from_live.ok",
                json!({
                    "profileId": requested_profile_id
                }),
            );
            ok(
                "当前供应商配置已从 live 文件回填。",
                SettingsBackfillPayload { settings },
            )
        }
        Err(error) => {
            log_manager_event(
                "manager.backfill_relay_profile_from_live.failed",
                json!({
                    "profileId": requested_profile_id,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("回填当前供应商配置失败：{error}"),
                SettingsBackfillPayload { settings },
            )
        }
    }
}

#[tauri::command]
pub fn list_context_entries(
    request: ContextSettingsRequest,
) -> CommandResult<ContextEntriesPayload> {
    let fallback = request.settings.clone();
    match system_context_tools_source().list_compat(request.settings) {
        Ok(result) => ok(
            "工具与插件列表已读取。",
            ContextEntriesPayload {
                settings: result.settings,
                entries: result.entries,
            },
        ),
        Err(error) => failed(
            &format!("读取工具与插件列表失败：{error}"),
            ContextEntriesPayload {
                settings: fallback,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn read_live_context_entries() -> CommandResult<LiveContextEntriesPayload> {
    read_live_context_entries_with_service(system_context_tools_source())
}

fn read_live_context_entries_with_service<E: ContextToolsEnvironment>(
    service: &ContextToolsService<E>,
) -> CommandResult<LiveContextEntriesPayload> {
    match service.read_live_compat() {
        Ok(entries) => ok(
            "live 工具与插件已读取。",
            LiveContextEntriesPayload { entries },
        ),
        Err(error) => failed(
            &format!("读取 live 工具与插件失败：{error}"),
            LiveContextEntriesPayload {
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn upsert_context_entry(request: ContextEntryRequest) -> CommandResult<ContextEntriesPayload> {
    let fallback = request.settings.clone();
    match system_context_tools_source().upsert_compat(CompatContextEntryRequest {
        settings: request.settings,
        kind: request.kind,
        id: request.id,
        toml_body: request.toml_body,
    }) {
        Ok(result) => ok(
            "工具与插件列表已读取。",
            ContextEntriesPayload {
                settings: result.settings,
                entries: result.entries,
            },
        ),
        Err(error) => failed(
            &format!("保存工具与插件失败：{error}"),
            ContextEntriesPayload {
                settings: fallback,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn sync_live_context_entries(
    request: ContextSettingsRequest,
) -> CommandResult<LiveContextEntriesPayload> {
    sync_live_context_entries_with_service(system_context_tools_source(), request)
}

fn sync_live_context_entries_with_service<E: ContextToolsEnvironment>(
    service: &ContextToolsService<E>,
    request: ContextSettingsRequest,
) -> CommandResult<LiveContextEntriesPayload> {
    match service.sync_all_global_compat(&request.settings) {
        Ok(entries) => ok(
            "live 工具与插件已同步。",
            LiveContextEntriesPayload { entries },
        ),
        Err(error) => failed(
            &format!("同步 live 工具与插件失败：{error}"),
            LiveContextEntriesPayload {
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn delete_context_entry(request: ContextDeleteRequest) -> CommandResult<ContextEntriesPayload> {
    let fallback = request.settings.clone();
    match system_context_tools_source().delete_compat(CompatContextDeleteRequest {
        settings: request.settings,
        kind: request.kind,
        id: request.id,
    }) {
        Ok(result) => ok(
            "工具与插件列表已读取。",
            ContextEntriesPayload {
                settings: result.settings,
                entries: result.entries,
            },
        ),
        Err(error) => failed(
            &format!("删除工具与插件失败：{error}"),
            ContextEntriesPayload {
                settings: fallback,
                entries: empty_context_entries(),
            },
        ),
    }
}

#[tauri::command]
pub fn extract_relay_common_config(
    request: ExtractRelayCommonConfigRequest,
) -> CommandResult<ExtractRelayCommonConfigPayload> {
    match codex_plus_core::relay_config::extract_common_config_from_config(&request.config_contents)
        .and_then(|common_config_contents| {
            let profile_config_contents =
                codex_plus_core::relay_config::strip_common_config_from_config(
                    &request.config_contents,
                    &common_config_contents,
                )?;
            Ok(ExtractRelayCommonConfigPayload {
                common_config_contents,
                profile_config_contents,
            })
        }) {
        Ok(payload) => ok("通用配置已按兼容切换规则提取。", payload),
        Err(error) => failed(
            &format!("提取通用配置失败：{error}"),
            ExtractRelayCommonConfigPayload {
                common_config_contents: String::new(),
                profile_config_contents: request.config_contents,
            },
        ),
    }
}

#[tauri::command]
pub async fn test_relay_profile(profile: RelayProfile) -> CommandResult<RelayProfileTestPayload> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let source = system_provider_source();
        let default_test_model = source
            .load_workspace()
            .map(|workspace| workspace.document.default_test_model)
            .unwrap_or_default();
        source.test_profile(TestProviderProfile {
            profile: ServiceProviderProfile::Ordinary(profile),
            default_test_model,
        })
    })
    .await
    .unwrap_or_else(|_| Err(provider_worker_error()));
    map_provider_test_result(result)
}

#[tauri::command]
pub async fn test_stepwise_settings(
    settings: BackendSettings,
) -> CommandResult<StepwiseTestPayload> {
    match codex_plus_core::stepwise::test_connection(&settings).await {
        Ok(result) => {
            let error = result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let item_count = result
                .get("items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if error.is_empty() {
                ok(
                    &format!("Stepwise 连接正常，测试返回 {item_count} 条建议。"),
                    StepwiseTestPayload { item_count, error },
                )
            } else {
                failed(
                    &format!("Stepwise 测试失败：{error}"),
                    StepwiseTestPayload { item_count, error },
                )
            }
        }
        Err(error) => failed(
            &format!("Stepwise 测试失败：{error}"),
            StepwiseTestPayload {
                item_count: 0,
                error: error.to_string(),
            },
        ),
    }
}

#[tauri::command]
pub async fn fetch_relay_profile_models(
    profile: RelayProfile,
) -> CommandResult<RelayProfileModelsPayload> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        system_provider_source().fetch_models(FetchProviderModels {
            profile: ServiceProviderProfile::Ordinary(profile),
        })
    })
    .await
    .unwrap_or_else(|_| Err(provider_worker_error()));
    map_provider_models_result(result)
}

#[tauri::command]
pub async fn diagnose_relay_profile(profile: RelayProfile) -> CommandResult<ProviderDoctorPayload> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let source = system_provider_source();
        let default_test_model = source
            .load_workspace()
            .map(|workspace| workspace.document.default_test_model)
            .unwrap_or_default();
        source.diagnose_profile(DiagnoseProviderProfile {
            profile: ServiceProviderProfile::Ordinary(profile),
            default_test_model,
        })
    })
    .await
    .unwrap_or_else(|_| Err(provider_worker_error()));
    map_provider_doctor_result(result)
}

#[tauri::command]
pub fn apply_relay_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 写锁失败：{error}"),
                relay_payload(unavailable_relay_status(&home), None),
            );
        }
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    if !settings.relay_profiles_enabled {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        return failed(
            "供应商配置总开关已关闭，未写入 config.toml / auth.json。",
            relay_payload(status, None),
        );
    }
    let relay = settings.active_relay_profile();
    log_relay_apply_request("manager.apply_relay_injection", &settings, &relay);
    if settings.active_aggregate_relay_profile().is_some() {
        return apply_aggregate_relay_injection_to_home(&home);
    }
    if relay_has_complete_files(&relay) {
        return match codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            &home,
            &relay,
            &relay_combined_common_config(&settings),
            settings.computer_use_guard_enabled,
        ) {
            Ok(result) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_relay_injection.ok",
                    &relay,
                    &status,
                    result.backup_path.as_ref(),
                    None,
                );
                ok(
                    "已按兼容切换规则切换供应商。",
                    relay_payload(status, result.backup_path),
                )
            }
            Err(error) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_relay_injection.failed",
                    &relay,
                    &status,
                    None,
                    Some(error.to_string()),
                );
                failed(
                    &format!("切换完整中转配置失败：{error}"),
                    relay_payload(status, None),
                )
            }
        };
    }

    let auth = codex_plus_core::relay_config::chatgpt_auth_status_from_home(&home);
    if !auth.authenticated {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        log_relay_apply_result(
            "manager.apply_relay_injection.failed",
            &relay,
            &status,
            None,
            Some("未检测到 ChatGPT 登录状态".to_string()),
        );
        return failed(
            "未检测到 ChatGPT 登录状态，已停止写入中转配置。",
            relay_payload(status, None),
        );
    }

    match codex_plus_core::relay_config::apply_relay_config_to_home_with_protocol(
        &home,
        &relay.base_url,
        &relay.api_key,
        relay.protocol,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_relay_injection.ok",
                &relay,
                &status,
                result.backup_path.as_ref(),
                None,
            );
            ok(
                "中转配置已写入，密钥未在界面明文显示。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_relay_injection.failed",
                &relay,
                &status,
                None,
                Some(error.to_string()),
            );
            failed(
                &format!("写入中转配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

fn apply_aggregate_relay_injection_to_home(home: &Path) -> CommandResult<RelayPayload> {
    match codex_plus_core::relay_config::apply_relay_config_to_home_with_protocol(
        home,
        &codex_plus_core::protocol_proxy::local_responses_proxy_base_url(
            codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        ),
        "codex-plus-aggregate",
        codex_plus_core::settings::RelayProtocol::Responses,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(home);
            ok(
                "聚合供应商配置已写入，真实请求会由本地代理按策略轮转。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(home);
            failed(
                &format!("写入聚合供应商配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

#[tauri::command]
pub fn apply_pure_api_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 写锁失败：{error}"),
                relay_payload(unavailable_relay_status(&home), None),
            );
        }
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    if !settings.relay_profiles_enabled {
        let status = codex_plus_core::relay_config::relay_status_from_home(&home);
        return failed(
            "供应商配置总开关已关闭，未写入 config.toml / auth.json。",
            relay_payload(status, None),
        );
    }
    let relay = settings.active_relay_profile();
    log_relay_apply_request("manager.apply_pure_api_injection", &settings, &relay);
    if relay_has_complete_files(&relay) {
        return match codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            &home,
            &relay,
            &relay_combined_common_config(&settings),
            settings.computer_use_guard_enabled,
        ) {
            Ok(result) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_pure_api_injection.ok",
                    &relay,
                    &status,
                    result.backup_path.as_ref(),
                    None,
                );
                if !status.configured {
                    return failed(
                        "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。",
                        relay_payload(status, result.backup_path),
                    );
                }
                ok(
                    "已按兼容切换规则切换供应商。",
                    relay_payload(status, result.backup_path),
                )
            }
            Err(error) => {
                let status = codex_plus_core::relay_config::relay_status_from_home(&home);
                log_relay_apply_result(
                    "manager.apply_pure_api_injection.failed",
                    &relay,
                    &status,
                    None,
                    Some(error.to_string()),
                );
                failed(
                    &format!("切换纯 API 配置失败：{error}"),
                    relay_payload(status, None),
                )
            }
        };
    }

    match codex_plus_core::relay_config::apply_pure_api_config_to_home_with_protocol(
        &home,
        &relay.base_url,
        &relay.api_key,
        relay.protocol,
        codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    ) {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_pure_api_injection.ok",
                &relay,
                &status,
                result.backup_path.as_ref(),
                None,
            );
            if !status.configured {
                return failed(
                    "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。",
                    relay_payload(status, result.backup_path),
                );
            }
            ok(
                "纯 API 模式已写入：config.toml 已写入 custom provider，auth.json 已切换为当前供应商。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_relay_apply_result(
                "manager.apply_pure_api_injection.failed",
                &relay,
                &status,
                None,
                Some(error.to_string()),
            );
            failed(
                &format!("写入纯 API 模式失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

#[tauri::command]
pub fn clear_relay_injection() -> CommandResult<RelayPayload> {
    let home = codex_plus_core::relay_config::default_codex_home_dir();
    let _live_guard = match codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home) {
        Ok(guard) => guard,
        Err(error) => {
            return failed(
                &format!("获取供应商 live 写锁失败：{error}"),
                relay_payload(unavailable_relay_status(&home), None),
            );
        }
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    let relay = settings.active_relay_profile();
    log_manager_event("manager.clear_relay_injection.start", json!({}));
    let auth_contents = (relay.relay_mode == codex_plus_core::settings::RelayMode::Official
        && !relay.official_mix_api_key
        && !relay.auth_contents.trim().is_empty())
    .then_some(relay.auth_contents.as_str());
    match codex_plus_core::relay_config::clear_relay_config_to_home_with_auth(&home, auth_contents)
    {
        Ok(result) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.clear_relay_injection.ok",
                json!({
                    "configured": status.configured,
                    "backupPath": result.backup_path.as_ref()
                }),
            );
            ok(
                "已清除 custom 中转 API 模式，并切换到官方 ChatGPT 登录模式。",
                relay_payload(status, result.backup_path),
            )
        }
        Err(error) => {
            let status = codex_plus_core::relay_config::relay_status_from_home(&home);
            log_manager_event(
                "manager.clear_relay_injection.failed",
                json!({
                    "configured": status.configured,
                    "error": error.to_string()
                }),
            );
            failed(
                &format!("清除中转配置失败：{error}"),
                relay_payload(status, None),
            )
        }
    }
}

fn relay_has_complete_files(relay: &codex_plus_core::settings::RelayProfile) -> bool {
    if relay.relay_mode == codex_plus_core::settings::RelayMode::Official
        && relay.official_mix_api_key
    {
        return !relay.config_contents.trim().is_empty();
    }
    !relay.config_contents.trim().is_empty() && !relay.auth_contents.trim().is_empty()
}

fn log_relay_apply_request(
    event: &str,
    settings: &BackendSettings,
    relay: &codex_plus_core::settings::RelayProfile,
) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        event,
        json!({
            "activeRelayId": settings.active_relay_id,
            "relayId": relay.id,
            "relayName": relay.name,
            "relayMode": relay.relay_mode,
            "protocol": relay.protocol,
            "baseUrl": relay.base_url,
            "hasConfigContents": !relay.config_contents.trim().is_empty(),
            "hasAuthContents": !relay.auth_contents.trim().is_empty(),
            "configContainsProxy": relay.config_contents.contains("127.0.0.1:57321")
        }),
    );
}

fn log_relay_apply_result(
    event: &str,
    relay: &codex_plus_core::settings::RelayProfile,
    status: &codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<&String>,
    error: Option<String>,
) {
    log_manager_event(
        event,
        json!({
            "relayId": relay.id,
            "relayName": relay.name,
            "relayMode": relay.relay_mode,
            "protocol": relay.protocol,
            "configured": status.configured,
            "requiresOpenaiAuth": status.requires_openai_auth,
            "hasBearerToken": status.has_bearer_token,
            "backupPath": backup_path,
            "error": error
        }),
    );
}

fn log_manager_event(event: &str, detail: Value) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(event, detail);
}

fn sanitize_manager_event(event: &str) -> String {
    let suffix = event
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let suffix = suffix.trim_matches(['.', '_', '-']).trim();
    if suffix.is_empty() {
        "manager.ui.event".to_string()
    } else if suffix.starts_with("manager.") {
        suffix.to_string()
    } else {
        format!("manager.ui.{suffix}")
    }
}

fn relay_payload(
    status: codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<String>,
) -> RelayPayload {
    RelayPayload {
        authenticated: status.authenticated,
        auth_source: status.auth_source,
        account_label: status.account_label,
        config_path: status.config_path,
        configured: status.configured,
        requires_openai_auth: status.requires_openai_auth,
        has_bearer_token: status.has_bearer_token,
        backup_path,
    }
}

fn relay_switch_payload(
    settings: BackendSettings,
    status: codex_plus_core::relay_config::RelayStatus,
    backup_path: Option<String>,
) -> RelaySwitchPayload {
    RelaySwitchPayload {
        settings,
        relay: relay_payload(status, backup_path),
        settings_path: codex_plus_core::paths::default_settings_path()
            .to_string_lossy()
            .to_string(),
        user_scripts: user_script_inventory(),
    }
}

fn unavailable_relay_status(home: &Path) -> codex_plus_core::relay_config::RelayStatus {
    codex_plus_core::relay_config::RelayStatus {
        authenticated: false,
        auth_source: "unavailable".to_string(),
        account_label: None,
        config_path: home.join("config.toml").to_string_lossy().to_string(),
        configured: false,
        requires_openai_auth: false,
        has_bearer_token: false,
    }
}

fn with_relay_live_read_lock<T>(
    home: &Path,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let _guard = codex_plus_core::relay_config::acquire_relay_live_read_lock(home)?;
    operation()
}

fn with_relay_live_mutation_lock<T>(
    home: &Path,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let _guard = codex_plus_core::relay_config::acquire_relay_live_mutation_lock(home)?;
    operation()
}

fn relay_switch_mutex() -> &'static Mutex<()> {
    static RELAY_SWITCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    RELAY_SWITCH_LOCK.get_or_init(|| Mutex::new(()))
}

fn empty_context_entries() -> codex_plus_core::relay_config::CodexContextEntries {
    codex_plus_core::relay_config::CodexContextEntries {
        mcp_servers: Vec::new(),
        skills: Vec::new(),
        plugins: Vec::new(),
    }
}

fn relay_files_payload_from_home(home: &std::path::Path) -> anyhow::Result<RelayFilesPayload> {
    let config_path = home.join("config.toml");
    let auth_path = home.join("auth.json");
    Ok(RelayFilesPayload {
        config_path: config_path.to_string_lossy().to_string(),
        auth_path: auth_path.to_string_lossy().to_string(),
        config_contents: read_optional_text_file(&config_path)?,
        auth_contents: read_optional_text_file(&auth_path)?,
    })
}

fn save_relay_file_in_home(
    home: &std::path::Path,
    kind: &str,
    contents: &str,
) -> anyhow::Result<()> {
    let path = match kind {
        "config" => home.join("config.toml"),
        "auth" => home.join("auth.json"),
        other => anyhow::bail!("未知配置文件类型：{other}"),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

fn read_optional_text_file(path: &std::path::Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        codex_plus_core::windows_open_url(url)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("启动系统浏览器失败：{error}"))
    }
}

fn settings_payload(message: &str, failure_context: &str) -> CommandResult<SettingsPayload> {
    match settings_payload_value() {
        Ok(payload) => ok(message, payload),
        Err((error, payload)) => failed(&format!("{failure_context}：{error}"), payload),
    }
}

fn settings_payload_value() -> Result<SettingsPayload, (anyhow::Error, SettingsPayload)> {
    let store = SettingsStore::default();
    let settings_path = codex_plus_core::paths::default_settings_path()
        .to_string_lossy()
        .to_string();
    match store.load() {
        Ok(settings) => Ok(SettingsPayload {
            settings,
            settings_path,
            user_scripts: user_script_inventory(),
        }),
        Err(error) => Err((
            error,
            SettingsPayload {
                settings: BackendSettings::default(),
                settings_path,
                user_scripts: user_script_inventory(),
            },
        )),
    }
}

fn fallback_settings_payload() -> SettingsPayload {
    SettingsPayload {
        settings: SettingsStore::default().load().unwrap_or_default(),
        settings_path: codex_plus_core::paths::default_settings_path()
            .to_string_lossy()
            .to_string(),
        user_scripts: user_script_inventory(),
    }
}

fn user_script_inventory() -> Value {
    default_user_script_manager()
        .inventory()
        .unwrap_or_else(|error| {
            json!({
                "enabled": true,
                "scripts": [],
                "error": error.to_string()
            })
        })
}

fn failed_script_market_payload(message: &str) -> ScriptMarketPayload {
    ScriptMarketPayload {
        market: json!({
            "status": "failed",
            "message": message,
            "indexUrl": script_market::DEFAULT_MARKET_INDEX_URL,
            "updatedAt": "",
            "scripts": []
        }),
        user_scripts: user_script_inventory(),
    }
}

fn script_market_payload_from_manifest(
    manifest: &ScriptMarketManifest,
    status: &str,
    message: &str,
) -> ScriptMarketPayload {
    let user_scripts = user_script_inventory();
    let installed = installed_market_versions(&user_scripts);
    let scripts = manifest
        .scripts
        .iter()
        .map(|script| market_script_payload(script, &installed))
        .collect::<Vec<_>>();
    ScriptMarketPayload {
        market: json!({
            "status": status,
            "message": message,
            "indexUrl": script_market::DEFAULT_MARKET_INDEX_URL,
            "updatedAt": manifest.updated_at.clone().unwrap_or_default(),
            "scripts": scripts
        }),
        user_scripts,
    }
}

fn installed_market_versions(user_scripts: &Value) -> BTreeMap<String, String> {
    user_scripts
        .get("scripts")
        .and_then(Value::as_array)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|script| {
                    let id = script.get("market_id").and_then(Value::as_str)?;
                    if id.is_empty() {
                        return None;
                    }
                    let version = script
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Some((id.to_string(), version))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn market_script_payload(script: &MarketScript, installed: &BTreeMap<String, String>) -> Value {
    let installed_version = installed.get(&script.id).cloned().unwrap_or_default();
    let is_installed = !installed_version.is_empty();
    json!({
        "id": script.id,
        "name": script.name,
        "description": script.description,
        "version": script.version,
        "author": script.author,
        "tags": script.tags,
        "homepage": script.homepage,
        "script_url": script.script_url,
        "sha256": script.sha256,
        "installed": is_installed,
        "installedVersion": installed_version,
        "updateAvailable": is_installed && installed.get(&script.id).map(|version| version != &script.version).unwrap_or(false)
    })
}

fn default_user_script_manager() -> UserScriptManager {
    let config_dir = user_scripts_config_dir();
    UserScriptManager::new(
        builtin_user_scripts_dir(),
        config_dir.join("user_scripts"),
        config_dir.join("user_scripts.json"),
    )
}

fn user_scripts_config_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            return PathBuf::from(roaming).join("Codex++");
        }
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("Codex++")
}

fn builtin_user_scripts_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.join("user_scripts"))
        .unwrap_or_else(|| PathBuf::from("user_scripts"))
}

fn diagnostics_report() -> String {
    let overview = match load_overview_payload() {
        Ok(payload) => ok("概览已加载。", payload),
        Err(_) => failed("概览后台任务失败。", overview_failure_payload()),
    };
    let settings = SettingsStore::default().load().unwrap_or_default();
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    serde_json::to_string_pretty(&json!({
        "generatedAtMs": generated_at_ms,
        "version": codex_plus_core::version::VERSION,
        "overview": overview.payload,
        "settings": settings,
        "logs": {
            "diagnosticLogPath": codex_plus_core::paths::default_diagnostic_log_path(),
            "latestStatusPath": codex_plus_core::paths::default_latest_status_path()
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH
        }
    }))
    .unwrap_or_else(|error| format!("诊断报告序列化失败：{error}"))
}

fn load_overview_payload() -> Result<OverviewPayload, codex_plus_manager_service::OverviewError> {
    SystemOverviewSource::default()
        .load_overview()
        .map(overview_payload_from_snapshot)
}

fn overview_payload_from_snapshot(snapshot: OverviewSnapshot) -> OverviewPayload {
    let codex_status = match snapshot.codex_app.presence {
        ResourcePresence::Found => "found",
        ResourcePresence::Missing => "missing",
    };
    let update_status = match snapshot.update_status {
        UpdateCheckState::NotChecked => "not_checked",
    };

    OverviewPayload {
        codex_app: PathState {
            status: codex_status.to_owned(),
            path: snapshot
                .codex_app
                .path
                .map(|path| path.to_string_lossy().to_string()),
        },
        codex_version: snapshot.codex_version,
        silent_shortcut: PathState {
            status: if snapshot.silent_shortcut.installed {
                "installed".to_owned()
            } else {
                "missing".to_owned()
            },
            path: snapshot
                .silent_shortcut
                .path
                .map(|path| path.to_string_lossy().to_string()),
        },
        management_shortcut: PathState {
            status: if snapshot.management_shortcut.installed {
                "installed".to_owned()
            } else {
                "missing".to_owned()
            },
            path: snapshot
                .management_shortcut
                .path
                .map(|path| path.to_string_lossy().to_string()),
        },
        latest_launch: snapshot.latest_launch,
        current_version: snapshot.current_version,
        update_status: update_status.to_owned(),
        settings_path: snapshot.settings_path.to_string_lossy().to_string(),
        logs_path: snapshot.logs_path.to_string_lossy().to_string(),
    }
}

fn overview_failure_payload() -> OverviewPayload {
    OverviewPayload {
        codex_app: PathState {
            status: "missing".to_owned(),
            path: None,
        },
        codex_version: None,
        silent_shortcut: PathState {
            status: "missing".to_owned(),
            path: None,
        },
        management_shortcut: PathState {
            status: "missing".to_owned(),
            path: None,
        },
        latest_launch: None,
        current_version: codex_plus_core::version::VERSION.to_owned(),
        update_status: "not_checked".to_owned(),
        settings_path: codex_plus_core::paths::default_settings_path()
            .to_string_lossy()
            .to_string(),
        logs_path: codex_plus_core::paths::default_diagnostic_log_path()
            .to_string_lossy()
            .to_string(),
    }
}

fn install_background_failure(action: &str, error: impl std::fmt::Display) -> InstallActionResult {
    let state = install::inspect_entrypoints();
    InstallActionResult {
        status: "failed".to_string(),
        message: format!("{action}后台任务失败：{error}"),
        silent_shortcut: state.silent_shortcut,
        management_shortcut: state.management_shortcut,
    }
}

fn watcher_payload() -> WatcherPayload {
    let flag = codex_plus_core::watcher::default_watcher_disabled_flag();
    WatcherPayload {
        enabled: !flag.exists(),
        disabled_flag: flag.to_string_lossy().to_string(),
    }
}

fn read_tail(path: &Path, max_lines: usize) -> std::io::Result<String> {
    let contents = fs::read_to_string(path)?;
    let mut lines = contents.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    Ok(lines.join("\n"))
}

fn system_provider_source() -> &'static ProviderService<SystemProviderEnvironment> {
    static SOURCE: OnceLock<ProviderService<SystemProviderEnvironment>> = OnceLock::new();
    SOURCE.get_or_init(|| ProviderService::new(system_provider_environment().clone()))
}

fn system_provider_import_source() -> &'static ProviderImportService<SystemProviderEnvironment> {
    static SOURCE: OnceLock<ProviderImportService<SystemProviderEnvironment>> = OnceLock::new();
    SOURCE.get_or_init(|| ProviderImportService::new(system_provider_environment().clone()))
}

fn system_relay_environment_source() -> &'static RelayEnvironmentService<SystemProviderEnvironment>
{
    static SOURCE: OnceLock<RelayEnvironmentService<SystemProviderEnvironment>> = OnceLock::new();
    SOURCE.get_or_init(|| RelayEnvironmentService::new(system_provider_environment().clone()))
}

fn system_context_tools_source() -> &'static ContextToolsService<SystemProviderEnvironment> {
    static SOURCE: OnceLock<ContextToolsService<SystemProviderEnvironment>> = OnceLock::new();
    SOURCE.get_or_init(|| ContextToolsService::new(system_provider_environment().clone()))
}

fn system_plugin_marketplace_source() -> &'static PluginMarketplaceService<SystemProviderEnvironment>
{
    static SOURCE: OnceLock<PluginMarketplaceService<SystemProviderEnvironment>> = OnceLock::new();
    SOURCE.get_or_init(|| PluginMarketplaceService::new(system_provider_environment().clone()))
}

fn system_provider_environment() -> &'static SystemProviderEnvironment {
    static ENVIRONMENT: OnceLock<SystemProviderEnvironment> = OnceLock::new();
    ENVIRONMENT.get_or_init(SystemProviderEnvironment::default)
}

fn provider_worker_error() -> ProviderNetworkError {
    ProviderNetworkError::for_failure(ProviderNetworkFailureKind::Network, None, None)
}

fn map_provider_test_result(
    result: Result<ProviderTestResult, ProviderNetworkError>,
) -> CommandResult<RelayProfileTestPayload> {
    match result {
        Ok(result) => {
            let (status, message, preview) = match result.outcome {
                ProviderTestOutcome::Success => (
                    "ok",
                    "供应商连接测试通过。".to_string(),
                    "request_succeeded".to_string(),
                ),
                ProviderTestOutcome::OfficialNoApiRequired => (
                    "ok",
                    "官方登录供应商无需 API 连接测试。".to_string(),
                    "official_no_api_required".to_string(),
                ),
                ProviderTestOutcome::Failure(kind) => (
                    "failed",
                    format!("供应商连接测试失败：{}。", network_failure_code(kind)),
                    network_failure_code(kind).to_string(),
                ),
            };
            CommandResult {
                status: status.to_string(),
                message,
                payload: RelayProfileTestPayload {
                    http_status: result.http_status.unwrap_or_default(),
                    endpoint: result
                        .endpoint
                        .map(|endpoint| endpoint.as_str().to_string())
                        .unwrap_or_default(),
                    response_preview: preview,
                },
            }
        }
        Err(error) => failed(
            &format!(
                "供应商连接测试失败：{}。",
                network_failure_code(error.kind())
            ),
            RelayProfileTestPayload {
                http_status: error.http_status().unwrap_or_default(),
                endpoint: error
                    .endpoint()
                    .map(|endpoint| endpoint.as_str().to_string())
                    .unwrap_or_default(),
                response_preview: network_failure_code(error.kind()).to_string(),
            },
        ),
    }
}

fn map_provider_models_result(
    result: Result<ProviderModelsResult, ProviderNetworkError>,
) -> CommandResult<RelayProfileModelsPayload> {
    match result {
        Ok(result) => ok(
            &format!("已获取 {} 个模型。", result.models.len()),
            RelayProfileModelsPayload {
                models: result.models,
                endpoint: result.endpoint.as_str().to_string(),
            },
        ),
        Err(error) => failed(
            &format!("获取模型失败：{}。", network_failure_code(error.kind())),
            RelayProfileModelsPayload {
                models: Vec::new(),
                endpoint: error
                    .endpoint()
                    .map(|endpoint| endpoint.as_str().to_string())
                    .unwrap_or_default(),
            },
        ),
    }
}

fn map_provider_doctor_result(
    result: Result<ProviderDoctorReport, ProviderNetworkError>,
) -> CommandResult<ProviderDoctorPayload> {
    let Ok(report) = result else {
        let error = result.unwrap_err();
        return failed(
            &format!("Provider Doctor：{}。", network_failure_code(error.kind())),
            ProviderDoctorPayload {
                profile_name: String::new(),
                model: String::new(),
                summary: "诊断未完成。".to_string(),
                recommendation: "检查网络后重试。".to_string(),
                checks: Vec::new(),
            },
        );
    };
    let summary = doctor_summary(report.outcome).to_string();
    let recommendation = doctor_recommendation(report.recommendation).to_string();
    let status = if matches!(
        report.outcome,
        DoctorOutcome::Failed | DoctorOutcome::AggregateUnsupported
    ) {
        "failed"
    } else {
        "ok"
    };
    CommandResult {
        status: status.to_string(),
        message: format!("Provider Doctor：{summary}"),
        payload: ProviderDoctorPayload {
            profile_name: report.profile_name,
            model: report.model,
            summary,
            recommendation,
            checks: report
                .checks
                .iter()
                .map(map_provider_doctor_check)
                .collect(),
        },
    }
}

fn map_provider_doctor_check(check: &ServiceProviderDoctorCheck) -> ProviderDoctorCheck {
    ProviderDoctorCheck {
        id: match check.id {
            ProviderDoctorCheckId::Config => "config",
            ProviderDoctorCheckId::Models => "models",
            ProviderDoctorCheckId::Request => "request",
        }
        .to_string(),
        title: match check.id {
            ProviderDoctorCheckId::Config => "配置完整性",
            ProviderDoctorCheckId::Models => "模型列表",
            ProviderDoctorCheckId::Request => "真实请求",
        }
        .to_string(),
        status: match check.status {
            DoctorCheckStatus::Passed => "ok",
            DoctorCheckStatus::Warning => "warning",
            DoctorCheckStatus::Failed => "failed",
            DoctorCheckStatus::Skipped => "skipped",
        }
        .to_string(),
        detail: doctor_check_detail(check),
    }
}

fn doctor_check_detail(check: &ServiceProviderDoctorCheck) -> String {
    let endpoint = check
        .endpoint
        .as_ref()
        .map(|endpoint| endpoint.as_str())
        .unwrap_or("安全端点不可用");
    match check.detail {
        DoctorDetailKind::ConfigReady => format!("配置可用：{endpoint}。"),
        DoctorDetailKind::MissingConfiguration => "Base URL、API Key 或测试模型缺失。".to_string(),
        DoctorDetailKind::InvalidEndpoint => "Base URL 不是有效的 HTTP(S) 地址。".to_string(),
        DoctorDetailKind::OfficialNoApiRequired => {
            "官方登录供应商不需要 Base URL 或 API Key。".to_string()
        }
        DoctorDetailKind::AggregateUnsupported => "请分别诊断聚合配置中的普通成员。".to_string(),
        DoctorDetailKind::ModelsAvailable => format!(
            "模型列表返回 {} 个模型。",
            check.model_count.unwrap_or_default()
        ),
        DoctorDetailKind::ModelsUnavailable => format!(
            "模型列表不可用：{}。",
            check
                .failure
                .map(network_failure_code)
                .unwrap_or("invalid_response")
        ),
        DoctorDetailKind::TestModelMissing => format!(
            "模型列表包含 {} 个模型，但未找到测试模型。",
            check.model_count.unwrap_or_default()
        ),
        DoctorDetailKind::RequestSucceeded => format!(
            "真实请求成功，HTTP {}。",
            check.http_status.unwrap_or_default()
        ),
        DoctorDetailKind::RequestFailed => format!(
            "真实请求失败：{}。",
            check
                .failure
                .map(network_failure_code)
                .unwrap_or("upstream_failure")
        ),
    }
}

fn doctor_summary(outcome: DoctorOutcome) -> &'static str {
    match outcome {
        DoctorOutcome::Passed => "供应商基础诊断通过。",
        DoctorOutcome::Warning => "测试模型不在模型列表中。",
        DoctorOutcome::Failed => "发现诊断失败项。",
        DoctorOutcome::OfficialNoApiRequired => "官方登录供应商无需 API 诊断。",
        DoctorOutcome::AggregateUnsupported => "聚合供应商需要逐个诊断成员。",
    }
}

fn doctor_recommendation(recommendation: DoctorRecommendation) -> &'static str {
    match recommendation {
        DoctorRecommendation::Ready => "供应商配置可用。",
        DoctorRecommendation::CompleteConfiguration => "补齐 Base URL、API Key 和测试模型。",
        DoctorRecommendation::CheckModelsEndpoint => "检查 Base URL 与 /models 支持情况。",
        DoctorRecommendation::UseDiscoveredModel => "改用上游模型列表中返回的模型名。",
        DoctorRecommendation::CheckCredentialsOrProtocol => "检查 Key 权限、模型名与上游协议。",
        DoctorRecommendation::UseOfficialLogin => "继续使用 Codex 官方登录模式。",
        DoctorRecommendation::TestAggregateMembers => "分别测试聚合配置中的普通成员。",
    }
}

fn network_failure_code(kind: ProviderNetworkFailureKind) -> &'static str {
    match kind {
        ProviderNetworkFailureKind::MissingConfiguration => "missing_configuration",
        ProviderNetworkFailureKind::InvalidEndpoint => "invalid_endpoint",
        ProviderNetworkFailureKind::Unauthorized => "unauthorized",
        ProviderNetworkFailureKind::NotFound => "not_found",
        ProviderNetworkFailureKind::RateLimited => "rate_limited",
        ProviderNetworkFailureKind::UpstreamFailure => "upstream_failure",
        ProviderNetworkFailureKind::Timeout => "timeout",
        ProviderNetworkFailureKind::Network => "network",
        ProviderNetworkFailureKind::InvalidResponse => "invalid_response",
        ProviderNetworkFailureKind::AggregateUnsupported => "aggregate_unsupported",
    }
}

fn ok<T: Serialize>(message: &str, payload: T) -> CommandResult<T> {
    CommandResult {
        status: "ok".to_string(),
        message: message.to_string(),
        payload,
    }
}

fn failed<T: Serialize>(message: &str, payload: T) -> CommandResult<T> {
    CommandResult {
        status: "failed".to_string(),
        message: message.to_string(),
        payload,
    }
}

fn default_debug_port() -> u16 {
    9229
}

fn default_helper_port() -> u16 {
    57321
}

fn default_log_lines() -> usize {
    200
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_and_provider_sync_adapters_preserve_compatibility_fields() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        let sqlite_dir = home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        create_minimal_thread_db(
            &sqlite_dir.join("state_5.sqlite"),
            "readable",
            "Readable",
            1,
        );
        let unsupported = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        unsupported
            .execute("CREATE TABLE unsupported (id TEXT PRIMARY KEY)", [])
            .unwrap();
        drop(unsupported);
        let service = codex_plus_manager_service::SessionService::new(
            SystemProviderEnvironment::for_paths(temp.path().join("settings.json"), home),
        );

        let listed = list_local_sessions_with_service(&service);
        let list_json = serde_json::to_value(&listed).unwrap();
        assert_eq!(listed.status, "failed");
        assert_eq!(listed.payload.sessions.len(), 1);
        assert!(list_json.get("dbPath").is_some());
        assert!(list_json.get("dbPaths").is_some());
        assert!(list_json.get("sessions").is_some());

        let delete_json = serde_json::to_value(DeleteResult {
            status: codex_plus_core::models::DeleteStatus::LocalDeleted,
            session_id: "readable".to_owned(),
            message: "deleted".to_owned(),
            undo_token: Some("opaque".to_owned()),
            backup_path: Some("backup.json".to_owned()),
        })
        .unwrap();
        assert!(delete_json.get("undo_token").is_some());
        assert!(delete_json.get("backup_path").is_some());

        let sync_json = provider_sync_result_payload(&codex_plus_data::ProviderSyncResult {
            status: codex_plus_data::ProviderSyncStatus::Synced,
            message: "synced".to_owned(),
            target_provider: "openai".to_owned(),
            backup_dir: None,
            changed_session_files: 1,
            skipped_locked_rollout_files: Vec::new(),
            sqlite_rows_updated: 2,
            sqlite_provider_rows_updated: 1,
            sqlite_user_event_rows_updated: 1,
            sqlite_cwd_rows_updated: 0,
            updated_workspace_roots: 0,
            encrypted_content_warning: None,
        });
        assert!(sync_json.get("changedSessionFiles").is_some());
        assert!(sync_json.get("skippedLockedRolloutFiles").is_some());
        assert!(sync_json.get("sqliteRowsUpdated").is_some());
        assert!(sync_json.get("backupDir").is_some());
    }

    #[test]
    fn plugin_marketplace_adapters_preserve_all_four_json_contracts() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        write_test_local_marketplace(&home);
        let service = PluginMarketplaceService::new(SystemProviderEnvironment::for_paths(
            temp.path().join("settings.json"),
            home.clone(),
        ));

        let local_status = plugin_marketplace_status_with_service(&service, &home);
        let local_status_json = serde_json::to_value(&local_status).unwrap();
        assert_eq!(local_status.status, "ok");
        assert!(!local_status.message.is_empty());
        assert_eq!(
            json_object_keys(&local_status_json),
            vec![
                "codexHome",
                "configRegistered",
                "marketplaceRoot",
                "message",
                "needsRepair",
                "status",
            ]
        );
        assert_eq!(
            local_status_json["marketplaceRoot"],
            home.join(".tmp").join("plugins").to_string_lossy().as_ref()
        );
        assert_eq!(local_status_json["configRegistered"], false);
        assert_eq!(local_status_json["needsRepair"], true);

        let local_repair = repair_plugin_marketplace_with_service(&service, &home);
        let local_repair_json = serde_json::to_value(&local_repair).unwrap();
        assert_eq!(local_repair.status, "ok");
        assert_eq!(
            json_object_keys(&local_repair_json),
            vec![
                "codexHome",
                "configured",
                "initialized",
                "marketplaceRoot",
                "message",
                "needsRepair",
                "status",
            ]
        );
        assert_eq!(local_repair_json["initialized"], false);
        assert_eq!(local_repair_json["configured"], true);
        assert_eq!(local_repair_json["needsRepair"], false);

        let remote_status = remote_plugin_marketplace_status_with_service(&service, &home);
        let remote_status_json = serde_json::to_value(&remote_status).unwrap();
        assert_eq!(remote_status.status, "ok");
        assert_eq!(
            json_object_keys(&remote_status_json),
            vec![
                "codexHome",
                "configRegistered",
                "marketplaceRoot",
                "message",
                "needsRepair",
                "pluginCount",
                "skillCount",
                "status",
            ]
        );
        assert!(remote_status_json["marketplaceRoot"].is_null());
        assert_eq!(remote_status_json["pluginCount"], 0);
        assert_eq!(remote_status_json["skillCount"], 0);

        let remote_repair = repair_remote_plugin_marketplace_with_service(&service, &home);
        let remote_repair_json = serde_json::to_value(&remote_repair).unwrap();
        assert_eq!(remote_repair.status, "ok");
        assert_eq!(
            json_object_keys(&remote_repair_json),
            vec![
                "codexHome",
                "configRegistered",
                "marketplaceRoot",
                "message",
                "needsRepair",
                "pluginCount",
                "skillCount",
                "status",
            ]
        );
        assert!(!remote_repair_json["marketplaceRoot"].is_null());
        assert_eq!(remote_repair_json["configRegistered"], true);
        assert_eq!(remote_repair_json["needsRepair"], false);
        assert!(remote_repair_json["pluginCount"].as_u64().unwrap() > 0);
        assert!(remote_repair_json["skillCount"].as_u64().unwrap() > 0);
    }

    #[test]
    fn plugin_marketplace_repair_adapter_preserves_failure_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        write_test_local_marketplace(&home);
        std::fs::write(home.join("config.toml"), "[invalid\n").unwrap();
        let service = PluginMarketplaceService::new(SystemProviderEnvironment::for_paths(
            temp.path().join("settings.json"),
            home.clone(),
        ));

        let result = repair_plugin_marketplace_with_service(&service, &home);

        assert_eq!(result.status, "failed");
        assert!(result.message.contains("config.toml"));
        assert_eq!(result.payload.codex_home, home.to_string_lossy());
        assert!(result.payload.marketplace_root.is_some());
        assert!(!result.payload.initialized);
        assert!(!result.payload.configured);
        assert!(result.payload.needs_repair);
    }

    #[test]
    fn backend_version_returns_structured_payload() {
        let result = backend_version();

        assert_eq!(result.status, "ok");
        assert!(!result.payload.version.is_empty());
    }

    fn json_object_keys(value: &Value) -> Vec<&str> {
        let mut keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }

    fn write_test_local_marketplace(home: &Path) {
        let root = home.join(".tmp/plugins");
        std::fs::create_dir_all(root.join(".agents/plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins/gmail")).unwrap();
        std::fs::write(
            root.join(".agents/plugins/marketplace.json"),
            r#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
        )
        .unwrap();
    }

    #[test]
    fn provider_import_and_environment_payloads_preserve_legacy_json_fields() {
        let ccs = ok(
            "已读取 cc-switch Codex 供应商配置：0 个。",
            CcsProvidersPayload {
                db_path: "fixture/cc-switch.db".to_owned(),
                providers: Vec::new(),
            },
        );
        let ccs_json = serde_json::to_value(ccs).unwrap();
        assert_eq!(ccs_json["status"], "ok");
        assert_eq!(
            ccs_json["message"],
            "已读取 cc-switch Codex 供应商配置：0 个。"
        );
        assert_eq!(ccs_json["dbPath"], "fixture/cc-switch.db");
        assert!(ccs_json["providers"].as_array().unwrap().is_empty());
        assert!(ccs_json.get("db_path").is_none());

        let pending = ok(
            "待确认供应商导入已读取。",
            PendingProviderImportPayload { pending: None },
        );
        let pending_json = serde_json::to_value(pending).unwrap();
        assert!(pending_json["pending"].is_null());

        let removal = ok(
            "环境变量已按确认项删除；重新启动 Codex 后生效。",
            RemoveEnvConflictsPayload {
                removed: Vec::new(),
                backup_path: None,
                remaining: Vec::new(),
            },
        );
        let removal_json = serde_json::to_value(removal).unwrap();
        assert!(removal_json["removed"].as_array().unwrap().is_empty());
        assert!(removal_json["backupPath"].is_null());
        assert!(removal_json["remaining"].as_array().unwrap().is_empty());
        assert!(removal_json.get("failures").is_none());
    }

    #[test]
    fn startup_options_returns_structured_payload() {
        let result = startup_options();

        assert_eq!(result.status, "ok");
    }

    #[test]
    fn startup_options_honors_show_update_environment() {
        unsafe {
            std::env::set_var("CODEX_PLUS_SHOW_UPDATE", "1");
        }

        let result = startup_options();

        unsafe {
            std::env::remove_var("CODEX_PLUS_SHOW_UPDATE");
        }

        assert_eq!(result.status, "ok");
        assert!(result.payload.show_update);
    }

    #[test]
    fn startup_options_honors_show_update_argument() {
        assert!(should_show_update(
            ["codex-plus-plus-manager.exe", "--show-update"],
            None
        ));
    }

    #[test]
    fn overview_contains_expected_operational_fields() {
        let result = tauri::async_runtime::block_on(load_overview());

        assert_eq!(result.status, "ok");
        assert!(!result.payload.current_version.is_empty());
        assert!(
            result.payload.codex_version.is_none()
                || result
                    .payload
                    .codex_version
                    .as_deref()
                    .is_some_and(|version| !version.is_empty())
        );
        assert!(matches!(
            result.payload.codex_app.status.as_str(),
            "found" | "missing"
        ));
        assert!(matches!(
            result.payload.silent_shortcut.status.as_str(),
            "installed" | "missing"
        ));
    }

    #[test]
    fn overview_snapshot_preserves_tauri_json_contract() {
        use codex_plus_manager_service::{
            LocatedResource, OverviewSnapshot, ResourcePresence, ShortcutSnapshot, UpdateCheckState,
        };

        let payload = overview_payload_from_snapshot(OverviewSnapshot {
            codex_app: LocatedResource {
                presence: ResourcePresence::Found,
                path: Some(PathBuf::from("C:/Codex")),
            },
            codex_version: Some("0.16.0".to_owned()),
            silent_shortcut: ShortcutSnapshot {
                installed: true,
                path: Some(PathBuf::from("C:/Desktop/Codex++.lnk")),
            },
            management_shortcut: ShortcutSnapshot {
                installed: false,
                path: Some(PathBuf::from("C:/Desktop/Manager.lnk")),
            },
            latest_launch: None,
            current_version: "1.2.36".to_owned(),
            update_status: UpdateCheckState::NotChecked,
            settings_path: PathBuf::from("C:/state/settings.json"),
            logs_path: PathBuf::from("C:/state/diagnostic.log"),
        });
        let result = ok("概览已加载。", payload);

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({
                "status": "ok",
                "message": "概览已加载。",
                "codex_app": { "status": "found", "path": "C:/Codex" },
                "codex_version": "0.16.0",
                "silent_shortcut": {
                    "status": "installed",
                    "path": "C:/Desktop/Codex++.lnk"
                },
                "management_shortcut": {
                    "status": "missing",
                    "path": "C:/Desktop/Manager.lnk"
                },
                "latest_launch": null,
                "current_version": "1.2.36",
                "update_status": "not_checked",
                "settings_path": "C:/state/settings.json",
                "logs_path": "C:/state/diagnostic.log"
            })
        );
    }

    #[test]
    fn overview_snapshot_maps_missing_codex_app_to_null_path() {
        use codex_plus_manager_service::{
            LocatedResource, OverviewSnapshot, ResourcePresence, ShortcutSnapshot, UpdateCheckState,
        };

        let payload = overview_payload_from_snapshot(OverviewSnapshot {
            codex_app: LocatedResource {
                presence: ResourcePresence::Missing,
                path: None,
            },
            codex_version: None,
            silent_shortcut: ShortcutSnapshot {
                installed: false,
                path: None,
            },
            management_shortcut: ShortcutSnapshot {
                installed: false,
                path: None,
            },
            latest_launch: None,
            current_version: "1.2.36".to_owned(),
            update_status: UpdateCheckState::NotChecked,
            settings_path: PathBuf::from("settings.json"),
            logs_path: PathBuf::from("diagnostic.log"),
        });
        let value = serde_json::to_value(payload).unwrap();

        assert_eq!(value["codex_app"]["status"], "missing");
        assert_eq!(value["codex_app"]["path"], serde_json::Value::Null);
        assert_eq!(value["codex_version"], serde_json::Value::Null);
    }

    #[test]
    fn update_install_requires_release_payload() {
        let result = tauri::async_runtime::block_on(perform_update(None));

        assert_eq!(result.status, "failed");
        assert!(result.message.contains("请先检查更新"));
    }

    #[test]
    fn watcher_state_returns_disabled_flag_path() {
        let result = load_watcher_state();

        assert_eq!(result.status, "ok");
        assert!(result.payload.disabled_flag.contains("watcher.disabled"));
    }

    #[test]
    fn missing_logs_return_failed_status() {
        let result = read_latest_logs(LogRequest { lines: 25 });

        if result.payload.text.is_empty() {
            assert_eq!(result.status, "failed");
        }
    }

    #[test]
    fn relay_payload_does_not_expose_token_text() {
        let payload = relay_payload(
            codex_plus_core::relay_config::RelayStatus {
                authenticated: true,
                auth_source: "registry.json".to_string(),
                account_label: Some("user@example.test".to_string()),
                config_path: "config.toml".to_string(),
                configured: true,
                requires_openai_auth: true,
                has_bearer_token: true,
            },
            None,
        );
        let text = serde_json::to_string(&payload).unwrap();

        assert!(!text.contains("sk-"));
        assert!(text.contains("hasBearerToken"));
    }

    #[test]
    fn tauri_live_read_waits_for_native_mutation_lock() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join("config.toml"), "model = \"before\"\n").unwrap();
        let mutation_lock =
            codex_plus_core::relay_config::acquire_relay_live_mutation_lock(&home).unwrap();
        let worker_home = home.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            result_tx
                .send(with_relay_live_read_lock(&worker_home, || {
                    std::fs::read_to_string(worker_home.join("config.toml")).map_err(Into::into)
                }))
                .unwrap();
        });
        started_rx.recv().unwrap();

        assert!(
            result_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(mutation_lock);
        assert_eq!(
            result_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            "model = \"before\"\n"
        );
        worker.join().unwrap();
    }

    #[test]
    fn tauri_live_mutation_waits_for_native_read_lock() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("codex");
        std::fs::create_dir(&home).unwrap();
        let read_lock = codex_plus_core::relay_config::acquire_relay_live_read_lock(&home).unwrap();
        let worker_home = home.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            result_tx
                .send(with_relay_live_mutation_lock(&worker_home, || {
                    std::fs::write(worker_home.join("auth.json"), "{}\n").map_err(Into::into)
                }))
                .unwrap();
        });
        started_rx.recv().unwrap();

        assert!(
            result_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        assert!(!home.join("auth.json").exists());
        drop(read_lock);
        result_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("auth.json")).unwrap(),
            "{}\n"
        );
        worker.join().unwrap();
    }

    #[test]
    fn provider_connectivity_mapper_keeps_payload_shape_and_redacts_content() {
        use codex_plus_manager_service::{
            ProviderNetworkFailureKind, ProviderTestOutcome, ProviderTestResult, SafeEndpoint,
        };

        let success = map_provider_test_result(Ok(ProviderTestResult {
            http_status: Some(200),
            endpoint: SafeEndpoint::parse("https://user:secret@example.test/v1?token=secret"),
            outcome: ProviderTestOutcome::Success,
        }));
        assert_eq!(
            serde_json::to_value(success).unwrap(),
            json!({
                "status": "ok",
                "message": "供应商连接测试通过。",
                "httpStatus": 200,
                "endpoint": "https://example.test/v1",
                "responsePreview": "request_succeeded"
            })
        );

        let failure = map_provider_test_result(Ok(ProviderTestResult {
            http_status: Some(401),
            endpoint: SafeEndpoint::parse("https://example.test/v1"),
            outcome: ProviderTestOutcome::Failure(ProviderNetworkFailureKind::Unauthorized),
        }));
        let serialized = serde_json::to_string(&failure).unwrap();
        assert_eq!(failure.status, "failed");
        assert_eq!(failure.payload.response_preview, "unauthorized");
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn provider_models_mapper_keeps_success_and_empty_failure_fallbacks() {
        use codex_plus_manager_service::{
            ProviderModelsResult, ProviderNetworkError, ProviderNetworkFailureKind, SafeEndpoint,
        };

        let success = map_provider_models_result(Ok(ProviderModelsResult {
            models: vec!["model-a".to_string(), "model-b".to_string()],
            endpoint: SafeEndpoint::parse("https://example.test/v1/models").unwrap(),
        }));
        assert_eq!(
            serde_json::to_value(success).unwrap(),
            json!({
                "status": "ok",
                "message": "已获取 2 个模型。",
                "models": ["model-a", "model-b"],
                "endpoint": "https://example.test/v1/models"
            })
        );

        let failure = map_provider_models_result(Err(ProviderNetworkError::for_failure(
            ProviderNetworkFailureKind::Timeout,
            None,
            None,
        )));
        assert_eq!(failure.status, "failed");
        assert!(failure.payload.models.is_empty());
        assert!(failure.payload.endpoint.is_empty());
    }

    #[test]
    fn provider_doctor_mapper_serializes_typed_checks_to_legacy_fields() {
        use codex_plus_manager_service::{
            DoctorCheckStatus, DoctorDetailKind, DoctorOutcome, DoctorRecommendation,
            ProviderDoctorCheck as ServiceDoctorCheck, ProviderDoctorCheckId, ProviderDoctorReport,
        };

        let result = map_provider_doctor_result(Ok(ProviderDoctorReport {
            profile_name: "Relay A".to_string(),
            model: "model-a".to_string(),
            outcome: DoctorOutcome::Warning,
            recommendation: DoctorRecommendation::UseDiscoveredModel,
            checks: vec![ServiceDoctorCheck {
                id: ProviderDoctorCheckId::Models,
                status: DoctorCheckStatus::Warning,
                detail: DoctorDetailKind::TestModelMissing,
                failure: None,
                http_status: None,
                endpoint: None,
                model_count: Some(2),
            }],
        }));

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "status": "ok",
                "message": "Provider Doctor：测试模型不在模型列表中。",
                "profileName": "Relay A",
                "model": "model-a",
                "summary": "测试模型不在模型列表中。",
                "recommendation": "改用上游模型列表中返回的模型名。",
                "checks": [{
                    "id": "models",
                    "title": "模型列表",
                    "status": "warning",
                    "detail": "模型列表包含 2 个模型，但未找到测试模型。"
                }]
            })
        );
    }

    #[test]
    fn aggregate_relay_injection_writes_local_proxy_without_chatgpt_auth() {
        let temp = tempfile::tempdir().unwrap();

        let result = apply_aggregate_relay_injection_to_home(temp.path());
        let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

        assert_eq!(result.status, "ok");
        assert!(result.payload.configured);
        assert!(!result.payload.authenticated);
        assert!(config.contains(r#"base_url = "http://127.0.0.1:57321/v1""#));
        assert!(config.contains(r#"experimental_bearer_token = "codex-plus-aggregate""#));
    }

    #[test]
    fn relay_files_payload_reads_config_and_auth_contents() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.toml"),
            "model_provider = \"custom\"\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("auth.json"),
            "{\"OPENAI_API_KEY\":\"sk-test\"}\n",
        )
        .unwrap();

        let payload = relay_files_payload_from_home(temp.path()).unwrap();

        assert!(payload.config_path.ends_with("config.toml"));
        assert!(payload.auth_path.ends_with("auth.json"));
        assert_eq!(payload.config_contents, "model_provider = \"custom\"\n");
        assert_eq!(payload.auth_contents, "{\"OPENAI_API_KEY\":\"sk-test\"}\n");
    }

    #[test]
    fn env_conflict_commands_ignore_codex_home_and_remove_openai_vars() {
        let test_openai_name = "OPENAI_CODEX_PLUS_ENV_CONFLICT_TEST";
        let previous_openai = std::env::var_os(test_openai_name);
        let temp = tempfile::tempdir().unwrap();
        let codex_home_guard = CodexHomeEnvGuard::set(temp.path());
        unsafe {
            std::env::set_var(test_openai_name, "sk-test");
        }

        let check = check_env_conflicts();
        assert_eq!(check.status, "ok");
        assert!(
            check
                .payload
                .conflicts
                .iter()
                .any(|item| item.name == test_openai_name)
        );
        assert!(
            !check
                .payload
                .conflicts
                .iter()
                .any(|item| item.name == "CODEX_HOME")
        );

        codex_plus_core::env_conflicts::remove_process_env_conflicts_for_tests(
            &[test_openai_name.to_string(), "CODEX_HOME".to_string()],
            codex_plus_core::paths::default_app_state_dir().join("test-backups"),
        )
        .unwrap();
        assert!(std::env::var_os(test_openai_name).is_none());
        assert_eq!(
            std::env::var_os("CODEX_HOME"),
            Some(temp.path().as_os_str().to_os_string())
        );

        unsafe {
            match previous_openai {
                Some(value) => std::env::set_var(test_openai_name, value),
                None => std::env::remove_var(test_openai_name),
            }
        }
        drop(codex_home_guard);
    }

    #[test]
    fn delete_local_session_falls_back_when_requested_db_no_longer_contains_thread() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let stale_db = sqlite_dir.join("codex-dev.db");
        let active_db = sqlite_dir.join("state_5.sqlite");
        let rollout_path = temp.path().join("rollout.jsonl");
        std::fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
        let stale = rusqlite::Connection::open(&stale_db).unwrap();
        stale
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT)",
                [],
            )
            .unwrap();
        drop(stale);
        let active = rusqlite::Connection::open(&active_db).unwrap();
        active
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT)",
                [],
            )
            .unwrap();
        active
            .execute(
                "INSERT INTO threads VALUES ('t1', ?1, 'Active Thread')",
                [rollout_path.to_string_lossy().to_string()],
            )
            .unwrap();
        drop(active);

        let result = {
            let _codex_home_guard = CodexHomeEnvGuard::set(&codex_home);
            delete_local_session(DeleteLocalSessionRequest {
                session_id: "t1".to_string(),
                title: "Active Thread".to_string(),
                db_path: Some(stale_db.to_string_lossy().to_string()),
            })
        };

        assert_eq!(result.status, "ok");
        assert_eq!(
            result.payload.status,
            codex_plus_core::models::DeleteStatus::LocalDeleted
        );
        let active = rusqlite::Connection::open(&active_db).unwrap();
        assert_eq!(
            active
                .query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn list_local_sessions_deduplicates_threads_across_current_and_legacy_dbs() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let current_db = sqlite_dir.join("state_5.sqlite");
        let legacy_db = codex_home.join("state_5.sqlite");
        create_minimal_thread_db(&current_db, "t1", "Current Copy", 100);
        create_minimal_thread_db(&legacy_db, "t1", "Legacy Copy", 200);

        let result = {
            let _codex_home_guard = CodexHomeEnvGuard::set(&codex_home);
            list_local_sessions()
        };

        assert_eq!(result.status, "ok");
        assert_eq!(result.payload.sessions.len(), 1);
        assert_eq!(result.payload.sessions[0].id, "t1");
        assert_eq!(result.payload.sessions[0].title, "Legacy Copy");
        assert_eq!(
            result.payload.sessions[0].db_path,
            legacy_db.to_string_lossy()
        );
    }

    #[test]
    fn delete_local_session_removes_duplicate_threads_from_all_candidate_dbs() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join("codex-home");
        let sqlite_dir = codex_home.join("sqlite");
        std::fs::create_dir_all(&sqlite_dir).unwrap();
        let current_db = sqlite_dir.join("state_5.sqlite");
        let legacy_db = codex_home.join("state_5.sqlite");
        create_minimal_thread_db(&current_db, "t1", "Current Copy", 100);
        create_minimal_thread_db(&legacy_db, "t1", "Legacy Copy", 200);

        let result = {
            let _codex_home_guard = CodexHomeEnvGuard::set(&codex_home);
            delete_local_session(DeleteLocalSessionRequest {
                session_id: "t1".to_string(),
                title: "Legacy Copy".to_string(),
                db_path: Some(legacy_db.to_string_lossy().to_string()),
            })
        };

        assert_eq!(result.status, "ok");
        assert_eq!(thread_count(&current_db, "t1"), 0);
        assert_eq!(thread_count(&legacy_db, "t1"), 0);
    }

    fn create_minimal_thread_db(path: &Path, id: &str, title: &str, updated_at_ms: i64) {
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT, updated_at_ms INTEGER)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO threads VALUES (?1, '', ?2, ?3)",
            (id, title, updated_at_ms),
        )
        .unwrap();
    }

    fn thread_count(path: &Path, id: &str) -> i64 {
        let db = rusqlite::Connection::open(path).unwrap();
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = ?1", [id], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    }

    static CODEX_HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct CodexHomeEnvGuard {
        previous: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CodexHomeEnvGuard {
        fn set(path: &Path) -> Self {
            let lock = CODEX_HOME_ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let previous = std::env::var_os("CODEX_HOME");
            unsafe {
                std::env::set_var("CODEX_HOME", path);
            }
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for CodexHomeEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(value) = self.previous.take() {
                    std::env::set_var("CODEX_HOME", value);
                } else {
                    std::env::remove_var("CODEX_HOME");
                }
            }
        }
    }

    #[test]
    fn apply_relay_profile_to_home_with_switch_rules_preserves_custom_provider_id() {
        let temp = tempfile::tempdir().unwrap();
        let profile = RelayProfile {
            relay_mode: codex_plus_core::settings::RelayMode::PureApi,
            protocol: codex_plus_core::settings::RelayProtocol::Responses,
            config_contents: "model_provider = \"ai\"\nmodel = \"gpt-image-2\"\n\n[model_providers.ai]\nname = \"ai\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n"
                .to_string(),
            auth_contents: "{}\n".to_string(),
            ..RelayProfile::default()
        };

        codex_plus_core::relay_config::apply_relay_profile_to_home_with_switch_rules(
            temp.path(),
            &profile,
            "",
        )
        .unwrap();

        let applied = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
        assert!(applied.contains("model_provider = \"ai\""));
        assert!(applied.contains("[model_providers.ai]"));
        assert!(!applied.contains("[model_providers.custom]"));
    }

    #[test]
    fn save_relay_file_in_home_only_allows_known_files() {
        let temp = tempfile::tempdir().unwrap();

        save_relay_file_in_home(temp.path(), "config", "model = \"gpt-5\"\n").unwrap();
        save_relay_file_in_home(temp.path(), "auth", "{}\n").unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
            "model = \"gpt-5\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
            "{}\n"
        );
        assert!(save_relay_file_in_home(temp.path(), "../bad", "").is_err());
    }

    #[test]
    fn normalize_settings_before_save_preserves_profile_context_until_manual_extract() {
        let settings = BackendSettings {
            relay_common_config_contents: "[mcp_servers.context7]\ncommand = \"npx\"\n".to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: false,
                config_contents: "model = \"gpt-5\"\n\n[mcp_servers.context7]\ncommand = \"npx\"\n"
                    .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert!(
            normalized.relay_profiles[0]
                .config_contents
                .contains("model = \"gpt-5\"")
        );
        assert!(
            normalized.relay_profiles[0]
                .config_contents
                .contains("[mcp_servers.context7]")
        );
        assert!(
            normalized
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );
        assert!(
            !normalized
                .relay_common_config_contents
                .contains("[mcp_servers")
        );
    }

    #[test]
    fn normalize_settings_before_save_preserves_manual_relay_mode_for_pure_api_profile() {
        let settings = BackendSettings {
            active_relay_id: "api".to_string(),
            launch_mode: codex_plus_core::settings::LaunchMode::Relay,
            relay_profiles: vec![RelayProfile {
                id: "api".to_string(),
                relay_mode: codex_plus_core::settings::RelayMode::PureApi,
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert_eq!(
            normalized.launch_mode,
            codex_plus_core::settings::LaunchMode::Relay
        );
    }

    #[test]
    fn reset_image_overlay_settings_preserves_supplier_settings() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let previous = codex_plus_core::paths::set_settings_path_for_tests(Some(settings_path));

        let settings = BackendSettings {
            codex_app_image_overlay_enabled: true,
            codex_app_image_overlay_path: "C:\\Users\\me\\Pictures\\overlay.png".to_string(),
            codex_app_image_overlay_opacity: 42,
            codex_app_image_overlay_fit_mode: "fill".to_string(),
            active_relay_id: "supplier-a".to_string(),
            relay_profiles: vec![RelayProfile {
                id: "supplier-a".to_string(),
                name: "供应商 A".to_string(),
                relay_mode: codex_plus_core::settings::RelayMode::PureApi,
                api_key: "sk-test".to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };
        SettingsStore::default().save(&settings).unwrap();

        let result = reset_image_overlay_settings();
        codex_plus_core::paths::set_settings_path_for_tests(previous);

        assert_eq!(result.status, "ok");
        assert!(!result.payload.settings.codex_app_image_overlay_enabled);
        assert_eq!(result.payload.settings.codex_app_image_overlay_path, "");
        assert_eq!(result.payload.settings.codex_app_image_overlay_opacity, 35);
        assert_eq!(
            result.payload.settings.codex_app_image_overlay_fit_mode,
            "fit"
        );
        assert_eq!(result.payload.settings.active_relay_id, "supplier-a");
        assert_eq!(result.payload.settings.relay_profiles.len(), 1);
        assert_eq!(result.payload.settings.relay_profiles[0].id, "supplier-a");
        assert_eq!(result.payload.settings.relay_profiles[0].api_key, "sk-test");
    }

    #[test]
    fn normalize_settings_before_save_preserves_official_profile_auth() {
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                relay_mode: codex_plus_core::settings::RelayMode::Official,
                official_mix_api_key: false,
                auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"edited"}}"#
                    .to_string(),
                config_contents: "model_provider = \"custom\"\n".to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        let auth_json: serde_json::Value =
            serde_json::from_str(&normalized.relay_profiles[0].auth_contents).unwrap();
        assert_eq!(
            auth_json,
            serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "edited"
                }
            })
        );
        assert!(normalized.relay_profiles[0].config_contents.is_empty());
    }

    #[test]
    fn normalize_settings_before_save_strips_common_from_enabled_profile() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_reasoning_effort = "high"

[features]
goals = true

[plugins."superpowers@openai-curated"]
enabled = true
"#
            .to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: true,
                config_contents: r#"model = "gpt-5"
model_reasoning_effort = "high"

[features]
goals = true
model_reasoning_effort = "high"

[plugins."superpowers@openai-curated"]
enabled = true
"#
                .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);
        let config = &normalized.relay_profiles[0].config_contents;

        assert!(config.contains("model = \"gpt-5\""));
        assert!(!config.contains("model_reasoning_effort"));
        assert!(!config.contains("[features]"));
        assert!(!config.contains("[plugins.\"superpowers@openai-curated\"]"));
    }

    #[test]
    fn normalize_settings_before_save_repairs_invalid_profile_common_duplication() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_reasoning_effort = "high"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#
            .to_string(),
            relay_profiles: vec![RelayProfile {
                use_common_config: true,
                config_contents: r#"model = "gpt-5"
model_reasoning_effort = "high"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#
                .to_string(),
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);
        let config = &normalized.relay_profiles[0].config_contents;

        assert!(config.contains("model = \"gpt-5\""));
        assert!(!config.contains("model_reasoning_effort"));
        assert!(!config.contains("[marketplaces.openai-bundled]"));
    }

    #[test]
    fn normalize_settings_before_save_removes_model_catalog_from_common_config() {
        let settings = BackendSettings {
            relay_common_config_contents: r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-b.json'
model_reasoning_effort = "high"
"#
            .to_string(),
            ..BackendSettings::default()
        };

        let normalized = normalize_settings_before_save(settings);

        assert!(
            !normalized
                .relay_common_config_contents
                .contains("model_catalog_json")
        );
        assert!(
            normalized
                .relay_common_config_contents
                .contains("model_reasoning_effort = \"high\"")
        );
    }

    #[test]
    fn context_entry_commands_update_settings_payload() {
        let settings = BackendSettings::default();
        let upsert = upsert_context_entry(ContextEntryRequest {
            settings: settings.clone(),
            kind: "mcp".to_string(),
            id: "context7".to_string(),
            toml_body: "command = \"npx\"\n".to_string(),
        });

        assert_eq!(upsert.status, "ok");
        assert!(
            upsert
                .payload
                .settings
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );

        let listed = list_context_entries(ContextSettingsRequest {
            settings: upsert.payload.settings.clone(),
        });
        assert_eq!(listed.payload.entries.mcp_servers[0].id, "context7");

        let deleted = delete_context_entry(ContextDeleteRequest {
            settings: upsert.payload.settings,
            kind: "mcp".to_string(),
            id: "context7".to_string(),
        });
        assert_eq!(deleted.status, "ok");
        assert!(
            !deleted
                .payload
                .settings
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );
    }

    #[test]
    fn context_entry_commands_preserve_payload_shape_fallback_and_redaction() {
        let secret = "tauri-context-secret-sentinel";
        let request = ContextEntryRequest {
            settings: BackendSettings::default(),
            kind: "plugin".to_string(),
            id: "browser".to_string(),
            toml_body: format!("token = \"{secret}\"\n"),
        };
        assert!(!format!("{request:?}").contains(secret));

        let result = upsert_context_entry(request);
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(
            json["entries"]["plugins"][0]["tomlBody"],
            format!("token = \"{secret}\"\n")
        );
        assert!(json["entries"]["plugins"][0].get("toml_body").is_none());

        let invalid = upsert_context_entry(ContextEntryRequest {
            settings: BackendSettings::default(),
            kind: "mcp".to_string(),
            id: "broken".to_string(),
            toml_body: "command = [".to_string(),
        });
        assert_eq!(invalid.status, "failed");
        assert!(invalid.payload.entries.mcp_servers.is_empty());
        assert!(
            invalid
                .payload
                .settings
                .relay_context_config_contents
                .is_empty()
        );
    }

    #[test]
    fn context_entry_transform_commands_do_not_persist_settings() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let store = SettingsStore::new(settings_path.clone());
        store.save(&BackendSettings::default()).unwrap();
        let bytes_before = std::fs::read(&settings_path).unwrap();

        let upsert = upsert_context_entry(ContextEntryRequest {
            settings: BackendSettings::default(),
            kind: "skill".to_string(),
            id: "writer".to_string(),
            toml_body: "enabled = true\n".to_string(),
        });
        let deleted = delete_context_entry(ContextDeleteRequest {
            settings: upsert.payload.settings,
            kind: "skill".to_string(),
            id: "writer".to_string(),
        });

        assert_eq!(upsert.status, "ok");
        assert_eq!(deleted.status, "ok");
        assert_eq!(std::fs::read(&settings_path).unwrap(), bytes_before);
    }

    #[test]
    fn context_live_adapter_uses_isolated_service_and_global_scope() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let home = temp.path().join("codex");
        std::fs::create_dir(&home).unwrap();
        let settings = BackendSettings {
            active_relay_id: "relay-a".to_string(),
            relay_profiles_enabled: true,
            relay_profiles: vec![RelayProfile {
                id: "relay-a".to_string(),
                context_selection_initialized: true,
                context_selection: codex_plus_core::settings::RelayContextSelection::default(),
                ..RelayProfile::default()
            }],
            relay_context_config_contents: r#"[plugins.browser]
enabled = true
token = "global"
"#
            .to_string(),
            ..BackendSettings::default()
        };
        SettingsStore::new(settings_path.clone())
            .save(&settings)
            .unwrap();
        std::fs::write(home.join("config.toml"), "model = \"gpt\"\n").unwrap();
        let service = codex_plus_manager_service::ContextToolsService::new(
            SystemProviderEnvironment::for_paths(settings_path, home.clone()),
        );

        let synced = sync_live_context_entries_with_service(
            &service,
            ContextSettingsRequest {
                settings: settings.clone(),
            },
        );
        assert_eq!(synced.status, "ok");
        assert_eq!(synced.payload.entries.plugins[0].id, "browser");
        assert!(
            std::fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains("[plugins.browser]")
        );

        let read = read_live_context_entries_with_service(&service);
        assert_eq!(read.status, "ok");
        assert_eq!(
            read.payload.entries.plugins[0].toml_body,
            "enabled = true\ntoken = \"global\"\n"
        );
    }

    #[test]
    fn open_external_url_rejects_non_http_urls() {
        let result = open_external_url("file:///C:/Windows/win.ini".to_string());

        assert_eq!(result.status, "failed");
        assert!(result.message.contains("只允许打开 http 或 https 链接"));
    }
}
