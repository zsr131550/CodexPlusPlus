use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use codex_plus_core::install::EntryPointState;
use codex_plus_core::settings::BackendSettings;
use codex_plus_core::status::LaunchStatus;
use serde::Serialize;
use serde_json::Value;

use crate::revision_ledger::{RevisionLedger, RevisionScope, RevisionTicket, scoped_fingerprint};

const MAX_LOG_BYTES: usize = 256 * 1024;
const MAX_LOG_LINES: usize = 200;
const MAX_LOG_LINE_BYTES: usize = 16 * 1024;
const MAX_FORMATTED_LOG_BYTES: usize = 256 * 1024;
const APP_PATH_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/app-path/v1";

#[derive(Clone, PartialEq, Eq)]
pub struct PrivatePath(String);

impl PrivatePath {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn trimmed(&self) -> Self {
        Self(self.0.trim().to_owned())
    }
}

impl fmt::Debug for PrivatePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivatePath([redacted])")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppPathRevision(RevisionTicket);

impl fmt::Debug for AppPathRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AppPathRevision([opaque])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RevisionedAppPath {
    pub revision: AppPathRevision,
    pub value: PrivatePath,
    pub configured: bool,
}

impl fmt::Debug for RevisionedAppPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevisionedAppPath")
            .field("revision", &self.revision)
            .field("configured", &self.configured)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum SectionValue<T> {
    Available(T),
    Unavailable(MaintenanceSection),
}

impl<T> SectionValue<T> {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }

    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Available(value) => Some(value),
            Self::Unavailable(_) => None,
        }
    }
}

