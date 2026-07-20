#![cfg(target_os = "windows")]

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use codex_plus_core::context_ownership::ContextOwnershipRevision;
use codex_plus_core::env_conflicts::{EnvConflict, EnvConflictSource};
use codex_plus_core::models::DeleteStatus;
use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_core::settings::{BackendSettings, RelayProtocol};
use codex_plus_core::update::UpdateTarget;
use codex_plus_core::zed_remote::{
    SshTarget, ZedAvailability, ZedOpenStrategy, ZedRemoteProjectSource, ZedRemoteRegistryRevision,
};
use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::{Locale, TextKey, ThemeMode, text};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::context::{ContextFailureKind, ContextViewState};
use codex_plus_manager_native::state::enhancements::{
    EnhancementFailure, EnhancementFailureKind, EnhancementViewState,
};
use codex_plus_manager_native::state::environment::EnvironmentViewState;
use codex_plus_manager_native::state::import::ImportViewState;
use codex_plus_manager_native::state::maintenance::MaintenanceViewState;
use codex_plus_manager_native::state::marketplace::{MarketplaceFailureKind, MarketplaceViewState};
use codex_plus_manager_native::state::provider::{
    ProviderEditorTab, ProviderSaveFailureKind, ProviderViewState,
};
use codex_plus_manager_native::state::sessions::{
    ProviderSyncFailureKind, SessionFilter, SessionViewState,
};
use codex_plus_manager_native::state::settings::{
    SettingsFailure, SettingsFailureKind, SettingsTab, SettingsViewState,
};
use codex_plus_manager_native::state::update::{UpdateFailureKind, UpdateViewState};
use codex_plus_manager_native::state::user_scripts::{
    ScriptsTab, UserScriptFailureKind, UserScriptViewState,
};
use codex_plus_manager_native::state::zed_remote::{ZedRemoteFailureKind, ZedRemoteViewState};
use codex_plus_manager_native::theme;
use codex_plus_manager_native::views::shell::{ShellFeatureStates, ShellViewModel, render_shell};
use codex_plus_manager_service::{
    CcsDiscovery, CcsProviderSummary, ContextBundle, ContextEntryKey, ContextEntryLiveState,
    ContextEntrySummary, ContextKind, ContextOwnershipOutcome, ContextSyncDiffSummary,
    ContextSyncGuard, ContextSyncKeys, ContextSyncOutcome, ContextSyncPreview,
    ContextToolsErrorKind, ContextWorkspace, EnhancementSettingsEnvironment,
    EnhancementSettingsService, InstallStarted, LaunchOutcome, MaintenanceSection,
    ManagerSettingsWorkspace, PluginMarketplaceErrorKind, PluginMarketplaceKind,
    PluginMarketplaceRevision, PluginMarketplaceStatus, PluginMarketplaceWorkspace,
    PrivateArgument, PrivatePath, PrivateUrl, ProviderActivationSummary,
    ProviderCommonConfigExtraction, ProviderDocument, ProviderLiveRevision, ProviderRevision,
    ProviderSyncErrorKind, ProviderSyncRevision, ProviderSyncTargetList, ProviderSyncTargetOption,
    ProviderSyncTargetSource, ProviderSyncWorkspace, ProviderWorkspace, RelayEnvironmentWorkspace,
    SafeSettingsGroup, ScriptIntegrity, ScriptMarketRevision, ScriptMarketSummary,
    ScriptMarketWorkspace, SectionValue, SessionDeleteBatchOutcome, SessionDeleteOutcome,
    SessionRevision, SessionSummary, SessionWorkspace, StepwiseTestOutcome, UpdateAvailability,
    UpdateCheckResult, UpdateDownload, UpdateEnvironment, UpdateEnvironmentError, UpdateProgress,
    UpdateService, UserScriptBackupEvidence, UserScriptErrorKind, UserScriptMutationOutcome,
    UserScriptOrigin, UserScriptRevision, UserScriptStatus, UserScriptSummary, UserScriptWorkspace,
    ZedProjectRevision, ZedRememberOutcome, ZedRemoteErrorKind, ZedRemoteOpenOutcome,
    ZedRemoteProjectSummary, ZedRemoteWorkspace, ZedSettingsRevision,
};
use eframe::egui;
use egui_kittest::{Harness, SnapshotOptions, SnapshotResults, kittest::Queryable};

mod common;

struct SnapshotState {
    model: ShellViewModel,
    provider: Option<ProviderViewState>,
    provider_import: Option<ImportViewState>,
    environment: Option<EnvironmentViewState>,
    context: Option<ContextViewState>,
    marketplace: Option<MarketplaceViewState>,
    sessions: Option<SessionViewState>,
    user_scripts: Option<UserScriptViewState>,
    cjk_font: Option<Vec<u8>>,
}

struct ZedSnapshotState {
    model: ShellViewModel,
    zed_remote: ZedRemoteViewState,
    cjk_font: Option<Vec<u8>>,
}

struct MaintenanceSnapshotState {
    model: ShellViewModel,
    maintenance: MaintenanceViewState,
    cjk_font: Option<Vec<u8>>,
}

struct SettingsSnapshotState {
    model: ShellViewModel,
    settings: SettingsViewState,
    cjk_font: Option<Vec<u8>>,
}

struct ProviderExtractionSnapshotState {
    model: ShellViewModel,
    provider: ProviderViewState,
    cjk_font: Option<Vec<u8>>,
}

struct EnhancementSnapshotState {
    model: ShellViewModel,
    enhancements: EnhancementViewState,
    cjk_font: Option<Vec<u8>>,
}

#[derive(Clone)]
struct SnapshotEnhancementEnvironment(BackendSettings);

impl EnhancementSettingsEnvironment for SnapshotEnhancementEnvironment {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.0.clone())
    }

    fn update_enhancement_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        Ok(predicate(&self.0).then(|| self.0.clone()))
    }
}

#[derive(Debug, Clone, Copy)]
enum ProviderExtractionSnapshotScenario {
    Ready,
    Running,
    Applied,
    NoContent,
    Conflict,
}

#[derive(Debug, Clone, Copy)]
enum EnhancementSnapshotScenario {
    Loading,
    Ready,
    MasterOff,
    Dirty,
    Saving,
    ResetConfirmation,
    Conflict,
    WorkerStopped,
}

#[derive(Debug, Clone, Copy)]
enum ContextSnapshotScenario {
    Loading,
    SafeList,
    Editor,
    Delete,
    Preview,
    Conflict,
    PartialOwnership,
}

#[derive(Debug, Clone, Copy)]
enum MarketplaceSnapshotScenario {
    Healthy,
    Confirmation,
    Running,
    Failure,
}

#[derive(Debug, Clone, Copy)]
enum SessionSnapshotScenario {
    Loading,
    Empty,
    Filtered,
    SelectionConfirmation,
    PartialDeleteFailure,
    ProviderRepairFailure,
}

#[derive(Debug, Clone, Copy)]
enum UserScriptSnapshotScenario {
    Loading,
    MarketList,
    VerifiedConfirmation,
    UnverifiedAcknowledgement,
    IntegrityFailure,
    LocalGlobalOff,
    DeleteConfirmation,
    BackedUpResult,
}

#[derive(Debug, Clone, Copy)]
enum ZedRemoteSnapshotScenario {
    Loading,
    ProjectList,
    LaunchConfirmation,
    SettingsConflict,
    PartialRemember,
}

#[derive(Debug, Clone, Copy)]
enum MaintenanceSnapshotScenario {
    Loading,
    Ready,
    Partial,
    LaunchSuccess,
}

#[derive(Debug, Clone, Copy)]
enum SettingsSnapshotScenario {
    StepwiseDirty,
    ImageResetConfirmation,
    ArgumentsConflict,
    StepwiseTestSuccess,
}

#[derive(Debug, Clone, Copy)]
enum UpdateSnapshotScenario {
    Idle,
    Checking,
    Current,
    Available,
    Confirmation,
    Downloading,
    Launching,
    Error,
}

const CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "overview_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "overview_1180_zh_light",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Dark,
        "overview_1180_en_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "overview_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "overview_960_zh_dark",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "overview_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "overview_960_en_dark",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Light,
        "overview_960_en_light",
    ),
];

const PROVIDER_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "providers_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "providers_1180_zh_light",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Dark,
        "providers_1180_en_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "providers_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "providers_960_zh_dark",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "providers_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "providers_960_en_dark",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Light,
        "providers_960_en_light",
    ),
];

const ENVIRONMENT_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "environment_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "environment_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "environment_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "environment_960_en_dark",
    ),
];

const IMPORT_CASES: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (
        1180.0,
        820.0,
        Locale::ZhCn,
        ThemeMode::Dark,
        "import_1180_zh_dark",
    ),
    (
        1180.0,
        820.0,
        Locale::En,
        ThemeMode::Light,
        "import_1180_en_light",
    ),
    (
        960.0,
        720.0,
        Locale::ZhCn,
        ThemeMode::Light,
        "import_960_zh_light",
    ),
    (
        960.0,
        720.0,
        Locale::En,
        ThemeMode::Dark,
        "import_960_en_dark",
    ),
];

const CONTEXT_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const CONTEXT_SCENARIOS: &[(ContextSnapshotScenario, &str)] = &[
    (ContextSnapshotScenario::Loading, "loading"),
    (ContextSnapshotScenario::SafeList, "list"),
    (ContextSnapshotScenario::Editor, "editor"),
    (ContextSnapshotScenario::Delete, "delete"),
    (ContextSnapshotScenario::Preview, "preview"),
    (ContextSnapshotScenario::Conflict, "conflict"),
    (ContextSnapshotScenario::PartialOwnership, "partial"),
];

