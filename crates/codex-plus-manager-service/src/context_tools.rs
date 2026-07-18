use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use codex_plus_core::context_ownership::{
    ContextEntryIdentity, ContextOwnershipManifest, ContextOwnershipRevision,
};
use codex_plus_core::relay_config::{
    CodexContextEntries, CodexContextEntry, acquire_relay_live_mutation_lock,
    acquire_relay_live_read_lock, apply_context_sync_config_file_to_home,
    context_entry_body_from_common_config, delete_context_entry_from_common_config,
    effective_context_config_for_profile, list_context_entries_from_common_config,
    plan_owned_context_sync, set_context_entry_enabled_in_common_config,
    upsert_context_entry_in_common_config,
};
use codex_plus_core::settings::BackendSettings;
use serde_json::json;

use crate::provider_activation::read_live_files;
use crate::{
    ProviderActivationEnvironment, ProviderLiveRevision, ProviderRevision, ProviderService,
    ProviderWorkspace,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextKind {
    Mcp,
    Skill,
    Plugin,
}

impl ContextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Skill => "skill",
            Self::Plugin => "plugin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mcp" | "mcpServer" | "mcpServers" => Some(Self::Mcp),
            "skill" | "skills" => Some(Self::Skill),
            "plugin" | "plugins" => Some(Self::Plugin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextEntryKey {
    pub kind: ContextKind,
    pub id: String,
}

impl ContextEntryKey {
    fn validate(&self) -> Result<(), ContextToolsError> {
        if self.id.trim().is_empty() || self.id.trim() != self.id {
            return Err(ContextToolsError::new(ContextToolsErrorKind::InvalidId));
        }
        Ok(())
    }

    fn identity(&self) -> ContextEntryIdentity {
        ContextEntryIdentity {
            kind: self.kind.as_str().to_string(),
            id: self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextEntryLiveState {
    StoredOnly,
    Matching,
    Different,
    PendingRemoval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEntrySummary {
    pub key: ContextEntryKey,
    pub display_name: String,
    pub enabled: bool,
    pub live_state: ContextEntryLiveState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextWorkspace {
    pub provider_revision: ProviderRevision,
    pub live_revision: ProviderLiveRevision,
    pub ownership_revision: ContextOwnershipRevision,
    pub active_provider_id: Option<String>,
    pub active_provider_name: Option<String>,
    pub entries: Vec<ContextEntrySummary>,
    pub unmanaged_live_count: usize,
    pub sync_needed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub context: ContextWorkspace,
    pub provider: ProviderWorkspace,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CompatContextEntries {
    pub settings: BackendSettings,
    pub entries: CodexContextEntries,
}

impl fmt::Debug for CompatContextEntries {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompatContextEntries")
            .field("entry_count", &context_entry_count(&self.entries))
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CompatContextEntryRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
    pub toml_body: String,
}

impl fmt::Debug for CompatContextEntryRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompatContextEntryRequest")
            .field("kind", &self.kind)
            .field("id", &self.id)
            .field("body_present", &!self.toml_body.is_empty())
            .field("body_length", &self.toml_body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CompatContextDeleteRequest {
    pub settings: BackendSettings,
    pub kind: String,
    pub id: String,
}

impl fmt::Debug for CompatContextDeleteRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompatContextDeleteRequest")
            .field("kind", &self.kind)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ContextEntryDraft {
    pub provider_revision: ProviderRevision,
    pub key: ContextEntryKey,
    pub toml_body: String,
}

impl fmt::Debug for ContextEntryDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextEntryDraft")
            .field("provider_revision", &self.provider_revision)
            .field("key", &self.key)
            .field("body_present", &!self.toml_body.is_empty())
            .field("body_length", &self.toml_body.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadContextEntryDraft {
    pub expected_provider_revision: ProviderRevision,
    pub key: ContextEntryKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveContextEntryMode {
    Create,
    Edit,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SaveContextEntry {
    pub expected_provider_revision: ProviderRevision,
    pub mode: SaveContextEntryMode,
    pub key: ContextEntryKey,
    pub toml_body: String,
}

impl fmt::Debug for SaveContextEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SaveContextEntry")
            .field(
                "expected_provider_revision",
                &self.expected_provider_revision,
            )
            .field("mode", &self.mode)
            .field("key", &self.key)
            .field("body_present", &!self.toml_body.is_empty())
            .field("body_length", &self.toml_body.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetContextEntryEnabled {
    pub expected_provider_revision: ProviderRevision,
    pub key: ContextEntryKey,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteContextEntry {
    pub expected_provider_revision: ProviderRevision,
    pub key: ContextEntryKey,
    pub confirmed_key: ContextEntryKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSyncScope {
    ActiveProvider,
    AllEnabledGlobal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSyncGuard {
    pub expected_provider_revision: ProviderRevision,
    pub expected_live_revision: ProviderLiveRevision,
    pub expected_ownership_revision: ContextOwnershipRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewContextSync {
    pub guard: ContextSyncGuard,
    pub scope: ContextSyncScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncContextToLive {
    pub guard: ContextSyncGuard,
    pub scope: ContextSyncScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSyncDiffSummary {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextSyncKeys {
    pub added: Vec<ContextEntryKey>,
    pub updated: Vec<ContextEntryKey>,
    pub removed: Vec<ContextEntryKey>,
    pub unchanged: Vec<ContextEntryKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSyncPreview {
    pub guard: ContextSyncGuard,
    pub active_provider_id: Option<String>,
    pub diff: ContextSyncDiffSummary,
    pub keys: ContextSyncKeys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextOwnershipOutcome {
    Updated,
    PartialFailure,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ContextSyncOutcome {
    pub bundle: ContextBundle,
    pub backup_path: Option<String>,
    pub ownership: ContextOwnershipOutcome,
    pub diff: ContextSyncDiffSummary,
}

impl fmt::Debug for ContextSyncOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSyncOutcome")
            .field("bundle", &self.bundle)
            .field("has_backup", &self.backup_path.is_some())
            .field("ownership", &self.ownership)
            .field("diff", &self.diff)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextToolsErrorKind {
    LoadFailed,
    LockFailed,
    ProviderConflict,
    LiveConflict,
    OwnershipConflict,
    InvalidId,
    EntryNotFound,
    EntryAlreadyExists,
    ConfirmationMismatch,
    InvalidToml,
    ActiveProviderMissing,
    ActiveProviderInvalid,
    SaveFailed,
    LiveWriteFailed,
    OwnershipWriteFailed,
}

pub struct ContextToolsError {
    kind: ContextToolsErrorKind,
}

impl ContextToolsError {
    fn new(kind: ContextToolsErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> ContextToolsErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ContextToolsErrorKind::LoadFailed => "context workspace load failed",
            ContextToolsErrorKind::LockFailed => "context workspace lock failed",
            ContextToolsErrorKind::ProviderConflict => "provider workspace changed on disk",
            ContextToolsErrorKind::LiveConflict => "live context changed on disk",
            ContextToolsErrorKind::OwnershipConflict => "context ownership changed on disk",
            ContextToolsErrorKind::InvalidId => "context id is invalid",
            ContextToolsErrorKind::EntryNotFound => "context entry was not found",
            ContextToolsErrorKind::EntryAlreadyExists => "context entry already exists",
            ContextToolsErrorKind::ConfirmationMismatch => "context confirmation does not match",
            ContextToolsErrorKind::InvalidToml => "context TOML is invalid",
            ContextToolsErrorKind::ActiveProviderMissing => "active provider is missing",
            ContextToolsErrorKind::ActiveProviderInvalid => "active provider is invalid",
            ContextToolsErrorKind::SaveFailed => "context settings save failed",
            ContextToolsErrorKind::LiveWriteFailed => "live context write failed",
            ContextToolsErrorKind::OwnershipWriteFailed => "context ownership write failed",
        }
    }
}

impl fmt::Debug for ContextToolsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextToolsError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ContextToolsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ContextToolsError {}

pub trait ContextToolsEnvironment: ProviderActivationEnvironment {
    fn load_context_ownership(&self) -> anyhow::Result<ContextOwnershipManifest>;
    fn save_context_ownership(&self, manifest: &ContextOwnershipManifest) -> anyhow::Result<()>;
}

pub trait ContextToolsSource: Send + Sync + 'static {
    fn load_workspace(&self) -> Result<ContextBundle, ContextToolsError>;
    fn load_entry_draft(
        &self,
        request: LoadContextEntryDraft,
    ) -> Result<ContextEntryDraft, ContextToolsError>;
    fn save_entry(&self, request: SaveContextEntry) -> Result<ContextBundle, ContextToolsError>;
    fn set_entry_enabled(
        &self,
        request: SetContextEntryEnabled,
    ) -> Result<ContextBundle, ContextToolsError>;
    fn delete_entry(&self, request: DeleteContextEntry)
    -> Result<ContextBundle, ContextToolsError>;
    fn preview_context_sync(
        &self,
        request: PreviewContextSync,
    ) -> Result<ContextSyncPreview, ContextToolsError>;
    fn sync_context_to_live(
        &self,
        request: SyncContextToLive,
    ) -> Result<ContextSyncOutcome, ContextToolsError>;
}

#[derive(Clone)]
pub struct ContextToolsService<E> {
    provider: ProviderService<E>,
}

impl<E> ContextToolsService<E> {
    pub fn new(environment: E) -> Self {
        Self {
            provider: ProviderService::new(environment),
        }
    }

    pub fn list_compat(&self, settings: BackendSettings) -> anyhow::Result<CompatContextEntries> {
        let entries =
            list_context_entries_from_common_config(&settings.relay_context_config_contents)?;
        Ok(CompatContextEntries { settings, entries })
    }

    pub fn upsert_compat(
        &self,
        request: CompatContextEntryRequest,
    ) -> anyhow::Result<CompatContextEntries> {
        let mut settings = request.settings;
        settings.relay_context_config_contents = upsert_context_entry_in_common_config(
            &settings.relay_context_config_contents,
            &request.kind,
            &request.id,
            &request.toml_body,
        )?;
        self.list_compat(settings)
    }

    pub fn delete_compat(
        &self,
        request: CompatContextDeleteRequest,
    ) -> anyhow::Result<CompatContextEntries> {
        let mut settings = request.settings;
        settings.relay_context_config_contents = delete_context_entry_from_common_config(
            &settings.relay_context_config_contents,
            &request.kind,
            &request.id,
        )?;
        self.list_compat(settings)
    }
}

impl<E: ContextToolsEnvironment> ContextToolsService<E> {
    pub fn read_live_compat(&self) -> anyhow::Result<CodexContextEntries> {
        let _lock = acquire_relay_live_read_lock(self.provider.environment().codex_home())?;
        let (files, _) = read_live_files(self.provider.environment().codex_home())?;
        list_context_entries_from_common_config(&files.config_contents)
    }

    pub fn sync_all_global_compat(
        &self,
        settings: &BackendSettings,
    ) -> anyhow::Result<CodexContextEntries> {
        let _lock = acquire_relay_live_mutation_lock(self.provider.environment().codex_home())?;
        let (files, _) = read_live_files(self.provider.environment().codex_home())?;
        let ownership = self.provider.environment().load_context_ownership()?;
        let plan = plan_owned_context_sync(
            &files.config_contents,
            &settings.relay_context_config_contents,
            &ownership,
        )?;
        plan.next_manifest.validated_json_bytes()?;
        apply_context_sync_config_file_to_home(
            self.provider.environment().codex_home(),
            &plan.updated_live_config,
        )?;
        self.provider
            .environment()
            .save_context_ownership(&plan.next_manifest)?;
        list_context_entries_from_common_config(&plan.updated_live_config)
    }

    pub fn load_workspace(&self) -> Result<ContextBundle, ContextToolsError> {
        let _lock = acquire_relay_live_read_lock(self.provider.environment().codex_home())
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LockFailed))?;
        self.load_bundle_locked()
    }

    pub fn load_entry_draft(
        &self,
        request: LoadContextEntryDraft,
    ) -> Result<ContextEntryDraft, ContextToolsError> {
        request.key.validate()?;
        let _lock = acquire_relay_live_read_lock(self.provider.environment().codex_home())
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LockFailed))?;
        let settings = self
            .provider
            .environment()
            .load_settings()
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        let provider = self
            .provider
            .workspace_from_settings(&settings)
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        if provider.revision != request.expected_provider_revision {
            return Err(ContextToolsError::new(
                ContextToolsErrorKind::ProviderConflict,
            ));
        }
        let body = context_entry_body_from_common_config(
            &settings.relay_context_config_contents,
            request.key.kind.as_str(),
            &request.key.id,
        )
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?
        .ok_or_else(|| ContextToolsError::new(ContextToolsErrorKind::EntryNotFound))?;
        Ok(ContextEntryDraft {
            provider_revision: provider.revision,
            key: request.key,
            toml_body: body,
        })
    }

    pub fn preview_context_sync(
        &self,
        request: PreviewContextSync,
    ) -> Result<ContextSyncPreview, ContextToolsError> {
        let _lock = acquire_relay_live_read_lock(self.provider.environment().codex_home())
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LockFailed))?;
        let sources = self.load_sources_locked()?;
        validate_context_sync_guard(&sources, &request.guard)?;
        let (desired, active_provider_id) =
            desired_context_config(&sources.settings, request.scope)?;
        let plan = plan_owned_context_sync(&sources.live_config, &desired, &sources.ownership)
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
        let (diff, keys) = context_sync_projection(&plan.diff)?;
        Ok(ContextSyncPreview {
            guard: request.guard,
            active_provider_id,
            diff,
            keys,
        })
    }

    pub fn sync_context_to_live(
        &self,
        request: SyncContextToLive,
    ) -> Result<ContextSyncOutcome, ContextToolsError> {
        let _lock = acquire_relay_live_mutation_lock(self.provider.environment().codex_home())
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LockFailed))?;
        let sources = self.load_sources_locked()?;
        validate_context_sync_guard(&sources, &request.guard)?;
        let (desired, _) = desired_context_config(&sources.settings, request.scope)?;
        let plan = plan_owned_context_sync(&sources.live_config, &desired, &sources.ownership)
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
        plan.next_manifest
            .validated_json_bytes()
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
        let (diff, _) = context_sync_projection(&plan.diff)?;
        let applied = apply_context_sync_config_file_to_home(
            self.provider.environment().codex_home(),
            &plan.updated_live_config,
        )
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LiveWriteFailed))?;
        let ownership = if self
            .provider
            .environment()
            .save_context_ownership(&plan.next_manifest)
            .is_ok()
        {
            ContextOwnershipOutcome::Updated
        } else {
            ContextOwnershipOutcome::PartialFailure
        };
        let bundle = self.load_bundle_locked()?;
        Ok(ContextSyncOutcome {
            bundle,
            backup_path: applied.backup_path,
            ownership,
            diff,
        })
    }

    pub fn save_entry(
        &self,
        request: SaveContextEntry,
    ) -> Result<ContextBundle, ContextToolsError> {
        request.key.validate()?;
        let key = request.key.clone();
        let mode = request.mode;
        let body = request.toml_body;
        self.mutate_stored(request.expected_provider_revision, move |settings| {
            let existing = context_entry_body_from_common_config(
                &settings.relay_context_config_contents,
                key.kind.as_str(),
                &key.id,
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
            match (mode, existing.is_some()) {
                (SaveContextEntryMode::Create, true) => {
                    return Err(ContextToolsError::new(
                        ContextToolsErrorKind::EntryAlreadyExists,
                    ));
                }
                (SaveContextEntryMode::Edit, false) => {
                    return Err(ContextToolsError::new(ContextToolsErrorKind::EntryNotFound));
                }
                _ => {}
            }
            upsert_context_entry_in_common_config(
                &settings.relay_context_config_contents,
                key.kind.as_str(),
                &key.id,
                &body,
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))
        })
    }

    pub fn set_entry_enabled(
        &self,
        request: SetContextEntryEnabled,
    ) -> Result<ContextBundle, ContextToolsError> {
        request.key.validate()?;
        let key = request.key.clone();
        let enabled = request.enabled;
        self.mutate_stored(request.expected_provider_revision, move |settings| {
            ensure_context_entry_exists(settings, &key)?;
            set_context_entry_enabled_in_common_config(
                &settings.relay_context_config_contents,
                key.kind.as_str(),
                &key.id,
                enabled,
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))
        })
    }

    pub fn delete_entry(
        &self,
        request: DeleteContextEntry,
    ) -> Result<ContextBundle, ContextToolsError> {
        request.key.validate()?;
        request.confirmed_key.validate()?;
        if request.key != request.confirmed_key {
            return Err(ContextToolsError::new(
                ContextToolsErrorKind::ConfirmationMismatch,
            ));
        }
        let key = request.key.clone();
        self.mutate_stored(request.expected_provider_revision, move |settings| {
            ensure_context_entry_exists(settings, &key)?;
            delete_context_entry_from_common_config(
                &settings.relay_context_config_contents,
                key.kind.as_str(),
                &key.id,
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))
        })
    }

    fn mutate_stored<F>(
        &self,
        expected_revision: ProviderRevision,
        transform: F,
    ) -> Result<ContextBundle, ContextToolsError>
    where
        F: FnOnce(&BackendSettings) -> Result<String, ContextToolsError>,
    {
        let current = self
            .provider
            .environment()
            .load_settings()
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        let current_workspace = self
            .provider
            .workspace_from_settings(&current)
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        if current_workspace.revision != expected_revision {
            return Err(ContextToolsError::new(
                ContextToolsErrorKind::ProviderConflict,
            ));
        }

        let context_config = transform(&current)?;
        let predicate_revision = expected_revision.clone();
        let updated = self
            .provider
            .environment()
            .update_settings_if(
                json!({"relayContextConfigContents": context_config}),
                |fresh| {
                    self.provider
                        .workspace_from_settings(fresh)
                        .is_ok_and(|workspace| workspace.revision == predicate_revision)
                },
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::SaveFailed))?;
        if updated.is_none() {
            return Err(ContextToolsError::new(
                ContextToolsErrorKind::ProviderConflict,
            ));
        }
        self.load_workspace()
    }

    fn load_bundle_locked(&self) -> Result<ContextBundle, ContextToolsError> {
        let sources = self.load_sources_locked()?;
        context_bundle_from_sources(sources)
    }

    fn load_sources_locked(&self) -> Result<LoadedContextSources, ContextToolsError> {
        let settings = self
            .provider
            .environment()
            .load_settings()
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        let provider = self
            .provider
            .workspace_from_settings(&settings)
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        let (live_files, live_revision) = read_live_files(self.provider.environment().codex_home())
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        let ownership = self
            .provider
            .environment()
            .load_context_ownership()
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::LoadFailed))?;
        Ok(LoadedContextSources {
            settings,
            provider,
            live_config: live_files.config_contents,
            live_revision,
            ownership,
        })
    }
}

impl<E: ContextToolsEnvironment> ContextToolsSource for ContextToolsService<E> {
    fn load_workspace(&self) -> Result<ContextBundle, ContextToolsError> {
        ContextToolsService::load_workspace(self)
    }

    fn load_entry_draft(
        &self,
        request: LoadContextEntryDraft,
    ) -> Result<ContextEntryDraft, ContextToolsError> {
        ContextToolsService::load_entry_draft(self, request)
    }

    fn save_entry(&self, request: SaveContextEntry) -> Result<ContextBundle, ContextToolsError> {
        ContextToolsService::save_entry(self, request)
    }

    fn set_entry_enabled(
        &self,
        request: SetContextEntryEnabled,
    ) -> Result<ContextBundle, ContextToolsError> {
        ContextToolsService::set_entry_enabled(self, request)
    }

    fn delete_entry(
        &self,
        request: DeleteContextEntry,
    ) -> Result<ContextBundle, ContextToolsError> {
        ContextToolsService::delete_entry(self, request)
    }

    fn preview_context_sync(
        &self,
        request: PreviewContextSync,
    ) -> Result<ContextSyncPreview, ContextToolsError> {
        ContextToolsService::preview_context_sync(self, request)
    }

    fn sync_context_to_live(
        &self,
        request: SyncContextToLive,
    ) -> Result<ContextSyncOutcome, ContextToolsError> {
        ContextToolsService::sync_context_to_live(self, request)
    }
}

struct LoadedContextSources {
    settings: BackendSettings,
    provider: ProviderWorkspace,
    live_config: String,
    live_revision: ProviderLiveRevision,
    ownership: ContextOwnershipManifest,
}

fn context_bundle_from_sources(
    sources: LoadedContextSources,
) -> Result<ContextBundle, ContextToolsError> {
    let context = context_workspace_from_sources(
        &sources.settings,
        &sources.provider,
        &sources.live_config,
        sources.live_revision,
        &sources.ownership,
    )?;
    Ok(ContextBundle {
        context,
        provider: sources.provider,
    })
}

fn validate_context_sync_guard(
    sources: &LoadedContextSources,
    guard: &ContextSyncGuard,
) -> Result<(), ContextToolsError> {
    if sources.provider.revision != guard.expected_provider_revision {
        return Err(ContextToolsError::new(
            ContextToolsErrorKind::ProviderConflict,
        ));
    }
    if sources.live_revision != guard.expected_live_revision {
        return Err(ContextToolsError::new(ContextToolsErrorKind::LiveConflict));
    }
    if sources.ownership.revision() != guard.expected_ownership_revision {
        return Err(ContextToolsError::new(
            ContextToolsErrorKind::OwnershipConflict,
        ));
    }
    Ok(())
}

fn desired_context_config(
    settings: &BackendSettings,
    scope: ContextSyncScope,
) -> Result<(String, Option<String>), ContextToolsError> {
    match scope {
        ContextSyncScope::AllEnabledGlobal => {
            Ok((settings.relay_context_config_contents.clone(), None))
        }
        ContextSyncScope::ActiveProvider => {
            if !settings.relay_profiles_enabled || settings.active_relay_id.trim().is_empty() {
                return Err(ContextToolsError::new(
                    ContextToolsErrorKind::ActiveProviderMissing,
                ));
            }
            let profile = settings
                .relay_profiles
                .iter()
                .find(|profile| profile.id == settings.active_relay_id)
                .ok_or_else(|| {
                    ContextToolsError::new(ContextToolsErrorKind::ActiveProviderMissing)
                })?;
            let desired = effective_context_config_for_profile(
                &settings.relay_context_config_contents,
                profile,
            )
            .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::ActiveProviderInvalid))?;
            Ok((desired, Some(profile.id.clone())))
        }
    }
}

fn context_sync_projection(
    diff: &codex_plus_core::context_ownership::ContextSyncDiff,
) -> Result<(ContextSyncDiffSummary, ContextSyncKeys), ContextToolsError> {
    let keys = ContextSyncKeys {
        added: context_keys_from_identities(&diff.added)?,
        updated: context_keys_from_identities(&diff.updated)?,
        removed: context_keys_from_identities(&diff.removed)?,
        unchanged: context_keys_from_identities(&diff.unchanged)?,
    };
    let summary = ContextSyncDiffSummary {
        added: keys.added.len(),
        updated: keys.updated.len(),
        removed: keys.removed.len(),
        unchanged: keys.unchanged.len(),
    };
    Ok((summary, keys))
}

fn context_keys_from_identities(
    identities: &[ContextEntryIdentity],
) -> Result<Vec<ContextEntryKey>, ContextToolsError> {
    identities
        .iter()
        .map(|identity| {
            context_key_from_identity(identity)
                .ok_or_else(|| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))
        })
        .collect()
}

fn ensure_context_entry_exists(
    settings: &BackendSettings,
    key: &ContextEntryKey,
) -> Result<(), ContextToolsError> {
    let exists = context_entry_body_from_common_config(
        &settings.relay_context_config_contents,
        key.kind.as_str(),
        &key.id,
    )
    .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?
    .is_some();
    if !exists {
        return Err(ContextToolsError::new(ContextToolsErrorKind::EntryNotFound));
    }
    Ok(())
}

fn context_workspace_from_sources(
    settings: &BackendSettings,
    provider: &ProviderWorkspace,
    live_config: &str,
    live_revision: ProviderLiveRevision,
    ownership: &ContextOwnershipManifest,
) -> Result<ContextWorkspace, ContextToolsError> {
    let stored = list_context_entries_from_common_config(&settings.relay_context_config_contents)
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
    let live = list_context_entries_from_common_config(live_config)
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
    let stored = context_entry_map(stored);
    let live = context_entry_map(live);
    let owned = ownership
        .entries
        .iter()
        .map(|entry| entry.identity.clone())
        .collect::<BTreeSet<_>>();
    let active_provider_id = provider
        .activation
        .enabled
        .then(|| provider.activation.active_profile_id.clone())
        .flatten();
    let desired = active_provider_id
        .as_ref()
        .and_then(|active_id| {
            settings
                .relay_profiles
                .iter()
                .find(|profile| profile.id == *active_id)
        })
        .map(|profile| {
            effective_context_config_for_profile(&settings.relay_context_config_contents, profile)
        })
        .transpose()
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::ActiveProviderInvalid))?
        .unwrap_or_else(|| settings.relay_context_config_contents.clone());
    let plan = plan_owned_context_sync(live_config, &desired, ownership)
        .map_err(|_| ContextToolsError::new(ContextToolsErrorKind::InvalidToml))?;
    let desired = plan
        .next_manifest
        .entries
        .iter()
        .map(|entry| entry.identity.clone())
        .collect::<BTreeSet<_>>();

    let entries = stored
        .iter()
        .map(|(key, entry)| {
            let identity = key.identity();
            let live_state = if !desired.contains(&identity) {
                if owned.contains(&key.identity()) {
                    ContextEntryLiveState::PendingRemoval
                } else if live.contains_key(key) {
                    ContextEntryLiveState::Different
                } else {
                    ContextEntryLiveState::StoredOnly
                }
            } else {
                match live.get(key) {
                    None => ContextEntryLiveState::StoredOnly,
                    Some(live) if live.toml_body == entry.toml_body => {
                        ContextEntryLiveState::Matching
                    }
                    Some(_) => ContextEntryLiveState::Different,
                }
            };
            ContextEntrySummary {
                key: key.clone(),
                display_name: key.id.clone(),
                enabled: entry.enabled,
                live_state,
            }
        })
        .collect::<Vec<_>>();
    let unmanaged_live_count = live
        .keys()
        .filter(|key| !stored.contains_key(*key) && !owned.contains(&key.identity()))
        .count();
    let sync_needed = !plan.diff.added.is_empty()
        || !plan.diff.updated.is_empty()
        || !plan.diff.removed.is_empty()
        || plan.next_manifest != *ownership;
    let active_provider_name = active_provider_id.as_ref().and_then(|active_id| {
        provider
            .document
            .profiles
            .iter()
            .find(|profile| profile.id() == active_id)
            .map(|profile| profile.name().to_string())
    });

    Ok(ContextWorkspace {
        provider_revision: provider.revision.clone(),
        live_revision,
        ownership_revision: ownership.revision(),
        active_provider_id,
        active_provider_name,
        entries,
        unmanaged_live_count,
        sync_needed,
    })
}

fn context_entry_map(entries: CodexContextEntries) -> BTreeMap<ContextEntryKey, CodexContextEntry> {
    let mut mapped = BTreeMap::new();
    for (kind, entries) in [
        (ContextKind::Mcp, entries.mcp_servers),
        (ContextKind::Skill, entries.skills),
        (ContextKind::Plugin, entries.plugins),
    ] {
        for entry in entries {
            mapped.insert(
                ContextEntryKey {
                    kind,
                    id: entry.id.clone(),
                },
                entry,
            );
        }
    }
    mapped
}

fn context_entry_count(entries: &CodexContextEntries) -> usize {
    entries.mcp_servers.len() + entries.skills.len() + entries.plugins.len()
}

fn context_key_from_identity(identity: &ContextEntryIdentity) -> Option<ContextEntryKey> {
    Some(ContextEntryKey {
        kind: ContextKind::parse(&identity.kind)?,
        id: identity.id.clone(),
    })
}
