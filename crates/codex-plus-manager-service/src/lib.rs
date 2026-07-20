mod context_tools;
mod desktop_host;
mod enhancements;
mod error;
mod maintenance;
mod manager_settings;
mod overview;
mod plugin_marketplace;
mod provider;
mod provider_activation;
mod provider_error;
mod provider_import;
mod provider_network;
mod provider_presets;
mod provider_sync;
mod provider_system;
mod relay_environment;
mod revision_ledger;
mod sessions;
mod system;
mod update;
mod user_scripts;
mod zed_remote;

pub use context_tools::{
    CompatContextDeleteRequest, CompatContextEntries, CompatContextEntryRequest, ContextBundle,
    ContextEntryDraft, ContextEntryKey, ContextEntryLiveState, ContextEntrySummary, ContextKind,
    ContextOwnershipOutcome, ContextSyncDiffSummary, ContextSyncGuard, ContextSyncKeys,
    ContextSyncOutcome, ContextSyncPreview, ContextSyncScope, ContextToolsEnvironment,
    ContextToolsError, ContextToolsErrorKind, ContextToolsService, ContextToolsSource,
    ContextWorkspace, DeleteContextEntry, LoadContextEntryDraft, PreviewContextSync,
    SaveContextEntry, SaveContextEntryMode, SetContextEntryEnabled, SyncContextToLive,
};
pub use desktop_host::{
    DesktopHostEnvironment, DesktopStartupArgs, DesktopStartupIssue, DesktopStartupIssueKind,
    DesktopStartupPlan,
};
pub use enhancements::{
    EnhancementError, EnhancementErrorKind, EnhancementRevision, EnhancementSettings,
    EnhancementSettingsEnvironment, EnhancementSettingsService, EnhancementSettingsSource,
    EnhancementWorkspace, ResetEnhancements, SaveEnhancements,
};
pub use error::{OverviewError, OverviewErrorKind};
pub use maintenance::{
    AppPathRevision, CodexAppSummary, CodexLaunchExecutor, CodexLaunchPlan, DiagnosticPathPresence,
    EntrypointSummary, LaunchCodex, LaunchOutcome, LaunchState, LaunchSummary, LoadMaintenance,
    MaintenanceEnvironment, MaintenanceError, MaintenanceErrorKind, MaintenanceIssue,
    MaintenanceSection, MaintenanceService, MaintenanceSource, MaintenanceWorkspace, PathKind,
    PrivatePath, RevisionedAppPath, SafeDiagnosticDocument, SafeErrorKind, SafeLogDocument,
    SafeLogEvent, SafeLogField, SafeLogRecord, SafeLogSeverity, SafeSettingsGroup,
    SaveCodexAppPath, SectionValue, WatcherSummary,
};
pub use manager_settings::{
    ConfirmedSecretClear, ExtraArgsRevision, ExtraArgsSettings, ImageOverlayFitMode,
    ImageOverlayRevision, ImageOverlaySettings, ManagerSettingsEnvironment, ManagerSettingsError,
    ManagerSettingsErrorKind, ManagerSettingsService, ManagerSettingsSource,
    ManagerSettingsWorkspace, PrivateArgument, PrivateUrl, ResetExtraArgs,
    ResetImageOverlaySettings, ResetStepwiseSettings, RevisionedExtraArgs,
    RevisionedImageOverlaySettings, RevisionedStepwiseSettings, SaveExtraArgs,
    SaveImageOverlaySettings, SaveStepwiseSettings, SecretReplacement, StepwiseConnectionTester,
    StepwiseRevision, StepwiseSecretChange, StepwiseSettings, StepwiseSettingsInput,
    StepwiseTestFailure, StepwiseTestOutcome, TestStepwiseSettings,
};
pub use overview::{
    LocatedResource, OverviewEnvironment, OverviewService, OverviewSnapshot, OverviewSource,
    ResourcePresence, ShortcutSnapshot, UpdateCheckState,
};
pub use plugin_marketplace::{
    PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceEnvironment, PluginMarketplaceError,
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRepair,
    PluginMarketplaceRepairOutcome, PluginMarketplaceRevision, PluginMarketplaceService,
    PluginMarketplaceSource, PluginMarketplaceStatus, PluginMarketplaceWorkspace,
    RepairPluginMarketplace,
};
pub use provider::{
    ExtractProviderCommonConfig, ProviderActivationSummary, ProviderCommonConfigExtraction,
    ProviderDocument, ProviderEnvironment, ProviderField, ProviderKind, ProviderProfile,
    ProviderRevision, ProviderService, ProviderSource, ProviderValidationIssue,
    ProviderValidationKind, ProviderWorkspace, SaveProviderWorkspace, ValidationSeverity,
    validate_provider_document,
};
pub use provider_activation::{
    ApplyActiveProvider, BackfillActiveProvider, ClearLiveProvider, ProviderActivationEnvironment,
    ProviderActivationError, ProviderActivationErrorKind, ProviderActivationSource,
    ProviderLiveFileKind, ProviderLiveFiles, ProviderLiveRevision, ProviderLiveWorkspace,
    ProviderMutationGuard, ProviderMutationOutcome, ProviderRollbackOutcome, SaveLiveFile,
    SwitchProvider,
};
pub use provider_error::{ProviderError, ProviderErrorKind};
pub use provider_import::{
    CcsDiscovery, CcsProviderSummary, ConfirmPendingImport, DismissPendingImport,
    ImportCcsProviders, PendingImportSnapshot, PendingImportSummary, ProviderImportEnvironment,
    ProviderImportError, ProviderImportErrorKind, ProviderImportOutcome, ProviderImportService,
    ProviderImportSource,
};
pub use provider_network::{
    DiagnoseProviderProfile, DoctorCheckStatus, DoctorDetailKind, DoctorOutcome,
    DoctorRecommendation, FetchProviderModels, NetworkModelsResponse, NetworkTestResponse,
    ProviderDoctorCheck, ProviderDoctorCheckId, ProviderDoctorReport,
    ProviderEnvironmentNetworkError, ProviderModelsResult, ProviderNetworkEnvironment,
    ProviderNetworkError, ProviderNetworkFailureKind, ProviderTestOutcome, ProviderTestResult,
    SafeEndpoint, TestProviderProfile, masked_auth_preview, masked_config_preview,
};
pub use provider_presets::{
    ProviderPreset, ProviderPresetCatalogError, ProviderPresetCatalogErrorKind,
    ProviderPresetCategory, apply_provider_preset, parse_provider_presets, provider_presets,
};
pub use provider_sync::{
    ProviderSyncEnvironment, ProviderSyncError, ProviderSyncErrorKind, ProviderSyncOutcome,
    ProviderSyncResult, ProviderSyncRevision, ProviderSyncService, ProviderSyncSource,
    ProviderSyncStatus, ProviderSyncTargetList, ProviderSyncTargetOption, ProviderSyncTargetSource,
    ProviderSyncWorkspace, RunProviderSync, SetProviderAutoRepair,
};
pub use provider_system::SystemProviderEnvironment;
pub use relay_environment::{
    EnvironmentRemovalOutcome, RelayEnvironmentEnvironment, RelayEnvironmentError,
    RelayEnvironmentErrorKind, RelayEnvironmentService, RelayEnvironmentSource,
    RelayEnvironmentWorkspace, RemoveEnvironmentConflicts,
};
pub use sessions::{
    CompatibilityUndoToken, DeleteSessionSelection, DeleteSessions, SessionDeleteBatchOutcome,
    SessionDeleteOutcome, SessionDeleteResult, SessionEnvironment, SessionError, SessionErrorKind,
    SessionLoadResult, SessionReadIssue, SessionReadIssueKind, SessionRevision, SessionService,
    SessionSource, SessionSummary, SessionWorkspace,
};
pub use system::{SystemOverviewEnvironment, SystemOverviewSource};
pub use update::{
    InstallStarted, InstallUpdate, SystemUpdateArtifact, SystemUpdateEnvironment,
    UpdateAvailability, UpdateCandidate, UpdateCheckResult, UpdateDownload, UpdateEnvironment,
    UpdateEnvironmentError, UpdateEnvironmentErrorKind, UpdateError, UpdateErrorKind, UpdateLimits,
    UpdateProgress, UpdateProgressSink, UpdateRevision, UpdateService, UpdateSource,
};
pub use user_scripts::{
    DeleteUserScript, InstallMarketScript, ScriptHomepage, ScriptIntegrity,
    ScriptMarketCompatibilityWorkspace, ScriptMarketRevision, ScriptMarketSummary,
    ScriptMarketWorkspace, SetUserScriptEnabled, SetUserScriptsEnabled, UserScriptBackupEvidence,
    UserScriptEnvironment, UserScriptError, UserScriptErrorKind, UserScriptMutationOutcome,
    UserScriptOrigin, UserScriptRevision, UserScriptService, UserScriptSource, UserScriptStatus,
    UserScriptSummary, UserScriptWorkspace,
};
pub use zed_remote::{
    ForgetZedRemoteProject, OpenZedRemoteProject, SaveZedPreferences, ZedLaunchExecutor,
    ZedProjectRevision, ZedRememberOutcome, ZedRemoteEnvironment, ZedRemoteError,
    ZedRemoteErrorKind, ZedRemoteOpenOutcome, ZedRemoteProjectSummary, ZedRemoteService,
    ZedRemoteSource, ZedRemoteWorkspace, ZedSettingsRevision,
};