const MARKETPLACE_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const MARKETPLACE_SCENARIOS: &[(MarketplaceSnapshotScenario, &str)] = &[
    (MarketplaceSnapshotScenario::Healthy, "healthy"),
    (MarketplaceSnapshotScenario::Confirmation, "confirmation"),
    (MarketplaceSnapshotScenario::Running, "running"),
    (MarketplaceSnapshotScenario::Failure, "failure"),
];

const SESSION_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const SESSION_SCENARIOS: &[(SessionSnapshotScenario, &str)] = &[
    (SessionSnapshotScenario::Loading, "loading"),
    (SessionSnapshotScenario::Empty, "empty"),
    (SessionSnapshotScenario::Filtered, "filtered"),
    (
        SessionSnapshotScenario::SelectionConfirmation,
        "confirmation",
    ),
    (
        SessionSnapshotScenario::PartialDeleteFailure,
        "partial_delete",
    ),
    (
        SessionSnapshotScenario::ProviderRepairFailure,
        "provider_failure",
    ),
];

const USER_SCRIPT_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const USER_SCRIPT_SCENARIOS: &[(UserScriptSnapshotScenario, &str)] = &[
    (UserScriptSnapshotScenario::Loading, "loading"),
    (UserScriptSnapshotScenario::MarketList, "market"),
    (
        UserScriptSnapshotScenario::VerifiedConfirmation,
        "verified_confirmation",
    ),
    (
        UserScriptSnapshotScenario::UnverifiedAcknowledgement,
        "unverified_ack",
    ),
    (
        UserScriptSnapshotScenario::IntegrityFailure,
        "integrity_failure",
    ),
    (
        UserScriptSnapshotScenario::LocalGlobalOff,
        "local_global_off",
    ),
    (
        UserScriptSnapshotScenario::DeleteConfirmation,
        "delete_confirmation",
    ),
    (
        UserScriptSnapshotScenario::BackedUpResult,
        "backed_up_result",
    ),
];

const ZED_REMOTE_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
];

const ZED_REMOTE_SCENARIOS: &[(ZedRemoteSnapshotScenario, &str)] = &[
    (ZedRemoteSnapshotScenario::Loading, "loading"),
    (ZedRemoteSnapshotScenario::ProjectList, "projects"),
    (
        ZedRemoteSnapshotScenario::LaunchConfirmation,
        "launch_confirmation",
    ),
    (
        ZedRemoteSnapshotScenario::SettingsConflict,
        "settings_conflict",
    ),
    (
        ZedRemoteSnapshotScenario::PartialRemember,
        "partial_remember",
    ),
];

const OPERATIONAL_VIEWPORTS: &[(f32, f32, Locale, ThemeMode, &str)] = &[
    (960.0, 720.0, Locale::ZhCn, ThemeMode::Light, "960_zh_light"),
    (960.0, 720.0, Locale::En, ThemeMode::Dark, "960_en_dark"),
    (1180.0, 820.0, Locale::ZhCn, ThemeMode::Dark, "1180_zh_dark"),
    (1180.0, 820.0, Locale::En, ThemeMode::Light, "1180_en_light"),
];

const PROVIDER_EXTRACTION_SCENARIOS: &[(ProviderExtractionSnapshotScenario, &str)] = &[
    (ProviderExtractionSnapshotScenario::Ready, "ready"),
    (ProviderExtractionSnapshotScenario::Running, "running"),
    (ProviderExtractionSnapshotScenario::Applied, "applied"),
    (ProviderExtractionSnapshotScenario::NoContent, "no_content"),
    (ProviderExtractionSnapshotScenario::Conflict, "conflict"),
];

const ENHANCEMENT_SCENARIOS: &[(EnhancementSnapshotScenario, &str)] = &[
    (EnhancementSnapshotScenario::Loading, "loading"),
    (EnhancementSnapshotScenario::Ready, "ready"),
    (EnhancementSnapshotScenario::MasterOff, "master_off"),
    (EnhancementSnapshotScenario::Dirty, "dirty"),
    (EnhancementSnapshotScenario::Saving, "saving"),
    (
        EnhancementSnapshotScenario::ResetConfirmation,
        "reset_confirmation",
    ),
    (EnhancementSnapshotScenario::Conflict, "conflict"),
    (EnhancementSnapshotScenario::WorkerStopped, "worker_stopped"),
];

const MAINTENANCE_SCENARIOS: &[(MaintenanceSnapshotScenario, &str)] = &[
    (MaintenanceSnapshotScenario::Loading, "loading"),
    (MaintenanceSnapshotScenario::Ready, "ready"),
    (MaintenanceSnapshotScenario::Partial, "partial"),
    (MaintenanceSnapshotScenario::LaunchSuccess, "launch_success"),
];

const SETTINGS_SCENARIOS: &[(SettingsSnapshotScenario, &str)] = &[
    (SettingsSnapshotScenario::StepwiseDirty, "stepwise_dirty"),
    (
        SettingsSnapshotScenario::ImageResetConfirmation,
        "image_reset_confirmation",
    ),
    (
        SettingsSnapshotScenario::ArgumentsConflict,
        "arguments_conflict",
    ),
    (
        SettingsSnapshotScenario::StepwiseTestSuccess,
        "stepwise_test_success",
    ),
];

const UPDATE_SCENARIOS: &[(UpdateSnapshotScenario, &str)] = &[
    (UpdateSnapshotScenario::Idle, "idle"),
    (UpdateSnapshotScenario::Checking, "checking"),
    (UpdateSnapshotScenario::Current, "current"),
    (UpdateSnapshotScenario::Available, "available"),
    (UpdateSnapshotScenario::Confirmation, "confirmation"),
    (UpdateSnapshotScenario::Downloading, "downloading"),
    (UpdateSnapshotScenario::Launching, "launching"),
    (UpdateSnapshotScenario::Error, "error"),
];

#[test]
fn overview_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(CASES, Route::Overview, false);
}

#[test]
fn provider_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(PROVIDER_CASES, Route::Providers, false);
}

#[test]
fn provider_common_config_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_provider_extraction_snapshot_matrix();
}

#[test]
fn enhancements_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_enhancement_snapshot_matrix();
}

#[test]
fn environment_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(ENVIRONMENT_CASES, Route::Environment, false);
}

#[test]
fn provider_import_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_snapshot_matrix(IMPORT_CASES, Route::Providers, true);
}

#[test]
fn context_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_context_snapshot_matrix();
}

#[test]
fn marketplace_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_marketplace_snapshot_matrix();
}

#[test]
fn sessions_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_session_snapshot_matrix();
}

#[test]
fn scripts_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_user_script_snapshot_matrix();
}

#[test]
fn zed_remote_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_zed_remote_snapshot_matrix();
}

#[test]
fn maintenance_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_maintenance_snapshot_matrix();
}

#[test]
fn settings_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_settings_snapshot_matrix();
}

#[test]
fn update_wgpu_snapshot_matrix() {
    if std::env::var_os("CODEX_PLUS_UI_SNAPSHOTS").as_deref() != Some("1".as_ref()) {
        return;
    }
    let _guard = snapshot_test_guard();

    run_update_snapshot_matrix();
}

fn snapshot_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

macro_rules! assert_nonblank_render {
    ($image:expr, $name:expr) => {{
        let rendered = &$image;
        let distinct = rendered
            .pixels()
            .map(|pixel| pixel.0)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            distinct.len() > 8,
            "{} rendered with only {} colors",
            $name,
            distinct.len()
        );

        let background = rendered.get_pixel(0, 0).0;
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for (x, y, pixel) in rendered.enumerate_pixels() {
            if pixel.0 == background {
                continue;
            }
            bounds = Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }
                None => (x, y, x, y),
            });
        }
        let (min_x, min_y, max_x, max_y) =
            bounds.unwrap_or_else(|| panic!("{} has no non-background content", $name));
        assert!(
            max_x.saturating_sub(min_x) > 0 && max_y.saturating_sub(min_y) > 0,
            "{} has zero-area content bounds: ({min_x}, {min_y})-({max_x}, {max_y})",
            $name
        );
    }};
}

fn run_snapshot_matrix(
    cases: &[(f32, f32, Locale, ThemeMode, &str)],
    route: Route,
    import_modal: bool,
) {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(width, height, locale, mode, name) in cases {
        let options = SnapshotOptions::new().output_path(&snapshots);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, height))
            .with_theme(match mode {
                ThemeMode::Dark => egui::Theme::Dark,
                ThemeMode::Light => egui::Theme::Light,
            })
            .with_os(egui::os::OperatingSystem::Windows)
            .with_options(options)
            .wgpu()
            .build_ui_state(
                |ui, state: &mut SnapshotState| {
                    if let Some(bytes) = state.cjk_font.take() {
                        egui_extras::install_image_loaders(ui.ctx());
                        fonts::install_cjk_font(ui.ctx(), bytes);
                        theme::apply(ui.ctx(), state.model.theme);
                    }
                    let _ = render_shell(
                        ui,
                        &state.model,
                        ShellFeatureStates {
                            provider: state.provider.as_ref(),
                            provider_import: state.provider_import.as_ref(),
                            environment: state.environment.as_ref(),
                            context: state.context.as_ref(),
                            marketplace: state.marketplace.as_ref(),
                            sessions: state.sessions.as_ref(),
                            user_scripts: state.user_scripts.as_ref(),
                            enhancements: None,
                            zed_remote: None,
                            maintenance: None,
                            settings: None,
                        },
                    );
                },
                SnapshotState {
                    model: {
                        let mut model = common::model(locale, mode);
                        model.route = route;
                        model
                    },
                    provider: (route == Route::Providers).then(common::provider_state),
                    provider_import: import_modal.then(import_state),
                    environment: (route == Route::Environment).then(environment_state),
                    context: None,
                    marketplace: None,
                    sessions: None,
                    user_scripts: None,
                    cjk_font: Some(font.clone()),
                },
            );

        harness.remove_cursor();
        harness.run();
        harness.snapshot(name);
        results.extend_harness(&mut harness);
    }

    results.unwrap();
}