impl<T> fmt::Debug for SectionValue<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Available(_) => formatter.write_str("Available(..)"),
            Self::Unavailable(section) => {
                formatter.debug_tuple("Unavailable").field(section).finish()
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CodexAppSummary {
    pub found: bool,
    pub version: Option<String>,
}

impl fmt::Debug for CodexAppSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexAppSummary")
            .field("found", &self.found)
            .field("has_version", &self.version.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntrypointSummary {
    pub silent_installed: bool,
    pub management_installed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatcherSummary {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchState {
    Starting,
    Running,
    Ready,
    Failed,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchSummary {
    pub status: LaunchState,
    pub started_at_ms: u64,
    pub debug_port: Option<u16>,
    pub helper_port: Option<u16>,
    pub app_path_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceIssue {
    pub section: MaintenanceSection,
    pub kind: MaintenanceErrorKind,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MaintenanceWorkspace {
    pub app_path: Option<RevisionedAppPath>,
    pub codex_app: SectionValue<CodexAppSummary>,
    pub entrypoints: SectionValue<EntrypointSummary>,
    pub watcher: SectionValue<WatcherSummary>,
    pub latest_launch: SectionValue<Option<LaunchSummary>>,
    pub logs: SectionValue<SafeLogDocument>,
    pub diagnostics: SafeDiagnosticDocument,
    pub issues: Vec<MaintenanceIssue>,
}

impl fmt::Debug for MaintenanceWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MaintenanceWorkspace")
            .field(
                "app_path_configured",
                &self.app_path.as_ref().is_some_and(|path| path.configured),
            )
            .field("codex_app_available", &self.codex_app.is_available())
            .field("entrypoints_available", &self.entrypoints.is_available())
            .field("watcher_available", &self.watcher.is_available())
            .field(
                "latest_launch_available",
                &self.latest_launch.is_available(),
            )
            .field("logs_available", &self.logs.is_available())
            .field("diagnostics", &self.diagnostics)
            .field("issue_count", &self.issues.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadMaintenance {
    pub log_lines: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SaveCodexAppPath {
    pub expected_revision: AppPathRevision,
    pub path: PrivatePath,
    pub confirmed_clear: bool,
}

impl fmt::Debug for SaveCodexAppPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SaveCodexAppPath")
            .field("expected_revision", &self.expected_revision)
            .field("path_configured", &!self.path.as_str().trim().is_empty())
            .field("confirmed_clear", &self.confirmed_clear)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchValidationPolicy {
    Strict,
    Compatibility,
}

#[derive(Clone, PartialEq, Eq)]
pub struct LaunchCodex {
    app_path: PrivatePath,
    debug_port: u16,
    helper_port: u16,
    validation: LaunchValidationPolicy,
}

impl LaunchCodex {
    pub fn native(app_path: PrivatePath, debug_port: u16, helper_port: u16) -> Self {
        Self {
            app_path,
            debug_port,
            helper_port,
            validation: LaunchValidationPolicy::Strict,
        }
    }

    pub fn compatibility(app_path: PrivatePath, debug_port: u16, helper_port: u16) -> Self {
        Self {
            app_path,
            debug_port,
            helper_port,
            validation: LaunchValidationPolicy::Compatibility,
        }
    }
}

impl fmt::Debug for LaunchCodex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaunchCodex")
            .field(
                "path_configured",
                &!self.app_path.as_str().trim().is_empty(),
            )
            .field("debug_port", &self.debug_port)
            .field("helper_port", &self.helper_port)
            .field("validation", &self.validation)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOutcome {
    pub debug_port: u16,
    pub helper_port: u16,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Missing,
    File,
    Directory,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticPathPresence {
    pub settings: bool,
    pub logs: bool,
    pub latest_status: bool,
}

pub struct CodexLaunchPlan {
    arguments: Vec<OsString>,
    debug_port: u16,
    helper_port: u16,
    path_configured: bool,
}

impl fmt::Debug for CodexLaunchPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexLaunchPlan")
            .field("debug_port", &self.debug_port)
            .field("helper_port", &self.helper_port)
            .field("path_configured", &self.path_configured)
            .field("argument_count", &self.arguments.len())
            .finish()
    }
}

impl CodexLaunchPlan {
    pub fn debug_port(&self) -> u16 {
        self.debug_port
    }

    pub fn helper_port(&self) -> u16 {
        self.helper_port
    }

    pub fn path_configured(&self) -> bool {
        self.path_configured
    }

    pub fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    pub(crate) fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
}

pub trait MaintenanceEnvironment: Send + Sync + 'static {
    fn load_maintenance_settings(&self) -> anyhow::Result<BackendSettings>;
    fn update_maintenance_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool;
    fn inspect_path(&self, path: &Path) -> anyhow::Result<PathKind>;
    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf>;
    fn codex_app_version(&self, path: &Path) -> Option<String>;
    fn inspect_entrypoints(&self) -> anyhow::Result<EntryPointState>;
    fn watcher_disabled(&self) -> anyhow::Result<bool>;
    fn load_latest_launch(&self) -> anyhow::Result<Option<LaunchStatus>>;
    fn read_diagnostic_tail(&self, max_bytes: usize) -> anyhow::Result<Vec<u8>>;
    fn diagnostic_path_presence(&self) -> DiagnosticPathPresence;
    fn launch_codex(&self, plan: &CodexLaunchPlan) -> anyhow::Result<()>;
}

pub trait CodexLaunchExecutor: Send + Sync + 'static {
    fn launch(&self, plan: &CodexLaunchPlan) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeLogSeverity {
    Info,
    Warning,
    Error,
}

impl SafeLogSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeLogEvent {
    ManagerLaunchRequested,
    NativeMaintenanceLoad,
    NativeMaintenanceSavePath,
    NativeMaintenanceLaunch,
    NativeSettingsLoad,
    NativeSettingsSave,
    NativeSettingsReset,
    NativeSettingsTest,
    RendererEvent,
    LauncherEvent,
    Other,
}

impl SafeLogEvent {
    fn from_raw(value: &str) -> Self {
        match value {
            "manager.launch_requested" => Self::ManagerLaunchRequested,
            "native.maintenance.load" => Self::NativeMaintenanceLoad,
            "native.maintenance.save_path" => Self::NativeMaintenanceSavePath,
            "native.maintenance.launch" => Self::NativeMaintenanceLaunch,
            "native.settings.load" => Self::NativeSettingsLoad,
            "native.settings.save" => Self::NativeSettingsSave,
            "native.settings.reset" => Self::NativeSettingsReset,
            "native.settings.test" => Self::NativeSettingsTest,
            value if value.starts_with("renderer.") => Self::RendererEvent,
            value if value.starts_with("launcher.") => Self::LauncherEvent,
            _ => Self::Other,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ManagerLaunchRequested => "manager.launch_requested",
            Self::NativeMaintenanceLoad => "native.maintenance.load",
            Self::NativeMaintenanceSavePath => "native.maintenance.save_path",
            Self::NativeMaintenanceLaunch => "native.maintenance.launch",
            Self::NativeSettingsLoad => "native.settings.load",
            Self::NativeSettingsSave => "native.settings.save",
            Self::NativeSettingsReset => "native.settings.reset",
            Self::NativeSettingsTest => "native.settings.test",
            Self::RendererEvent => "renderer.event",
            Self::LauncherEvent => "launcher.event",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeSettingsGroup {
    Stepwise,
    ImageOverlay,
    ExtraArgs,
}

impl SafeSettingsGroup {
    fn from_raw(value: &str) -> Option<Self> {
        match value {
            "stepwise" => Some(Self::Stepwise),
            "image_overlay" | "imageOverlay" => Some(Self::ImageOverlay),
            "extra_args" | "extraArgs" => Some(Self::ExtraArgs),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Stepwise => "stepwise",
            Self::ImageOverlay => "image_overlay",
            Self::ExtraArgs => "extra_args",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeErrorKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    InvalidPath,
    InvalidPort,
    EntrypointReadFailed,
    WatcherReadFailed,
    StatusReadFailed,
    LogReadFailed,
    LaunchFailed,
    ValidationFailed,
    RequestFailed,
    WorkerStopped,
    Other,
}

impl SafeErrorKind {
    fn from_raw(value: &str) -> Option<Self> {
        match value {
            "settings_read_failed" | "SettingsReadFailed" => Some(Self::SettingsReadFailed),
            "settings_write_failed" | "SettingsWriteFailed" => Some(Self::SettingsWriteFailed),
            "settings_conflict" | "SettingsConflict" => Some(Self::SettingsConflict),
            "invalid_revision" | "InvalidRevision" => Some(Self::InvalidRevision),
            "invalid_path" | "InvalidPath" => Some(Self::InvalidPath),
            "invalid_port" | "InvalidPort" => Some(Self::InvalidPort),
            "entrypoint_read_failed" | "EntrypointReadFailed" => Some(Self::EntrypointReadFailed),
            "watcher_read_failed" | "WatcherReadFailed" => Some(Self::WatcherReadFailed),
            "status_read_failed" | "StatusReadFailed" => Some(Self::StatusReadFailed),
            "log_read_failed" | "LogReadFailed" => Some(Self::LogReadFailed),
            "launch_failed" | "LaunchFailed" => Some(Self::LaunchFailed),
            "validation_failed" | "ValidationFailed" => Some(Self::ValidationFailed),
            "request_failed" | "RequestFailed" => Some(Self::RequestFailed),
            "worker_stopped" | "WorkerStopped" => Some(Self::WorkerStopped),
            "other" | "Other" => Some(Self::Other),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SettingsReadFailed => "settings_read_failed",
            Self::SettingsWriteFailed => "settings_write_failed",
            Self::SettingsConflict => "settings_conflict",
            Self::InvalidRevision => "invalid_revision",
            Self::InvalidPath => "invalid_path",
            Self::InvalidPort => "invalid_port",
            Self::EntrypointReadFailed => "entrypoint_read_failed",
            Self::WatcherReadFailed => "watcher_read_failed",
            Self::StatusReadFailed => "status_read_failed",
            Self::LogReadFailed => "log_read_failed",
            Self::LaunchFailed => "launch_failed",
            Self::ValidationFailed => "validation_failed",
            Self::RequestFailed => "request_failed",
            Self::WorkerStopped => "worker_stopped",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeLogField {
    DebugPort(u16),
    HelperPort(u16),
    RequestId(u64),
    Count(u64),
    Enabled(bool),
    Success(bool),
    Group(SafeSettingsGroup),
    ErrorKind(SafeErrorKind),
}

impl SafeLogField {
    fn order(self) -> u8 {
        match self {
            Self::DebugPort(_) => 0,
            Self::HelperPort(_) => 1,
            Self::RequestId(_) => 2,
            Self::Count(_) => 3,
            Self::Enabled(_) => 4,
            Self::Success(_) => 5,
            Self::Group(_) => 6,
            Self::ErrorKind(_) => 7,
        }
    }

    fn write_to(self, output: &mut String) {
        match self {
            Self::DebugPort(value) => output.push_str(&format!("debug_port={value}")),
            Self::HelperPort(value) => output.push_str(&format!("helper_port={value}")),
            Self::RequestId(value) => output.push_str(&format!("request_id={value}")),
            Self::Count(value) => output.push_str(&format!("count={value}")),
            Self::Enabled(value) => output.push_str(&format!("enabled={value}")),
            Self::Success(value) => output.push_str(&format!("success={value}")),
            Self::Group(value) => {
                output.push_str("group=");
                output.push_str(value.as_str());
            }
            Self::ErrorKind(value) => {
                output.push_str("error_kind=");
                output.push_str(value.as_str());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeLogRecord {
    pub timestamp_ms: u64,
    pub event: SafeLogEvent,
    pub severity: SafeLogSeverity,
    pub fields: Vec<SafeLogField>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SafeLogDocument {
    records: Vec<SafeLogRecord>,
    requested_lines: usize,
    effective_lines: usize,
    dropped_lines: usize,
    input_truncated: bool,
    text: String,
}

impl SafeLogDocument {
    pub fn records(&self) -> &[SafeLogRecord] {
        &self.records
    }

    pub fn requested_lines(&self) -> usize {
        self.requested_lines
    }

    pub fn effective_lines(&self) -> usize {
        self.effective_lines
    }

    pub fn parsed_lines(&self) -> usize {
        self.records.len()
    }

    pub fn dropped_lines(&self) -> usize {
        self.dropped_lines
    }

    pub fn input_truncated(&self) -> bool {
        self.input_truncated
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for SafeLogDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeLogDocument")
            .field("requested_lines", &self.requested_lines)
            .field("effective_lines", &self.effective_lines)
            .field("parsed_lines", &self.records.len())
            .field("dropped_lines", &self.dropped_lines)
            .field("input_truncated", &self.input_truncated)
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SafeDiagnosticDocument {
    text: String,
    top_level_field_count: usize,
}

impl SafeDiagnosticDocument {
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for SafeDiagnosticDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeDiagnosticDocument")
            .field("text_bytes", &self.text.len())
            .field("top_level_field_count", &self.top_level_field_count)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceSection {
    Settings,
    CodexApp,
    Entrypoints,
    Watcher,
    LatestLaunch,
    Logs,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceErrorKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    InvalidPath,
    InvalidPort,
    EntrypointReadFailed,
    WatcherReadFailed,
    StatusReadFailed,
    LogReadFailed,
    LaunchFailed,
    WorkerStopped,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MaintenanceError {
    kind: MaintenanceErrorKind,
    section: Option<MaintenanceSection>,
    refreshed_workspace: Option<Box<MaintenanceWorkspace>>,
}

impl MaintenanceError {
    pub fn new(kind: MaintenanceErrorKind, section: Option<MaintenanceSection>) -> Self {
        Self {
            kind,
            section,
            refreshed_workspace: None,
        }
    }

    pub fn kind(&self) -> MaintenanceErrorKind {
        self.kind
    }

    pub fn section(&self) -> Option<MaintenanceSection> {
        self.section
    }

    pub fn refreshed_workspace(&self) -> Option<&MaintenanceWorkspace> {
        self.refreshed_workspace.as_deref()
    }

    fn with_refreshed_workspace(mut self, workspace: Option<MaintenanceWorkspace>) -> Self {
        self.refreshed_workspace = workspace.map(Box::new);
        self
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            MaintenanceErrorKind::SettingsReadFailed => "maintenance settings read failed",
            MaintenanceErrorKind::SettingsWriteFailed => "maintenance settings write failed",
            MaintenanceErrorKind::SettingsConflict => "maintenance settings changed on disk",
            MaintenanceErrorKind::InvalidRevision => "maintenance revision is invalid",
            MaintenanceErrorKind::InvalidPath => "maintenance path is invalid",
            MaintenanceErrorKind::InvalidPort => "maintenance port is invalid",
            MaintenanceErrorKind::EntrypointReadFailed => "maintenance entrypoint read failed",
            MaintenanceErrorKind::WatcherReadFailed => "maintenance watcher read failed",
            MaintenanceErrorKind::StatusReadFailed => "maintenance status read failed",
            MaintenanceErrorKind::LogReadFailed => "maintenance log read failed",
            MaintenanceErrorKind::LaunchFailed => "maintenance launch failed",
            MaintenanceErrorKind::WorkerStopped => "maintenance worker stopped",
        }
    }
}

impl fmt::Debug for MaintenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MaintenanceError")
            .field("kind", &self.kind)
            .field("section", &self.section)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

impl fmt::Display for MaintenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for MaintenanceError {}

#[derive(Clone)]
pub struct MaintenanceService<E> {
    environment: E,
    revisions: Arc<RevisionLedger>,
    selected_log_lines: Arc<AtomicUsize>,
}

impl<E> MaintenanceService<E> {
    pub fn new(environment: E) -> Self {
        Self {
            environment,
            revisions: Arc::new(RevisionLedger::default()),
            selected_log_lines: Arc::new(AtomicUsize::new(MAX_LOG_LINES)),
        }
    }
}

impl<E: MaintenanceEnvironment> MaintenanceService<E> {
    pub fn load_workspace(
        &self,
        request: LoadMaintenance,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        let mut issues = Vec::new();
        let settings = match self.environment.load_maintenance_settings() {
            Ok(settings) => Some(settings),
            Err(_) => {
                issues.push(MaintenanceIssue {
                    section: MaintenanceSection::Settings,
                    kind: MaintenanceErrorKind::SettingsReadFailed,
                });
                None
            }
        };
        let app_path = settings
            .as_ref()
            .map(|settings| self.revisioned_app_path(settings));
        let saved_path = settings
            .as_ref()
            .map_or("", |settings| settings.codex_app_path.as_str());
        let resolved_app = self.environment.resolve_codex_app(saved_path);
        let codex_app = SectionValue::Available(CodexAppSummary {
            found: resolved_app.is_some(),
            version: resolved_app
                .as_deref()
                .and_then(|path| self.environment.codex_app_version(path)),
        });
        let entrypoints = match self.environment.inspect_entrypoints() {
            Ok(state) => SectionValue::Available(EntrypointSummary {
                silent_installed: state.silent_shortcut.installed,
                management_installed: state.management_shortcut.installed,
            }),
            Err(_) => {
                issues.push(MaintenanceIssue {
                    section: MaintenanceSection::Entrypoints,
                    kind: MaintenanceErrorKind::EntrypointReadFailed,
                });
                SectionValue::Unavailable(MaintenanceSection::Entrypoints)
            }
        };
        let watcher = match self.environment.watcher_disabled() {
            Ok(disabled) => SectionValue::Available(WatcherSummary { enabled: !disabled }),
            Err(_) => {
                issues.push(MaintenanceIssue {
                    section: MaintenanceSection::Watcher,
                    kind: MaintenanceErrorKind::WatcherReadFailed,
                });
                SectionValue::Unavailable(MaintenanceSection::Watcher)
            }
        };
        let latest_launch = match self.environment.load_latest_launch() {
            Ok(status) => SectionValue::Available(status.map(safe_launch_summary)),
            Err(_) => {
                issues.push(MaintenanceIssue {
                    section: MaintenanceSection::LatestLaunch,
                    kind: MaintenanceErrorKind::StatusReadFailed,
                });
                SectionValue::Unavailable(MaintenanceSection::LatestLaunch)
            }
        };
        let logs = match self.load_logs(request.log_lines) {
            Ok(document) => SectionValue::Available(document),
            Err(_) => {
                issues.push(MaintenanceIssue {
                    section: MaintenanceSection::Logs,
                    kind: MaintenanceErrorKind::LogReadFailed,
                });
                SectionValue::Unavailable(MaintenanceSection::Logs)
            }
        };
        let diagnostics = self.build_diagnostics()?;

        Ok(MaintenanceWorkspace {
            app_path,
            codex_app,
            entrypoints,
            watcher,
            latest_launch,
            logs,
            diagnostics,
            issues,
        })
    }

    pub fn save_app_path(
        &self,
        request: SaveCodexAppPath,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        let path = request.path.trimmed();
        if path.is_empty() {
            if !request.confirmed_clear {
                return Err(MaintenanceError::new(
                    MaintenanceErrorKind::InvalidPath,
                    Some(MaintenanceSection::Settings),
                ));
            }
        } else {
            let kind = self
                .environment
                .inspect_path(Path::new(path.as_str()))
                .map_err(|_| {
                    MaintenanceError::new(
                        MaintenanceErrorKind::InvalidPath,
                        Some(MaintenanceSection::Settings),
                    )
                })?;
            if !matches!(kind, PathKind::File | PathKind::Directory) {
                return Err(MaintenanceError::new(
                    MaintenanceErrorKind::InvalidPath,
                    Some(MaintenanceSection::Settings),
                ));
            }
        }

        let expected = self
            .revisions
            .take(request.expected_revision.0, RevisionScope::AppPath)
            .ok_or_else(|| {
                MaintenanceError::new(
                    MaintenanceErrorKind::InvalidRevision,
                    Some(MaintenanceSection::Settings),
                )
            })?;
        let updated = self
            .environment
            .update_maintenance_settings_if(
                serde_json::json!({ "codexAppPath": path.as_str() }),
                move |current| app_path_fingerprint(current) == expected,
            )
            .map_err(|_| {
                MaintenanceError::new(
                    MaintenanceErrorKind::SettingsWriteFailed,
                    Some(MaintenanceSection::Settings),
                )
            })?;
        if updated.is_none() {
            let fresh = self
                .load_workspace(LoadMaintenance {
                    log_lines: self.selected_log_lines.load(Ordering::Relaxed),
                })
                .ok();
            return Err(MaintenanceError::new(
                MaintenanceErrorKind::SettingsConflict,
                Some(MaintenanceSection::Settings),
            )
            .with_refreshed_workspace(fresh));
        }
        self.load_workspace(LoadMaintenance {
            log_lines: self.selected_log_lines.load(Ordering::Relaxed),
        })
    }

    pub fn launch(&self, request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError> {
        let app_path = request.app_path.trimmed();
        if request.validation == LaunchValidationPolicy::Strict {
            if request.debug_port == 0 || request.helper_port == 0 {
                return Err(MaintenanceError::new(
                    MaintenanceErrorKind::InvalidPort,
                    None,
                ));
            }
            if !app_path.is_empty() {
                let kind = self
                    .environment
                    .inspect_path(Path::new(app_path.as_str()))
                    .map_err(|_| MaintenanceError::new(MaintenanceErrorKind::InvalidPath, None))?;
                if !matches!(kind, PathKind::File | PathKind::Directory) {
                    return Err(MaintenanceError::new(
                        MaintenanceErrorKind::InvalidPath,
                        None,
                    ));
                }
            }
        }

        let mut arguments = Vec::with_capacity(if app_path.is_empty() { 4 } else { 6 });
        if !app_path.is_empty() {
            arguments.push(OsString::from("--app-path"));
            arguments.push(OsString::from(app_path.as_str()));
        }
        arguments.push(OsString::from("--debug-port"));
        arguments.push(OsString::from(request.debug_port.to_string()));
        arguments.push(OsString::from("--helper-port"));
        arguments.push(OsString::from(request.helper_port.to_string()));
        let plan = CodexLaunchPlan {
            arguments,
            debug_port: request.debug_port,
            helper_port: request.helper_port,
            path_configured: !app_path.is_empty(),
        };
        self.environment
            .launch_codex(&plan)
            .map_err(|_| MaintenanceError::new(MaintenanceErrorKind::LaunchFailed, None))?;
        Ok(LaunchOutcome {
            debug_port: request.debug_port,
            helper_port: request.helper_port,
            accepted: true,
        })
    }

    fn revisioned_app_path(&self, settings: &BackendSettings) -> RevisionedAppPath {
        RevisionedAppPath {
            revision: AppPathRevision(
                self.revisions
                    .issue(RevisionScope::AppPath, app_path_fingerprint(settings)),
            ),
            value: PrivatePath::new(settings.codex_app_path.clone()),
            configured: !settings.codex_app_path.trim().is_empty(),
        }
    }

    pub fn load_logs(&self, requested_lines: usize) -> Result<SafeLogDocument, MaintenanceError> {
        let effective_lines = effective_log_lines(requested_lines);
        self.selected_log_lines
            .store(effective_lines, Ordering::Relaxed);
        let raw = self
            .environment
            .read_diagnostic_tail(MAX_LOG_BYTES)
            .map_err(|_| {
                MaintenanceError::new(
                    MaintenanceErrorKind::LogReadFailed,
                    Some(MaintenanceSection::Logs),
                )
            })?;
        Ok(parse_safe_log_tail(&raw, requested_lines))
    }

    pub fn build_diagnostics(&self) -> Result<SafeDiagnosticDocument, MaintenanceError> {
        let settings = self.environment.load_maintenance_settings();
        let codex_app = match &settings {
            Ok(settings) => self
                .environment
                .resolve_codex_app(&settings.codex_app_path)
                .map_or(DiagnosticPresence::Missing, |_| DiagnosticPresence::Found),
            Err(_) => DiagnosticPresence::Unknown,
        };
        let entrypoints = self
            .environment
            .inspect_entrypoints()
            .map(|state| DiagnosticEntrypoints {
                silent: DiagnosticPresence::from_bool(state.silent_shortcut.installed),
                management: DiagnosticPresence::from_bool(state.management_shortcut.installed),
            })
            .unwrap_or(DiagnosticEntrypoints {
                silent: DiagnosticPresence::Unknown,
                management: DiagnosticPresence::Unknown,
            });
        let watcher = self
            .environment
            .watcher_disabled()
            .map(|disabled| {
                if disabled {
                    DiagnosticWatcher::Disabled
                } else {
                    DiagnosticWatcher::Enabled
                }
            })
            .unwrap_or(DiagnosticWatcher::Unknown);
        let latest_launch = self
            .environment
            .load_latest_launch()
            .map(diagnostic_launch)
            .unwrap_or_else(|_| DiagnosticLaunch {
                status: DiagnosticLaunchStatus::Unknown,
                timestamp_present: false,
                debug_port_present: false,
                helper_port_present: false,
            });
        let logs = self
            .load_logs(self.selected_log_lines.load(Ordering::Relaxed))
            .ok();
        let paths = self.environment.diagnostic_path_presence();
        let configured = settings
            .as_ref()
            .map(DiagnosticConfigured::from_settings)
            .unwrap_or_default();
        let extra_args = settings
            .as_ref()
            .map(|settings| settings.codex_extra_args.len())
            .unwrap_or_default();
        let report = DiagnosticReport {
            version: env!("CARGO_PKG_VERSION"),
            platform: DiagnosticPlatform {
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
            },
            status: DiagnosticStatus {
                codex_app,
                entrypoints,
                watcher,
                latest_launch,
            },
            configured,
            counts: DiagnosticCounts {
                extra_args,
                logs_parsed: logs.as_ref().map_or(0, SafeLogDocument::parsed_lines),
                logs_dropped: logs.as_ref().map_or(0, SafeLogDocument::dropped_lines),
            },
            paths: DiagnosticPaths {
                settings_present: paths.settings,
                logs_present: paths.logs,
                status_present: paths.latest_status,
            },
        };
        let mut text = serde_json::to_string_pretty(&report).map_err(|_| {
            MaintenanceError::new(
                MaintenanceErrorKind::SettingsReadFailed,
                Some(MaintenanceSection::Diagnostics),
            )
        })?;
        text.push('\n');
        Ok(SafeDiagnosticDocument {
            text,
            top_level_field_count: 6,
        })
    }
}

pub trait MaintenanceSource: Send + Sync + 'static {
    fn load_workspace(
        &self,
        request: LoadMaintenance,
    ) -> Result<MaintenanceWorkspace, MaintenanceError>;
    fn load_logs(&self, requested_lines: usize) -> Result<SafeLogDocument, MaintenanceError>;
    fn build_diagnostics(&self) -> Result<SafeDiagnosticDocument, MaintenanceError>;
    fn save_app_path(
        &self,
        request: SaveCodexAppPath,
    ) -> Result<MaintenanceWorkspace, MaintenanceError>;
    fn launch(&self, request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError>;
}

impl<E: MaintenanceEnvironment> MaintenanceSource for MaintenanceService<E> {
    fn load_workspace(
        &self,
        request: LoadMaintenance,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        MaintenanceService::load_workspace(self, request)
    }

    fn load_logs(&self, requested_lines: usize) -> Result<SafeLogDocument, MaintenanceError> {
        MaintenanceService::load_logs(self, requested_lines)
    }

    fn build_diagnostics(&self) -> Result<SafeDiagnosticDocument, MaintenanceError> {
        MaintenanceService::build_diagnostics(self)
    }

    fn save_app_path(
        &self,
        request: SaveCodexAppPath,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        MaintenanceService::save_app_path(self, request)
    }

    fn launch(&self, request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError> {
        MaintenanceService::launch(self, request)
    }
}

#[derive(Serialize)]
struct CanonicalAppPath<'a> {
    value: &'a str,
}

fn app_path_fingerprint(settings: &BackendSettings) -> [u8; 32] {
    scoped_fingerprint(
        APP_PATH_FINGERPRINT_DOMAIN,
        &CanonicalAppPath {
            value: &settings.codex_app_path,
        },
    )
}

fn safe_launch_summary(status: LaunchStatus) -> LaunchSummary {
    let safe_status = match status.status.as_str() {
        "starting" => LaunchState::Starting,
        "running" => LaunchState::Running,
        "ready" => LaunchState::Ready,
        "failed" => LaunchState::Failed,
        "stopped" => LaunchState::Stopped,
        _ => LaunchState::Unknown,
    };
    LaunchSummary {
        status: safe_status,
        started_at_ms: status.started_at_ms,
        debug_port: status.debug_port,
        helper_port: status.helper_port,
        app_path_present: status.codex_app.is_some(),
    }
}

fn effective_log_lines(requested_lines: usize) -> usize {
    requested_lines.clamp(1, MAX_LOG_LINES)
}

fn parse_safe_log_tail(raw: &[u8], requested_lines: usize) -> SafeLogDocument {
    let effective_lines = effective_log_lines(requested_lines);
    let input_truncated = raw.len() > MAX_LOG_BYTES;
    let mut dropped_lines = usize::from(input_truncated);
    let mut tail = if input_truncated {
        &raw[raw.len() - MAX_LOG_BYTES..]
    } else {
        raw
    };
    if input_truncated {
        tail = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(&[][..], |index| &tail[index + 1..]);
    }

    let lines = tail.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    let nonempty_len = if lines.last().is_some_and(|line| line.is_empty()) {
        lines.len().saturating_sub(1)
    } else {
        lines.len()
    };
    let start = nonempty_len.saturating_sub(effective_lines);
    let mut records = Vec::new();
    for line in &lines[start..nonempty_len] {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_LOG_LINE_BYTES {
            dropped_lines += 1;
            continue;
        }
        match parse_safe_log_record(line) {
            Some(record) => records.push(record),
            None => dropped_lines += 1,
        }
    }

    let formatted = records.iter().map(format_record).collect::<Vec<_>>();
    let mut formatted_bytes = 0usize;
    let mut output_start = formatted.len();
    for (index, line) in formatted.iter().enumerate().rev() {
        if formatted_bytes.saturating_add(line.len()) > MAX_FORMATTED_LOG_BYTES {
            break;
        }
        formatted_bytes += line.len();
        output_start = index;
    }
    if output_start > 0 {
        dropped_lines += output_start;
        records.drain(..output_start);
    }
    let text = formatted[output_start..].concat();

    SafeLogDocument {
        records,
        requested_lines,
        effective_lines,
        dropped_lines,
        input_truncated,
        text,
    }
}

fn parse_safe_log_record(line: &[u8]) -> Option<SafeLogRecord> {
    let text = String::from_utf8_lossy(line);
    let value: Value = serde_json::from_str(&text).ok()?;
    let timestamp_ms = value.get("timestamp_ms")?.as_u64()?;
    let event = SafeLogEvent::from_raw(value.get("event")?.as_str()?);
    let detail = value.get("detail").and_then(Value::as_object);
    let mut fields = safe_fields(event, detail);
    fields.sort_by_key(|field| field.order());
    let severity = if fields.iter().any(|field| {
        matches!(
            field,
            SafeLogField::ErrorKind(_) | SafeLogField::Success(false)
        )
    }) {
        SafeLogSeverity::Error
    } else if event == SafeLogEvent::Other {
        SafeLogSeverity::Warning
    } else {
        SafeLogSeverity::Info
    };
    Some(SafeLogRecord {
        timestamp_ms,
        event,
        severity,
        fields,
    })
}

fn safe_fields(
    event: SafeLogEvent,
    detail: Option<&serde_json::Map<String, Value>>,
) -> Vec<SafeLogField> {
    let Some(detail) = detail else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    match event {
        SafeLogEvent::ManagerLaunchRequested | SafeLogEvent::NativeMaintenanceLaunch => {
            push_u16(detail, "debug_port", SafeLogField::DebugPort, &mut fields);
            push_u16(detail, "helper_port", SafeLogField::HelperPort, &mut fields);
        }
        _ => {}
    }
    match event {
        SafeLogEvent::NativeMaintenanceLoad
        | SafeLogEvent::NativeMaintenanceSavePath
        | SafeLogEvent::NativeMaintenanceLaunch
        | SafeLogEvent::NativeSettingsLoad
        | SafeLogEvent::NativeSettingsSave
        | SafeLogEvent::NativeSettingsReset
        | SafeLogEvent::NativeSettingsTest => {
            push_u64(detail, "request_id", SafeLogField::RequestId, &mut fields);
        }
        _ => {}
    }
    match event {
        SafeLogEvent::NativeMaintenanceLoad
        | SafeLogEvent::NativeSettingsSave
        | SafeLogEvent::NativeSettingsReset
        | SafeLogEvent::NativeSettingsTest => {
            push_u64(detail, "count", SafeLogField::Count, &mut fields);
        }
        _ => {}
    }
    match event {
        SafeLogEvent::NativeSettingsSave | SafeLogEvent::NativeSettingsReset => {
            if let Some(value) = detail.get("enabled").and_then(Value::as_bool) {
                fields.push(SafeLogField::Enabled(value));
            }
            if let Some(group) = detail
                .get("group")
                .and_then(Value::as_str)
                .and_then(SafeSettingsGroup::from_raw)
            {
                fields.push(SafeLogField::Group(group));
            }
        }
        _ => {}
    }
    match event {
        SafeLogEvent::NativeMaintenanceLoad
        | SafeLogEvent::NativeMaintenanceSavePath
        | SafeLogEvent::NativeMaintenanceLaunch
        | SafeLogEvent::NativeSettingsLoad
        | SafeLogEvent::NativeSettingsSave
        | SafeLogEvent::NativeSettingsReset
        | SafeLogEvent::NativeSettingsTest => {
            if let Some(value) = detail.get("success").and_then(Value::as_bool) {
                fields.push(SafeLogField::Success(value));
            }
            if let Some(kind) = detail
                .get("error_kind")
                .and_then(Value::as_str)
                .and_then(SafeErrorKind::from_raw)
            {
                fields.push(SafeLogField::ErrorKind(kind));
            }
        }
        _ => {}
    }
    fields
}

fn push_u16(
    detail: &serde_json::Map<String, Value>,
    key: &str,
    field: impl FnOnce(u16) -> SafeLogField,
    fields: &mut Vec<SafeLogField>,
) {
    if let Some(value) = detail
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
    {
        fields.push(field(value));
    }
}

fn push_u64(
    detail: &serde_json::Map<String, Value>,
    key: &str,
    field: impl FnOnce(u64) -> SafeLogField,
    fields: &mut Vec<SafeLogField>,
) {
    if let Some(value) = detail.get(key).and_then(Value::as_u64) {
        fields.push(field(value));
    }
}

fn format_record(record: &SafeLogRecord) -> String {
    let mut output = format!(
        "{} {} {}",
        record.timestamp_ms,
        record.severity.as_str(),
        record.event.as_str()
    );
    for field in &record.fields {
        output.push(' ');
        field.write_to(&mut output);
    }
    output.push('\n');
    output
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport {
    version: &'static str,
    platform: DiagnosticPlatform,
    status: DiagnosticStatus,
    configured: DiagnosticConfigured,
    counts: DiagnosticCounts,
    paths: DiagnosticPaths,
}

#[derive(Serialize)]
struct DiagnosticPlatform {
    os: &'static str,
    arch: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticStatus {
    codex_app: DiagnosticPresence,
    entrypoints: DiagnosticEntrypoints,
    watcher: DiagnosticWatcher,
    latest_launch: DiagnosticLaunch,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticPresence {
    Found,
    Missing,
    Unknown,
}

impl DiagnosticPresence {
    fn from_bool(value: bool) -> Self {
        if value { Self::Found } else { Self::Missing }
    }
}

#[derive(Serialize)]
struct DiagnosticEntrypoints {
    silent: DiagnosticPresence,
    management: DiagnosticPresence,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticWatcher {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticLaunch {
    status: DiagnosticLaunchStatus,
    timestamp_present: bool,
    debug_port_present: bool,
    helper_port_present: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticLaunchStatus {
    None,
    Starting,
    Running,
    Ready,
    Failed,
    Stopped,
    Unknown,
}

fn diagnostic_launch(status: Option<LaunchStatus>) -> DiagnosticLaunch {
    let Some(status) = status else {
        return DiagnosticLaunch {
            status: DiagnosticLaunchStatus::None,
            timestamp_present: false,
            debug_port_present: false,
            helper_port_present: false,
        };
    };
    let safe_status = match status.status.as_str() {
        "starting" => DiagnosticLaunchStatus::Starting,
        "running" => DiagnosticLaunchStatus::Running,
        "ready" => DiagnosticLaunchStatus::Ready,
        "failed" => DiagnosticLaunchStatus::Failed,
        "stopped" => DiagnosticLaunchStatus::Stopped,
        _ => DiagnosticLaunchStatus::Unknown,
    };
    DiagnosticLaunch {
        status: safe_status,
        timestamp_present: status.started_at_ms > 0,
        debug_port_present: status.debug_port.is_some(),
        helper_port_present: status.helper_port.is_some(),
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticConfigured {
    app_path: bool,
    stepwise_api_key: bool,
    stepwise_enabled: bool,
    image_overlay: bool,
}

impl DiagnosticConfigured {
    fn from_settings(settings: &BackendSettings) -> Self {
        Self {
            app_path: !settings.codex_app_path.trim().is_empty(),
            stepwise_api_key: !settings.codex_app_stepwise_api_key.trim().is_empty(),
            stepwise_enabled: settings.codex_app_stepwise_enabled,
            image_overlay: settings.codex_app_image_overlay_enabled,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticCounts {
    extra_args: usize,
    logs_parsed: usize,
    logs_dropped: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticPaths {
    settings_present: bool,
    logs_present: bool,
    status_present: bool,
}