fn run_provider_extraction_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in PROVIDER_EXTRACTION_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in OPERATIONAL_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut ProviderExtractionSnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                provider: Some(&state.provider),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    ProviderExtractionSnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Providers;
                            model
                        },
                        provider: provider_extraction_snapshot_state(scenario),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, ProviderExtractionSnapshotScenario::Running) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_provider_extraction_snapshot_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("provider extraction snapshot should render");
            let snapshot_name = format!("provider_extract_{scenario_name}_{viewport_name}");
            assert_nonblank_render!(image, snapshot_name.as_str());
            harness.snapshot(snapshot_name);
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn provider_extraction_snapshot_state(
    scenario: ProviderExtractionSnapshotScenario,
) -> ProviderViewState {
    let mut state = common::provider_state();
    state.editor_tab = ProviderEditorTab::Config;
    match scenario {
        ProviderExtractionSnapshotScenario::Ready => {}
        ProviderExtractionSnapshotScenario::Running => {
            let _ = state.begin_common_config_extraction().unwrap();
        }
        ProviderExtractionSnapshotScenario::Applied => {
            let (request_id, request) = state.begin_common_config_extraction().unwrap();
            let mut saved = (**state.baseline.as_ref().unwrap()).clone();
            saved.document = request.document;
            saved.document.common_config_contents =
                "approval_policy = \"on-request\"\n".to_string();
            assert!(state.apply_common_config_extraction_response(
                request_id,
                Ok(ProviderCommonConfigExtraction::Applied(Box::new(saved))),
            ));
        }
        ProviderExtractionSnapshotScenario::NoContent => {
            let (request_id, _) = state.begin_common_config_extraction().unwrap();
            assert!(state.apply_common_config_extraction_response(
                request_id,
                Ok(ProviderCommonConfigExtraction::NoContent),
            ));
        }
        ProviderExtractionSnapshotScenario::Conflict => {
            let (request_id, _) = state.begin_common_config_extraction().unwrap();
            assert!(state.apply_common_config_extraction_response(
                request_id,
                Err(ProviderSaveFailureKind::Conflict),
            ));
        }
    }
    state
}

fn assert_provider_extraction_snapshot_layout(
    harness: &Harness<'_, ProviderExtractionSnapshotState>,
    scenario: ProviderExtractionSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let action = match locale {
        Locale::ZhCn => "提取公共配置",
        Locale::En => "Extract common config",
    };
    assert_inside(harness.get_by_label(action).rect(), width, height, action);

    let signal = match (locale, scenario) {
        (_, ProviderExtractionSnapshotScenario::Ready) => action,
        (Locale::ZhCn, ProviderExtractionSnapshotScenario::Running) => "正在提取公共配置",
        (Locale::En, ProviderExtractionSnapshotScenario::Running) => "Extracting common config",
        (Locale::ZhCn, ProviderExtractionSnapshotScenario::Applied) => "公共配置已提取",
        (Locale::En, ProviderExtractionSnapshotScenario::Applied) => "Common config extracted",
        (Locale::ZhCn, ProviderExtractionSnapshotScenario::NoContent) => "没有可提取的公共配置",
        (Locale::En, ProviderExtractionSnapshotScenario::NoContent) => "No common config found",
        (Locale::ZhCn, ProviderExtractionSnapshotScenario::Conflict) => {
            "供应商配置已在磁盘上更改。请重新加载后再次提取。"
        }
        (Locale::En, ProviderExtractionSnapshotScenario::Conflict) => {
            "Provider workspace changed on disk. Reload before extracting again."
        }
    };
    assert_inside(harness.get_by_label(signal).rect(), width, height, signal);
    let tree = format!("{:#?}", harness.root());
    assert!(!tree.contains("sentinel"), "{tree}");
}

fn run_enhancement_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in ENHANCEMENT_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in OPERATIONAL_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut EnhancementSnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                enhancements: Some(&state.enhancements),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    EnhancementSnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Enhancements;
                            model
                        },
                        enhancements: enhancement_snapshot_state(scenario),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, EnhancementSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_enhancement_snapshot_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("enhancement snapshot should render");
            let snapshot_name = format!("enhancements_{scenario_name}_{viewport_name}");
            assert_nonblank_render!(image, snapshot_name.as_str());
            harness.snapshot(snapshot_name);
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn enhancement_snapshot_state(scenario: EnhancementSnapshotScenario) -> EnhancementViewState {
    if matches!(scenario, EnhancementSnapshotScenario::Loading) {
        let mut state = EnhancementViewState::default();
        let _ = state.begin_load();
        return state;
    }

    let settings = BackendSettings {
        enhancements_enabled: !matches!(scenario, EnhancementSnapshotScenario::MasterOff),
        computer_use_guard_enabled: true,
        codex_app_plugin_marketplace_unlock: true,
        codex_app_session_delete: true,
        codex_app_fast_startup: true,
        codex_app_zed_remote_open: true,
        ..BackendSettings::default()
    };
    let workspace = Arc::new(
        EnhancementSettingsService::new(SnapshotEnhancementEnvironment(settings))
            .load()
            .unwrap(),
    );
    let mut state = EnhancementViewState::from_workspace(Arc::clone(&workspace));
    match scenario {
        EnhancementSnapshotScenario::Loading
        | EnhancementSnapshotScenario::Ready
        | EnhancementSnapshotScenario::MasterOff => {}
        EnhancementSnapshotScenario::Dirty => {
            let mut draft = *state.draft();
            draft.markdown_export = !draft.markdown_export;
            state.edit(draft);
        }
        EnhancementSnapshotScenario::Saving => {
            let mut draft = *state.draft();
            draft.markdown_export = !draft.markdown_export;
            state.edit(draft);
            let _ = state.begin_save().unwrap();
        }
        EnhancementSnapshotScenario::ResetConfirmation => {
            assert!(state.request_reset());
        }
        EnhancementSnapshotScenario::Conflict => {
            let mut draft = *state.draft();
            draft.markdown_export = !draft.markdown_export;
            state.edit(draft);
            let (request_id, _) = state.begin_save().unwrap();
            assert!(state.apply_save_response(
                request_id,
                Err(EnhancementFailure::with_workspace(
                    EnhancementFailureKind::SettingsConflict,
                    Arc::clone(&workspace),
                )),
            ));
        }
        EnhancementSnapshotScenario::WorkerStopped => {
            let mut draft = *state.draft();
            draft.markdown_export = !draft.markdown_export;
            state.edit(draft);
            let _ = state.begin_save().unwrap();
            state.fail_running_operations();
        }
    }
    state
}

fn assert_enhancement_snapshot_layout(
    harness: &Harness<'_, EnhancementSnapshotState>,
    scenario: EnhancementSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let signal = match (locale, scenario) {
        (Locale::ZhCn, EnhancementSnapshotScenario::Loading) => "正在加载增强设置",
        (Locale::En, EnhancementSnapshotScenario::Loading) => "Loading enhancement settings",
        (Locale::ZhCn, EnhancementSnapshotScenario::Ready)
        | (Locale::ZhCn, EnhancementSnapshotScenario::MasterOff) => "启用增强功能",
        (Locale::En, EnhancementSnapshotScenario::Ready)
        | (Locale::En, EnhancementSnapshotScenario::MasterOff) => "Enable enhancements",
        (Locale::ZhCn, EnhancementSnapshotScenario::Dirty)
        | (Locale::ZhCn, EnhancementSnapshotScenario::Saving) => "增强设置有未保存更改",
        (Locale::En, EnhancementSnapshotScenario::Dirty)
        | (Locale::En, EnhancementSnapshotScenario::Saving) => {
            "Enhancement settings have unsaved changes"
        }
        (Locale::ZhCn, EnhancementSnapshotScenario::ResetConfirmation) => "重置增强设置？",
        (Locale::En, EnhancementSnapshotScenario::ResetConfirmation) => {
            "Reset enhancement settings?"
        }
        (Locale::ZhCn, EnhancementSnapshotScenario::Conflict) => "增强设置已在磁盘上更改",
        (Locale::En, EnhancementSnapshotScenario::Conflict) => {
            "Enhancement settings changed on disk"
        }
        (Locale::ZhCn, EnhancementSnapshotScenario::WorkerStopped) => "增强设置后台服务已停止",
        (Locale::En, EnhancementSnapshotScenario::WorkerStopped) => {
            "The enhancement settings worker has stopped"
        }
    };
    assert_inside(harness.get_by_label(signal).rect(), width, height, signal);
    if !matches!(scenario, EnhancementSnapshotScenario::Loading) {
        let save = match locale {
            Locale::ZhCn => "保存增强设置",
            Locale::En => "Save enhancements",
        };
        assert_inside(harness.get_by_label(save).rect(), width, height, save);
    }
    if matches!(scenario, EnhancementSnapshotScenario::MasterOff) {
        assert!(
            harness
                .query_by(|node| {
                    node.label().as_deref() == Some("Computer Use Guard") && node.is_disabled()
                })
                .is_some()
        );
    }
    let tree = format!("{:#?}", harness.root());
    assert!(!tree.contains("sentinel"), "{tree}");
}

fn run_context_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in CONTEXT_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in CONTEXT_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                context: state.context.as_ref(),
                                marketplace: state.marketplace.as_ref(),
                                sessions: state.sessions.as_ref(),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Context;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: Some(context_snapshot_state(scenario)),
                        marketplace: Some(marketplace_snapshot_state(
                            MarketplaceSnapshotScenario::Healthy,
                        )),
                        sessions: None,
                        user_scripts: None,
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, ContextSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_context_layout(&harness, scenario, locale, width, height);
            let image = harness.render().expect("context snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "context snapshot rendered blank");
            harness.snapshot(format!("context_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_marketplace_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in MARKETPLACE_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in MARKETPLACE_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                context: state.context.as_ref(),
                                marketplace: state.marketplace.as_ref(),
                                sessions: state.sessions.as_ref(),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Context;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: Some(context_snapshot_state(ContextSnapshotScenario::SafeList)),
                        marketplace: Some(marketplace_snapshot_state(scenario)),
                        sessions: None,
                        user_scripts: None,
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, MarketplaceSnapshotScenario::Running) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_marketplace_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("marketplace snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "marketplace snapshot rendered blank");
            harness.snapshot(format!("marketplace_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_session_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in SESSION_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in SESSION_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                sessions: state.sessions.as_ref(),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Sessions;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: None,
                        marketplace: None,
                        sessions: Some(session_snapshot_state(scenario)),
                        user_scripts: None,
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, SessionSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_session_layout(&harness, scenario, locale, width, height);
            let image = harness.render().expect("session snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "session snapshot rendered blank");
            harness.snapshot(format!("sessions_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_user_script_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in USER_SCRIPT_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in USER_SCRIPT_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                user_scripts: state.user_scripts.as_ref(),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Scripts;
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: None,
                        marketplace: None,
                        sessions: None,
                        user_scripts: Some(user_script_snapshot_state(scenario)),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, UserScriptSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_user_script_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("user script snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "user script snapshot rendered blank");
            harness.snapshot(format!("scripts_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_zed_remote_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in ZED_REMOTE_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in ZED_REMOTE_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut ZedSnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                zed_remote: Some(&state.zed_remote),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    ZedSnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::ZedRemote;
                            model
                        },
                        zed_remote: zed_remote_snapshot_state(scenario),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, ZedRemoteSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_zed_remote_layout(&harness, scenario, locale, width, height);
            let image = harness.render().expect("Zed remote snapshot should render");
            let distinct = image
                .pixels()
                .map(|pixel| pixel.0)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(distinct.len() > 8, "Zed remote snapshot rendered blank");
            harness.snapshot(format!("zed_remote_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_maintenance_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in MAINTENANCE_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in OPERATIONAL_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut MaintenanceSnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                maintenance: Some(&state.maintenance),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    MaintenanceSnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Maintenance;
                            model
                        },
                        maintenance: maintenance_snapshot_state(scenario),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(scenario, MaintenanceSnapshotScenario::Loading) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_maintenance_snapshot_layout(&harness, scenario, locale, width, height);
            let image = harness
                .render()
                .expect("maintenance snapshot should render");
            let snapshot_name = format!("maintenance_{scenario_name}_{viewport_name}");
            assert_nonblank_render!(image, snapshot_name.as_str());
            harness.snapshot(snapshot_name);
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_settings_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in SETTINGS_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in OPERATIONAL_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SettingsSnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(
                            ui,
                            &state.model,
                            ShellFeatureStates {
                                settings: Some(&state.settings),
                                ..ShellFeatureStates::default()
                            },
                        );
                    },
                    SettingsSnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::Settings;
                            model
                        },
                        settings: settings_snapshot_state(scenario),
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            harness.run();
            assert_settings_snapshot_layout(&harness, scenario, locale, width, height);
            let image = harness.render().expect("settings snapshot should render");
            let snapshot_name = format!("settings_{scenario_name}_{viewport_name}");
            assert_nonblank_render!(image, snapshot_name.as_str());
            harness.snapshot(snapshot_name);
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn run_update_snapshot_matrix() {
    let font = fonts::load_cjk_font().expect("Windows CJK font is required for UI snapshots");
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let mut results = SnapshotResults::new();

    for &(scenario, scenario_name) in UPDATE_SCENARIOS {
        for &(width, height, locale, mode, viewport_name) in OPERATIONAL_VIEWPORTS {
            let options = SnapshotOptions::new().output_path(&snapshots);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .with_theme(match mode {
                    ThemeMode::Dark => egui::Theme::Dark,
                    ThemeMode::Light => egui::Theme::Light,
                })
                .with_os(egui::os::OperatingSystem::Windows)
                .with_options(options)
                .wgpu()
                .build_ui_state(
                    |ui, state: &mut SnapshotState| {
                        if let Some(bytes) = state.cjk_font.take() {
                            egui_extras::install_image_loaders(ui.ctx());
                            fonts::install_cjk_font(ui.ctx(), bytes);
                            theme::apply(ui.ctx(), state.model.theme);
                        }
                        let _ = render_shell(ui, &state.model, ShellFeatureStates::default());
                    },
                    SnapshotState {
                        model: {
                            let mut model = common::model(locale, mode);
                            model.route = Route::About;
                            model.update = update_snapshot_state(scenario);
                            model
                        },
                        provider: None,
                        provider_import: None,
                        environment: None,
                        context: None,
                        marketplace: None,
                        sessions: None,
                        user_scripts: None,
                        cjk_font: Some(font.clone()),
                    },
                );

            harness.remove_cursor();
            if matches!(
                scenario,
                UpdateSnapshotScenario::Checking | UpdateSnapshotScenario::Launching
            ) {
                harness.run_steps(2);
            } else {
                harness.run();
            }
            assert_update_snapshot_layout(&harness, scenario, locale, width, height);
            let tree = format!("{:#?}", harness.root());
            assert!(!tree.contains("updates.invalid"), "{tree}");
            assert!(!tree.contains("private-update-path"), "{tree}");
            let image = harness.render().expect("update snapshot should render");
            assert_nonblank_render!(image, scenario_name);
            harness.snapshot(format!("update_{scenario_name}_{viewport_name}"));
            results.extend_harness(&mut harness);
        }
    }

    results.unwrap();
}

fn assert_update_snapshot_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: UpdateSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = match locale {
        Locale::ZhCn => "关于 Codex++",
        Locale::En => "About Codex++",
    };
    assert_inside(harness.get_by_label(header).rect(), width, height, header);
    let repository = match locale {
        Locale::ZhCn => "项目仓库",
        Locale::En => "Project repository",
    };
    assert_inside(
        harness.get_by_label(repository).rect(),
        width,
        height,
        repository,
    );
    let label = match (locale, scenario) {
        (Locale::ZhCn, UpdateSnapshotScenario::Idle) => "尚未检查更新",
        (Locale::En, UpdateSnapshotScenario::Idle) => "Updates have not been checked",
        (Locale::ZhCn, UpdateSnapshotScenario::Checking) => "正在检查更新...",
        (Locale::En, UpdateSnapshotScenario::Checking) => "Checking for updates...",
        (Locale::ZhCn, UpdateSnapshotScenario::Current) => "Codex++ 已是最新版本",
        (Locale::En, UpdateSnapshotScenario::Current) => "Codex++ is up to date",
        (_, UpdateSnapshotScenario::Available | UpdateSnapshotScenario::Confirmation) => {
            match locale {
                Locale::ZhCn => "版本 99.0.0 可以安装",
                Locale::En => "Version 99.0.0 is available",
            }
        }
        (_, UpdateSnapshotScenario::Downloading) => "40 / 100 bytes",
        (Locale::ZhCn, UpdateSnapshotScenario::Launching) => "安装器已打开，正在退出 Codex++...",
        (Locale::En, UpdateSnapshotScenario::Launching) => "Installer opened. Exiting Codex++...",
        (Locale::ZhCn, UpdateSnapshotScenario::Error) => "更新操作失败，请重试",
        (Locale::En, UpdateSnapshotScenario::Error) => "The update operation failed. Try again.",
    };
    assert_inside(harness.get_by_label(label).rect(), width, height, label);
    if matches!(scenario, UpdateSnapshotScenario::Confirmation) {
        let title = match locale {
            Locale::ZhCn => "确认更新",
            Locale::En => "Confirm update",
        };
        assert_inside(harness.get_by_label(title).rect(), width, height, title);
    }
}

fn update_snapshot_state(scenario: UpdateSnapshotScenario) -> UpdateViewState {
    let mut state = UpdateViewState::default();
    match scenario {
        UpdateSnapshotScenario::Idle => {}
        UpdateSnapshotScenario::Checking => {
            let first = state.begin_check(false).unwrap();
            let _ = state.apply_check_response(first, Ok(available_update_result()));
            let _ = state.begin_check(true);
        }
        UpdateSnapshotScenario::Current => {
            let request = state.begin_check(false).unwrap();
            let _ = state.apply_check_response(
                request,
                Ok(Arc::new(UpdateCheckResult {
                    installed_version: env!("CARGO_PKG_VERSION").to_owned(),
                    latest_version: env!("CARGO_PKG_VERSION").to_owned(),
                    summary: String::new(),
                    availability: UpdateAvailability::Current,
                })),
            );
        }
        UpdateSnapshotScenario::Available => {
            install_available_update(&mut state);
        }
        UpdateSnapshotScenario::Confirmation => {
            install_available_update(&mut state);
            let _ = state.request_install_confirmation();
        }
        UpdateSnapshotScenario::Downloading => {
            install_available_update(&mut state);
            let _ = state.request_install_confirmation();
            let (request, _) = state.confirm_install().unwrap();
            let _ = state.apply_progress(
                request,
                UpdateProgress {
                    downloaded_bytes: 40,
                    total_bytes: Some(100),
                },
            );
        }
        UpdateSnapshotScenario::Launching => {
            install_available_update(&mut state);
            let _ = state.request_install_confirmation();
            let (request, _) = state.confirm_install().unwrap();
            let _ = state.apply_install_response(
                request,
                Ok(InstallStarted {
                    version: "99.0.0".to_owned(),
                }),
            );
        }
        UpdateSnapshotScenario::Error => {
            let first = state.begin_check(false).unwrap();
            let _ = state.apply_check_response(
                first,
                Ok(Arc::new(UpdateCheckResult {
                    installed_version: env!("CARGO_PKG_VERSION").to_owned(),
                    latest_version: env!("CARGO_PKG_VERSION").to_owned(),
                    summary: String::new(),
                    availability: UpdateAvailability::Current,
                })),
            );
            let retry = state.begin_check(false).unwrap();
            let _ = state.apply_check_response(retry, Err(UpdateFailureKind::MetadataFetchFailed));
        }
    }
    state
}

fn install_available_update(state: &mut UpdateViewState) {
    let request = state.begin_check(false).unwrap();
    let _ = state.apply_check_response(request, Ok(available_update_result()));
}

fn available_update_result() -> Arc<UpdateCheckResult> {
    Arc::new(
        UpdateService::new(SnapshotUpdateEnvironment)
            .check()
            .unwrap(),
    )
}

struct SnapshotUpdateEnvironment;

impl UpdateEnvironment for SnapshotUpdateEnvironment {
    type Artifact = Vec<u8>;

    fn current_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    fn target(&self) -> UpdateTarget {
        UpdateTarget::WindowsX64
    }

    fn fetch_release_metadata(
        &self,
        _maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        Ok(serde_json::to_vec(&serde_json::json!({
            "version": "99.0.0",
            "body": "Safe bounded release summary for the Native update workflow.",
            "assets": [{
                "name": "CodexPlusPlus-99.0.0-windows-x64-setup.exe",
                "url": "https://updates.invalid/CodexPlusPlus-99.0.0-windows-x64-setup.exe"
            }]
        }))
        .unwrap())
    }

    fn open_asset_download(&self, _url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        panic!("snapshot never downloads")
    }

    fn create_update_artifact(
        &self,
        _safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        panic!("snapshot never creates artifacts")
    }

    fn publish_update_artifact(
        &self,
        _artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("snapshot never publishes artifacts")
    }

    fn cleanup_update_artifact(&self, _artifact: &mut Self::Artifact) {}

    fn launch_update_artifact(
        &self,
        _artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("snapshot never launches artifacts")
    }
}

fn assert_maintenance_snapshot_layout(
    harness: &Harness<'_, MaintenanceSnapshotState>,
    scenario: MaintenanceSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::Maintenance)
    );
    assert_inside(harness.get_by_label(&header).rect(), width, height, &header);

    let application = harness
        .get_by_label(text(locale, TextKey::CodexApplication))
        .rect();
    let diagnostics = harness
        .get_by_label(text(locale, TextKey::Diagnostics))
        .rect();
    assert_inside(
        application,
        width,
        height,
        text(locale, TextKey::CodexApplication),
    );
    assert_inside(
        diagnostics,
        width,
        height,
        text(locale, TextKey::Diagnostics),
    );

    match scenario {
        MaintenanceSnapshotScenario::Loading => {
            let loading = format!(
                "{}: {}",
                text(locale, TextKey::Status),
                text(locale, TextKey::Loading)
            );
            assert_inside(
                harness.get_by_label(&loading).rect(),
                width,
                height,
                &loading,
            );
        }
        MaintenanceSnapshotScenario::Ready => {
            let path = "C:/fixture/Codex";
            let path_editor = harness.get_by(|node| {
                node.role() == egui::accesskit::Role::TextInput
                    && node.value().as_deref() == Some(path)
            });
            assert_inside(path_editor.rect(), width, height, path);
        }
        MaintenanceSnapshotScenario::Partial => {
            let unavailable = text(locale, TextKey::SafeDocumentUnavailable);
            assert_inside(
                harness.get_by_label(unavailable).rect(),
                width,
                height,
                unavailable,
            );
        }
        MaintenanceSnapshotScenario::LaunchSuccess => {
            let accepted = text(locale, TextKey::LaunchAccepted);
            assert_inside(
                harness.get_by_label(accepted).rect(),
                width,
                height,
                accepted,
            );
        }
    }

    if width <= 960.0 {
        let path_label = harness
            .get_by_label(text(locale, TextKey::ApplicationPath))
            .rect();
        assert!(
            diagnostics.min.y > path_label.max.y + 120.0,
            "compact maintenance must stack: {path_label:?} {diagnostics:?}"
        );
    } else {
        assert!(
            diagnostics.min.x > application.min.x + 300.0,
            "wide maintenance must use columns: {application:?} {diagnostics:?}"
        );
        assert!(
            (diagnostics.min.y - application.min.y).abs() < 8.0,
            "wide maintenance columns must align: {application:?} {diagnostics:?}"
        );
    }

    let tree = format!("{:#?}", harness.root());
    assert!(!tree.contains("private-"), "{tree}");
    assert!(!tree.contains("sentinel"), "{tree}");
}

fn assert_settings_snapshot_layout(
    harness: &Harness<'_, SettingsSnapshotState>,
    scenario: SettingsSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::Settings)
    );
    assert_inside(harness.get_by_label(&header).rect(), width, height, &header);

    let (tab, signal) = match (locale, scenario) {
        (Locale::ZhCn, SettingsSnapshotScenario::StepwiseDirty) => {
            ("Stepwise", "当前分组有未保存更改".to_owned())
        }
        (Locale::En, SettingsSnapshotScenario::StepwiseDirty) => {
            ("Stepwise", "This group has unsaved changes".to_owned())
        }
        (Locale::ZhCn, SettingsSnapshotScenario::ImageResetConfirmation) => {
            ("图片覆盖", "重置图片覆盖设置？".to_owned())
        }
        (Locale::En, SettingsSnapshotScenario::ImageResetConfirmation) => {
            ("Image overlay", "Reset image overlay settings?".to_owned())
        }
        (Locale::ZhCn, SettingsSnapshotScenario::ArgumentsConflict) => {
            ("启动参数", "设置已在其他位置更改".to_owned())
        }
        (Locale::En, SettingsSnapshotScenario::ArgumentsConflict) => {
            ("Launch arguments", "Settings changed elsewhere".to_owned())
        }
        (Locale::ZhCn, SettingsSnapshotScenario::StepwiseTestSuccess) => {
            ("Stepwise", "连接测试通过，条目数: 4".to_owned())
        }
        (Locale::En, SettingsSnapshotScenario::StepwiseTestSuccess) => {
            ("Stepwise", "Connection succeeded, items: 4".to_owned())
        }
    };
    assert_inside(harness.get_by_label(tab).rect(), width, height, tab);
    assert_inside(harness.get_by_label(&signal).rect(), width, height, &signal);

    let tree = format!("{:#?}", harness.root());
    assert!(!tree.contains("private-"), "{tree}");
    assert!(!tree.contains("sentinel"), "{tree}");
}

fn assert_context_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: ContextSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::ToolsPlugins)
    );
    assert_inside(harness.get_by_label(&header).rect(), width, height, &header);
    let marketplace_title = match locale {
        Locale::ZhCn => "插件市场",
        Locale::En => "Plugin marketplaces",
    };
    assert_inside(
        harness.get_by_label(marketplace_title).rect(),
        width,
        height,
        marketplace_title,
    );
    let label = match scenario {
        ContextSnapshotScenario::Loading => text(locale, TextKey::ToolsPluginsSubtitle),
        ContextSnapshotScenario::SafeList => {
            "beta-with-a-very-long-context-entry-id-that-must-truncate-safely"
        }
        ContextSnapshotScenario::Editor => match locale {
            Locale::ZhCn => "新建技能条目",
            Locale::En => "Create Skill entry",
        },
        ContextSnapshotScenario::Delete => text(locale, TextKey::DeleteContextEntry),
        ContextSnapshotScenario::Preview => text(locale, TextKey::PreviewLiveSync),
        ContextSnapshotScenario::Conflict => text(locale, TextKey::ContextProviderConflict),
        ContextSnapshotScenario::PartialOwnership => {
            "C:/isolated/context/backups/context-backup-with-a-very-long-file-name.toml"
        }
    };
    assert_inside(harness.get_by_label(label).rect(), width, height, label);
}

fn assert_marketplace_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: MarketplaceSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let labels = match locale {
        Locale::ZhCn => ["插件市场", "OpenAI 插件", "官方远端缓存"],
        Locale::En => [
            "Plugin marketplaces",
            "OpenAI plugins",
            "Official remote cache",
        ],
    };
    for label in labels {
        assert_inside(harness.get_by_label(label).rect(), width, height, label);
    }
    let scenario_label = match (locale, scenario) {
        (_, MarketplaceSnapshotScenario::Healthy) => return,
        (Locale::ZhCn, MarketplaceSnapshotScenario::Confirmation) => "修复 OpenAI 插件？",
        (Locale::En, MarketplaceSnapshotScenario::Confirmation) => "Repair OpenAI plugins?",
        (Locale::ZhCn, MarketplaceSnapshotScenario::Running) => "正在修复",
        (Locale::En, MarketplaceSnapshotScenario::Running) => "Repairing",
        (Locale::ZhCn, MarketplaceSnapshotScenario::Failure) => "写入失败",
        (Locale::En, MarketplaceSnapshotScenario::Failure) => "Write failed",
    };
    assert_inside(
        harness.get_by_label(scenario_label).rect(),
        width,
        height,
        scenario_label,
    );
}

fn assert_session_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: SessionSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::Sessions)
    );
    for label in [
        header.as_str(),
        text(locale, TextKey::RefreshSessions),
        text(locale, TextKey::HistoricalSessionRepair),
    ] {
        assert_inside(harness.get_by_label(label).rect(), width, height, label);
    }
    let scenario_label = match scenario {
        SessionSnapshotScenario::Loading => text(locale, TextKey::SessionsSubtitle),
        SessionSnapshotScenario::Empty => text(locale, TextKey::NoSessions),
        SessionSnapshotScenario::Filtered => "Alpha snapshot session 0",
        SessionSnapshotScenario::SelectionConfirmation => {
            text(locale, TextKey::ConfirmSessionDeletion)
        }
        SessionSnapshotScenario::PartialDeleteFailure => {
            text(locale, TextKey::SessionDeletePartial)
        }
        SessionSnapshotScenario::ProviderRepairFailure => {
            text(locale, TextKey::ProviderRepairFailed)
        }
    };
    assert_inside(
        harness.get_by_label(scenario_label).rect(),
        width,
        height,
        scenario_label,
    );
}

fn assert_user_script_layout(
    harness: &Harness<'_, SnapshotState>,
    scenario: UserScriptSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::Scripts)
    );
    for label in [
        header.as_str(),
        text(locale, TextKey::ScriptMarket),
        text(locale, TextKey::LocalScripts),
    ] {
        assert_inside(harness.get_by_label(label).rect(), width, height, label);
    }

    let scenario_label = match scenario {
        UserScriptSnapshotScenario::Loading => text(locale, TextKey::ScriptsSubtitle),
        UserScriptSnapshotScenario::MarketList => {
            "Long metadata script name that must truncate without moving controls"
        }
        UserScriptSnapshotScenario::VerifiedConfirmation => {
            text(locale, TextKey::UpdateScriptQuestion)
        }
        UserScriptSnapshotScenario::UnverifiedAcknowledgement => {
            text(locale, TextKey::AcknowledgeUnverified)
        }
        UserScriptSnapshotScenario::IntegrityFailure => {
            text(locale, TextKey::ScriptIntegrityMismatch)
        }
        UserScriptSnapshotScenario::LocalGlobalOff => text(locale, TextKey::EnableAllScripts),
        UserScriptSnapshotScenario::DeleteConfirmation => {
            text(locale, TextKey::ConfirmScriptDeletion)
        }
        UserScriptSnapshotScenario::BackedUpResult => text(locale, TextKey::BackupCreated),
    };
    assert_inside(
        harness.get_by_label(scenario_label).rect(),
        width,
        height,
        scenario_label,
    );
}

fn assert_zed_remote_layout(
    harness: &Harness<'_, ZedSnapshotState>,
    scenario: ZedRemoteSnapshotScenario,
    locale: Locale,
    width: f32,
    height: f32,
) {
    let header = format!(
        "{} {}",
        text(locale, TextKey::AppName),
        text(locale, TextKey::ZedRemote)
    );
    assert_inside(harness.get_by_label(&header).rect(), width, height, &header);
    let scenario_label = match scenario {
        ZedRemoteSnapshotScenario::Loading => text(locale, TextKey::Loading),
        ZedRemoteSnapshotScenario::ProjectList => {
            "Long Zed workspace label that must truncate without moving command controls"
        }
        ZedRemoteSnapshotScenario::LaunchConfirmation => text(locale, TextKey::ZedOpenConfirmation),
        ZedRemoteSnapshotScenario::SettingsConflict => text(locale, TextKey::ZedSettingsConflict),
        ZedRemoteSnapshotScenario::PartialRemember => text(locale, TextKey::ZedRememberFailed),
    };
    assert_inside(
        harness.get_by_label(scenario_label).rect(),
        width,
        height,
        scenario_label,
    );
}

fn assert_inside(rect: egui::Rect, width: f32, height: f32, label: &str) {
    assert!(rect.is_positive(), "{label}: {rect:?}");
    assert!(rect.min.x >= 0.0 && rect.min.y >= 0.0, "{label}: {rect:?}");
    assert!(
        rect.max.x <= width && rect.max.y <= height,
        "{label}: {rect:?}"
    );
}

fn maintenance_snapshot_state(scenario: MaintenanceSnapshotScenario) -> MaintenanceViewState {
    let mut state = MaintenanceViewState::default();
    if matches!(scenario, MaintenanceSnapshotScenario::Loading) {
        state.begin_load();
        return state;
    }

    let mut workspace = (*common::maintenance_workspace("C:/fixture/Codex")).clone();
    if matches!(scenario, MaintenanceSnapshotScenario::Partial) {
        workspace.entrypoints = SectionValue::Unavailable(MaintenanceSection::Entrypoints);
        workspace.watcher = SectionValue::Unavailable(MaintenanceSection::Watcher);
        workspace.logs = SectionValue::Unavailable(MaintenanceSection::Logs);
    }
    let request_id = state.begin_load();
    assert!(state.apply_load_response(request_id, Ok(Arc::new(workspace))));

    if matches!(scenario, MaintenanceSnapshotScenario::LaunchSuccess) {
        let (launch_id, _) = state.begin_launch().expect("fixture launch must start");
        assert!(state.apply_launch_response(
            launch_id,
            Ok(LaunchOutcome {
                debug_port: 9229,
                helper_port: 57321,
                accepted: true,
            }),
        ));
    }
    state
}

fn settings_snapshot_state(scenario: SettingsSnapshotScenario) -> SettingsViewState {
    let mut state = SettingsViewState::from_workspace(settings_snapshot_workspace(1));
    match scenario {
        SettingsSnapshotScenario::StepwiseDirty => {
            state.edit_stepwise_url("https://edited.snapshot.example.test/v1".to_owned());
            state.edit_stepwise_model(
                "snapshot-model-with-a-long-but-safe-operational-name".to_owned(),
            );
        }
        SettingsSnapshotScenario::ImageResetConfirmation => {
            state.set_tab(SettingsTab::ImageOverlay);
            state.edit_image_path("C:/fixture/overlay-preview.png".to_owned());
            assert!(state.request_reset(SafeSettingsGroup::ImageOverlay));
        }
        SettingsSnapshotScenario::ArgumentsConflict => {
            state.set_tab(SettingsTab::LaunchArguments);
            state.edit_extra_args(
                "--snapshot-mode\n--option=fixture-value\n--long-safe-argument=abcdefghijklmnopqrstuvwxyz"
                    .to_owned(),
            );
            let (request_id, _) = state
                .begin_extra_args_save()
                .expect("dirty fixture arguments must save");
            assert!(state.apply_extra_args_save_response(
                request_id,
                Err(SettingsFailure::with_workspace(
                    SettingsFailureKind::SettingsConflict,
                    SafeSettingsGroup::ExtraArgs,
                    settings_snapshot_workspace(2),
                )),
            ));
        }
        SettingsSnapshotScenario::StepwiseTestSuccess => {
            let (request_id, _) = state
                .begin_stepwise_test()
                .expect("fixture Stepwise test must start");
            assert!(state.apply_stepwise_test_response(
                request_id,
                Ok(StepwiseTestOutcome { item_count: 4 }),
            ));
        }
    }
    state
}

fn settings_snapshot_workspace(seed: u8) -> Arc<ManagerSettingsWorkspace> {
    let mut workspace = (*common::manager_settings_workspace(seed)).clone();
    workspace.stepwise.settings.base_url =
        PrivateUrl::new(format!("https://snapshot-{seed}.example.test/v1"));
    workspace.stepwise.settings.api_key_env = "CODEX_PLUS_SNAPSHOT_KEY".to_owned();
    workspace.stepwise.settings.model = format!("snapshot-model-{seed}");
    workspace.image_overlay.settings.path =
        PrivatePath::new(format!("C:/fixture/overlay-{seed}.png"));
    workspace.extra_args.settings.args = vec![
        PrivateArgument::new(format!("--snapshot-{seed}")),
        PrivateArgument::new("--safe-mode"),
    ];
    Arc::new(workspace)
}

fn zed_remote_snapshot_state(scenario: ZedRemoteSnapshotScenario) -> ZedRemoteViewState {
    let mut state = ZedRemoteViewState::default();
    if matches!(scenario, ZedRemoteSnapshotScenario::Loading) {
        state.begin_load();
        return state;
    }

    let request_id = state.begin_load();
    state.apply_load_response(request_id, Ok(Arc::new(zed_remote_workspace())));
    match scenario {
        ZedRemoteSnapshotScenario::Loading | ZedRemoteSnapshotScenario::ProjectList => {}
        ZedRemoteSnapshotScenario::LaunchConfirmation => {
            state.request_open("zed-current", ZedOpenStrategy::NewWindow, true);
        }
        ZedRemoteSnapshotScenario::SettingsConflict => {
            state.set_strategy(ZedOpenStrategy::NewWindow);
            let (save_id, _) = state.begin_save_preferences().unwrap();
            state.apply_save_response(
                save_id,
                Err(ZedRemoteFailureKind::Service(
                    ZedRemoteErrorKind::SettingsConflict,
                )),
            );
        }
        ZedRemoteSnapshotScenario::PartialRemember => {
            state.request_open("zed-current", ZedOpenStrategy::ReuseWindow, true);
            let (open_id, _) = state.begin_open().unwrap();
            state.apply_open_response(
                open_id,
                Ok(Arc::new(ZedRemoteOpenOutcome {
                    workspace: zed_remote_workspace(),
                    strategy: ZedOpenStrategy::ReuseWindow,
                    url: "zed://snapshot-redacted".to_owned(),
                    remember: ZedRememberOutcome::Failed(ZedRemoteErrorKind::RegistryWriteFailed),
                })),
            );
        }
    }
    state
}

fn zed_remote_workspace() -> ZedRemoteWorkspace {
    ZedRemoteWorkspace {
        settings_revision: ZedSettingsRevision::from_digest([31; 32]),
        registry_revision: ZedRemoteRegistryRevision::from_digest([32; 32]),
        default_strategy: ZedOpenStrategy::ReuseWindow,
        registry_enabled: true,
        availability: ZedAvailability {
            platform_supported: true,
            cli_found: true,
            app_found: true,
        },
        projects: vec![
            zed_remote_project(
                "zed-current",
                "Long Zed workspace label that must truncate without moving command controls",
                "2001:0db8:85a3:0000:0000:8a2e:0370:7334",
                &format!("/{}", "snapshot-very-long-segment/".repeat(7)),
                ZedRemoteProjectSource::CurrentThread,
            ),
            zed_remote_project(
                "zed-recent",
                "Recent snapshot workspace",
                "recent.snapshot.example.test",
                "/srv/recent-snapshot",
                ZedRemoteProjectSource::Recent,
            ),
            zed_remote_project(
                "zed-discovered",
                "Discovered snapshot workspace",
                "discovered.snapshot.example.test",
                "/srv/discovered-snapshot",
                ZedRemoteProjectSource::SqliteThreadCwd,
            ),
        ],
    }
}

fn zed_remote_project(
    id: &str,
    label: &str,
    host: &str,
    remote_path: &str,
    source: ZedRemoteProjectSource,
) -> ZedRemoteProjectSummary {
    ZedRemoteProjectSummary {
        id: id.to_owned(),
        revision: ZedProjectRevision::from_digest([id.len() as u8; 32]),
        label: label.to_owned(),
        host_id: format!("snapshot-host-{id}"),
        ssh: SshTarget {
            user: "snapshot-user".to_owned(),
            host: host.to_owned(),
            port: Some(2222),
        },
        remote_path: remote_path.to_owned(),
        url: format!("zed://ssh/snapshot-user@{host}:2222{remote_path}"),
        source,
        last_opened_at_ms: Some(1_700_000_000_000),
        is_current: source == ZedRemoteProjectSource::CurrentThread,
    }
}

fn context_snapshot_state(scenario: ContextSnapshotScenario) -> ContextViewState {
    let mut state = ContextViewState::default();
    if matches!(scenario, ContextSnapshotScenario::Loading) {
        state.begin_workspace_refresh();
        return state;
    }

    let request_id = state.begin_workspace_refresh();
    state.apply_workspace_response(request_id, Ok(context_bundle()));
    match scenario {
        ContextSnapshotScenario::Loading | ContextSnapshotScenario::SafeList => {}
        ContextSnapshotScenario::Editor => {
            state.open_create(ContextKind::Skill);
            state.set_editor_id("new-skill".to_owned());
            state.set_editor_body("token = \"snapshot-secret-sentinel\"\n".to_owned());
        }
        ContextSnapshotScenario::Delete => {
            state.request_delete(context_key(ContextKind::Mcp, "alpha"));
        }
        ContextSnapshotScenario::Preview => install_context_preview(&mut state),
        ContextSnapshotScenario::Conflict => {
            state.open_create(ContextKind::Skill);
            state.set_editor_id("conflicting-skill".to_owned());
            let (mutation_id, _) = state.begin_save().unwrap();
            state.apply_stored_mutation_response(
                mutation_id,
                Err(ContextFailureKind::Service(
                    ContextToolsErrorKind::ProviderConflict,
                )),
            );
        }
        ContextSnapshotScenario::PartialOwnership => {
            install_context_preview(&mut state);
            let (sync_id, _) = state.begin_sync().unwrap();
            state.apply_sync_response(
                sync_id,
                Ok(Arc::new(ContextSyncOutcome {
                    bundle: (*context_bundle()).clone(),
                    backup_path: Some(
                        "C:/isolated/context/backups/context-backup-with-a-very-long-file-name.toml"
                            .to_owned(),
                    ),
                    ownership: ContextOwnershipOutcome::PartialFailure,
                    diff: ContextSyncDiffSummary::default(),
                })),
            );
        }
    }
    state
}

fn marketplace_snapshot_state(scenario: MarketplaceSnapshotScenario) -> MarketplaceViewState {
    let healthy = matches!(scenario, MarketplaceSnapshotScenario::Healthy);
    let mut state = MarketplaceViewState::default();
    let request_id = state.begin_inspection().unwrap();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(PluginMarketplaceWorkspace {
            revision: PluginMarketplaceRevision::from_digest([7; 32]),
            local: marketplace_status(healthy, 12, 34),
            remote: marketplace_status(healthy, 8, 21),
        })),
    );
    match scenario {
        MarketplaceSnapshotScenario::Healthy => {}
        MarketplaceSnapshotScenario::Confirmation => {
            state.request_repair_confirmation(PluginMarketplaceKind::Local);
        }
        MarketplaceSnapshotScenario::Running => {
            state.request_repair_confirmation(PluginMarketplaceKind::Local);
            state.confirm_repair().unwrap();
        }
        MarketplaceSnapshotScenario::Failure => {
            state.request_repair_confirmation(PluginMarketplaceKind::Remote);
            let (repair_id, _) = state.confirm_repair().unwrap();
            state.apply_repair_response(
                repair_id,
                PluginMarketplaceKind::Remote,
                Err(MarketplaceFailureKind::Service(
                    PluginMarketplaceErrorKind::WriteFailed,
                )),
            );
        }
    }
    state
}

fn session_snapshot_state(scenario: SessionSnapshotScenario) -> SessionViewState {
    let mut state = SessionViewState::default();
    if matches!(scenario, SessionSnapshotScenario::Loading) {
        state.begin_workspace_refresh();
        state.begin_provider_workspace_refresh().unwrap();
        return state;
    }

    install_session_workspace(&mut state, session_workspace(0..8));
    install_provider_sync_workspace(&mut state);
    match scenario {
        SessionSnapshotScenario::Loading => unreachable!("returned above"),
        SessionSnapshotScenario::Empty => {
            install_session_workspace(&mut state, SessionWorkspace::default());
        }
        SessionSnapshotScenario::Filtered => {
            state.set_filter(SessionFilter::Active);
            state.set_query("alpha".to_owned());
        }
        SessionSnapshotScenario::SelectionConfirmation => {
            state.select_all_filtered();
            state.request_delete();
        }
        SessionSnapshotScenario::PartialDeleteFailure => {
            state.set_selected("snapshot-session-0", true);
            state.set_selected("snapshot-session-1", true);
            state.request_delete();
            let (request_id, _) = state.confirm_delete().unwrap();
            state.apply_delete_response(
                request_id,
                Ok(Arc::new(SessionDeleteBatchOutcome {
                    outcomes: vec![
                        SessionDeleteOutcome::metadata_only(
                            "snapshot-session-0",
                            DeleteStatus::LocalDeleted,
                            Some("C:/isolated/session-backups/snapshot-session-0.json".to_owned()),
                        ),
                        SessionDeleteOutcome::metadata_only(
                            "snapshot-session-1",
                            DeleteStatus::Partial,
                            Some("C:/isolated/session-backups/snapshot-session-1.json".to_owned()),
                        ),
                    ],
                    workspace: session_workspace(2..8),
                })),
            );
        }
        SessionSnapshotScenario::ProviderRepairFailure => {
            state.request_provider_run_confirmation();
            let (request_id, _) = state.confirm_provider_run().unwrap();
            state.apply_provider_run_response(
                request_id,
                Err(ProviderSyncFailureKind::Service(
                    ProviderSyncErrorKind::SyncFailed,
                )),
            );
        }
    }
    state
}

fn user_script_snapshot_state(scenario: UserScriptSnapshotScenario) -> UserScriptViewState {
    let mut state = UserScriptViewState::default();
    if matches!(scenario, UserScriptSnapshotScenario::Loading) {
        state.begin_local_refresh();
        state.begin_market_refresh();
        return state;
    }

    let globally_enabled = !matches!(scenario, UserScriptSnapshotScenario::LocalGlobalOff);
    let local_request = state.begin_local_refresh();
    state.apply_local_response(
        local_request,
        Ok(Arc::new(user_script_workspace(globally_enabled, true))),
    );
    let integrity = if matches!(
        scenario,
        UserScriptSnapshotScenario::UnverifiedAcknowledgement
    ) {
        ScriptIntegrity::Unverified
    } else {
        ScriptIntegrity::Verified
    };
    let market_request = state.begin_market_refresh();
    state.apply_market_response(
        market_request,
        Ok(Arc::new(script_market_workspace(integrity))),
    );

    match scenario {
        UserScriptSnapshotScenario::Loading | UserScriptSnapshotScenario::MarketList => {}
        UserScriptSnapshotScenario::VerifiedConfirmation
        | UserScriptSnapshotScenario::UnverifiedAcknowledgement => {
            assert!(state.request_install("demo"));
        }
        UserScriptSnapshotScenario::IntegrityFailure => {
            assert!(state.request_install("demo"));
            let (request_id, _) = state.confirm_install().unwrap();
            state.apply_mutation_response(
                request_id,
                Err(UserScriptFailureKind::Service(
                    UserScriptErrorKind::IntegrityMismatch,
                )),
            );
        }
        UserScriptSnapshotScenario::LocalGlobalOff => {
            state.set_tab(ScriptsTab::Local);
        }
        UserScriptSnapshotScenario::DeleteConfirmation => {
            state.set_tab(ScriptsTab::Local);
            assert!(state.request_delete("user:custom.js"));
        }
        UserScriptSnapshotScenario::BackedUpResult => {
            state.set_tab(ScriptsTab::Local);
            assert!(state.request_delete("user:custom.js"));
            let (request_id, _) = state.confirm_delete().unwrap();
            state.apply_mutation_response(
                request_id,
                Ok(Arc::new(UserScriptMutationOutcome {
                    workspace: user_script_workspace(true, false),
                    backup: UserScriptBackupEvidence {
                        id: "snapshot-backup".to_owned(),
                        created: true,
                    },
                })),
            );
        }
    }
    state
}

fn user_script_workspace(globally_enabled: bool, include_custom: bool) -> UserScriptWorkspace {
    let mut scripts = vec![
        UserScriptSummary {
            key: "builtin:base.js".to_owned(),
            name: "Base renderer helper".to_owned(),
            origin: UserScriptOrigin::Builtin,
            enabled: true,
            status: UserScriptStatus::NotLoaded,
            market_id: None,
            version: None,
        },
        UserScriptSummary {
            key: "user:market-demo.js".to_owned(),
            name: "Installed market script".to_owned(),
            origin: UserScriptOrigin::User,
            enabled: true,
            status: UserScriptStatus::NotLoaded,
            market_id: Some("demo".to_owned()),
            version: Some("1".to_owned()),
        },
    ];
    if include_custom {
        scripts.insert(
            1,
            UserScriptSummary {
                key: "user:custom.js".to_owned(),
                name: "Custom workspace script".to_owned(),
                origin: UserScriptOrigin::User,
                enabled: false,
                status: UserScriptStatus::Disabled,
                market_id: None,
                version: None,
            },
        );
    }
    UserScriptWorkspace {
        revision: UserScriptRevision::from_digest([9; 32]),
        globally_enabled,
        scripts,
    }
}

fn script_market_workspace(integrity: ScriptIntegrity) -> ScriptMarketWorkspace {
    ScriptMarketWorkspace {
        revision: ScriptMarketRevision::from_digest([8; 32]),
        updated_at: Some("2026-07-18T00:00:00Z".to_owned()),
        entries: vec![
            ScriptMarketSummary {
                id: "demo".to_owned(),
                name: "Long metadata script name that must truncate without moving controls"
                    .to_owned(),
                description: "Keeps workspace metadata formatting consistent.".to_owned(),
                version: "2".to_owned(),
                author: "Snapshot fixture".to_owned(),
                tags: vec!["workflow".to_owned(), "metadata".to_owned()],
                source_host: "snapshot.invalid".to_owned(),
                homepage: None,
                integrity,
                installed_version: Some("1".to_owned()),
                update_available: true,
            },
            ScriptMarketSummary {
                id: "new-script".to_owned(),
                name: "Available verified script".to_owned(),
                description: "Adds a focused workspace utility.".to_owned(),
                version: "1".to_owned(),
                author: "Snapshot fixture".to_owned(),
                tags: vec!["utility".to_owned()],
                source_host: "snapshot.invalid".to_owned(),
                homepage: None,
                integrity: ScriptIntegrity::Verified,
                installed_version: None,
                update_available: false,
            },
        ],
    }
}

fn install_session_workspace(state: &mut SessionViewState, workspace: SessionWorkspace) {
    let request_id = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(request_id, Ok(Arc::new(workspace))));
}

fn install_provider_sync_workspace(state: &mut SessionViewState) {
    let request_id = state.begin_provider_workspace_refresh().unwrap();
    assert!(state.apply_provider_workspace_response(
        request_id,
        Ok(Arc::new(ProviderSyncWorkspace {
            targets: ProviderSyncTargetList {
                current_provider: "snapshot-provider".to_owned(),
                targets: vec![ProviderSyncTargetOption {
                    id: "snapshot-provider".to_owned(),
                    sources: vec![ProviderSyncTargetSource::Config],
                    is_current_provider: true,
                    is_manual: false,
                    is_saved: true,
                }],
            },
            selected_target: "snapshot-provider".to_owned(),
            auto_repair: true,
            revision: ProviderSyncRevision::from_digest([5; 32]),
        })),
    ));
}

fn session_workspace(indices: std::ops::Range<usize>) -> SessionWorkspace {
    SessionWorkspace {
        db_paths: vec![
            "C:/isolated/codex/sqlite/codex-dev.db".to_owned(),
            "C:/isolated/codex/state_5.sqlite".to_owned(),
        ],
        sessions: indices
            .map(|index| {
                let mut session = SessionSummary::new(
                    format!("snapshot-session-{index}"),
                    if index % 2 == 0 {
                        format!("Alpha snapshot session {index}")
                    } else {
                        format!("Beta snapshot session {index}")
                    },
                    SessionRevision::from_digest([index as u8; 32]),
                );
                session.cwd = format!("C:/isolated/workspaces/project-{index}");
                session.model_provider = "snapshot-provider".to_owned();
                session.archived = index % 3 == 1;
                session.updated_at_ms = Some(1_700_000_000_000 + index as i64);
                session.source_db_paths = vec!["codex-dev.db".to_owned()];
                session
            })
            .collect(),
        read_issues: Vec::new(),
    }
}

fn marketplace_status(
    healthy: bool,
    plugin_count: usize,
    skill_count: usize,
) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: if healthy { plugin_count } else { 0 },
        skill_count: if healthy { skill_count } else { 0 },
    }
}

fn install_context_preview(state: &mut ContextViewState) {
    let (request_id, _) = state.begin_preview().unwrap();
    state.apply_preview_response(
        request_id,
        Ok(Arc::new(ContextSyncPreview {
            guard: ContextSyncGuard {
                expected_provider_revision: ProviderRevision::parse("a".repeat(64)).unwrap(),
                expected_live_revision: ProviderLiveRevision::parse("b".repeat(64)).unwrap(),
                expected_ownership_revision: ContextOwnershipRevision::parse("c".repeat(64))
                    .unwrap(),
            },
            active_provider_id: Some("snapshot-provider".to_owned()),
            diff: ContextSyncDiffSummary {
                added: 1,
                updated: 1,
                removed: 1,
                unchanged: 2,
            },
            keys: ContextSyncKeys {
                added: vec![context_key(ContextKind::Plugin, "lint")],
                updated: vec![context_key(ContextKind::Mcp, "alpha")],
                removed: vec![context_key(ContextKind::Plugin, "old-plugin")],
                unchanged: vec![
                    context_key(ContextKind::Mcp, "stable"),
                    context_key(ContextKind::Skill, "review"),
                ],
            },
        })),
    );
}

fn context_bundle() -> Arc<ContextBundle> {
    let provider_revision = ProviderRevision::parse("a".repeat(64)).unwrap();
    Arc::new(ContextBundle {
        context: ContextWorkspace {
            provider_revision: provider_revision.clone(),
            live_revision: ProviderLiveRevision::parse("b".repeat(64)).unwrap(),
            ownership_revision: ContextOwnershipRevision::parse("c".repeat(64)).unwrap(),
            active_provider_id: Some("snapshot-provider".to_owned()),
            active_provider_name: Some("Snapshot provider".to_owned()),
            entries: vec![
                ContextEntrySummary {
                    key: context_key(ContextKind::Mcp, "alpha"),
                    display_name: "alpha".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::Matching,
                },
                ContextEntrySummary {
                    key: context_key(
                        ContextKind::Mcp,
                        "beta-with-a-very-long-context-entry-id-that-must-truncate-safely",
                    ),
                    display_name:
                        "beta-with-a-very-long-context-entry-id-that-must-truncate-safely"
                            .to_owned(),
                    enabled: false,
                    live_state: ContextEntryLiveState::Different,
                },
                ContextEntrySummary {
                    key: context_key(ContextKind::Skill, "review"),
                    display_name: "review".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::StoredOnly,
                },
                ContextEntrySummary {
                    key: context_key(ContextKind::Plugin, "lint"),
                    display_name: "lint".to_owned(),
                    enabled: true,
                    live_state: ContextEntryLiveState::PendingRemoval,
                },
            ],
            unmanaged_live_count: 2,
            sync_needed: true,
        },
        provider: ProviderWorkspace {
            revision: provider_revision,
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: true,
                active_profile_id: Some("snapshot-provider".to_owned()),
                active_profile_kind: None,
            },
            context_options: CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    })
}

fn context_key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_owned(),
    }
}

fn environment_state() -> EnvironmentViewState {
    let mut state = EnvironmentViewState::default();
    let request_id = state.begin_inspection();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(RelayEnvironmentWorkspace {
            report: RelayEnvironmentReport {
                clash_verge_tun: ClashVergeTunCheck {
                    enabled: false,
                    config_path: None,
                },
                proxy_environment: ProxyEnvironmentCheck {
                    variables: Vec::new(),
                },
                codex_env_file: CodexEnvFileCheck {
                    exists: false,
                    path: "fixture/.env".to_owned(),
                },
            },
            conflicts: vec![EnvConflict {
                name: "OPENAI_API_KEY".to_owned(),
                source: EnvConflictSource::Process,
                value_present: true,
            }],
            revision: "a".repeat(64),
        })),
    );
    state
}

fn import_state() -> ImportViewState {
    let mut state = ImportViewState::default();
    let request_id = state.begin_discovery();
    state.apply_discovery_response(
        request_id,
        Ok(Arc::new(CcsDiscovery {
            source_path: "fixture/cc-switch.db".to_owned(),
            source_revision: "b".repeat(64),
            provider_revision: ProviderRevision::parse("c".repeat(64)).unwrap(),
            providers: vec![CcsProviderSummary {
                source_id: "fixture".to_owned(),
                name: "Snapshot provider".to_owned(),
                base_url: "https://snapshot.invalid/v1".to_owned(),
                protocol: RelayProtocol::Responses,
                duplicate: false,
            }],
            importable_count: 1,
            duplicate_count: 0,
        })),
    );
    state
}
