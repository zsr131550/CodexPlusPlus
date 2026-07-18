use std::collections::BTreeSet;
use std::fmt;

use codex_plus_core::settings::BackendSettings;
use codex_plus_data::{
    ProviderSyncResult, ProviderSyncStatus, ProviderSyncTargetList, ProviderSyncTargetOption,
    ProviderSyncTargetSource,
};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProviderSyncRevision([u8; 32]);

impl ProviderSyncRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for ProviderSyncRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderSyncRevision(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSyncWorkspace {
    pub targets: ProviderSyncTargetList,
    pub selected_target: String,
    pub auto_repair: bool,
    pub revision: ProviderSyncRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProviderSync {
    pub target_provider: String,
    pub confirmed_target_provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetProviderAutoRepair {
    pub expected_revision: ProviderSyncRevision,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSyncErrorKind {
    LoadFailed,
    ConfirmationMismatch,
    SettingsConflict,
    SyncFailed,
}

pub struct ProviderSyncError {
    kind: ProviderSyncErrorKind,
    compatibility_detail: Option<String>,
}

impl ProviderSyncError {
    pub fn new(kind: ProviderSyncErrorKind) -> Self {
        Self {
            kind,
            compatibility_detail: None,
        }
    }

    pub(crate) fn with_compatibility_detail(kind: ProviderSyncErrorKind, detail: String) -> Self {
        Self {
            kind,
            compatibility_detail: Some(detail),
        }
    }

    pub fn kind(&self) -> ProviderSyncErrorKind {
        self.kind
    }

    pub fn compatibility_detail(&self) -> Option<&str> {
        self.compatibility_detail.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ProviderSyncErrorKind::LoadFailed => "provider sync workspace load failed",
            ProviderSyncErrorKind::ConfirmationMismatch => {
                "provider sync confirmation does not match"
            }
            ProviderSyncErrorKind::SettingsConflict => "provider sync settings changed on disk",
            ProviderSyncErrorKind::SyncFailed => "provider sync failed",
        }
    }
}

impl fmt::Debug for ProviderSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderSyncError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ProviderSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ProviderSyncError {}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderSyncOutcome {
    pub result: ProviderSyncResult,
    pub workspace: ProviderSyncWorkspace,
}

impl fmt::Debug for ProviderSyncOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderSyncOutcome")
            .field("status", &self.result.status)
            .field("target_provider", &self.result.target_provider)
            .field("changed_session_files", &self.result.changed_session_files)
            .field("sqlite_rows_updated", &self.result.sqlite_rows_updated)
            .field(
                "skipped_locked_rollout_file_count",
                &self.result.skipped_locked_rollout_files.len(),
            )
            .field("has_backup", &self.result.backup_dir.is_some())
            .field("workspace", &self.workspace)
            .finish()
    }
}

pub trait ProviderSyncEnvironment: Send + Sync + 'static {
    fn load_provider_sync_settings(&self) -> anyhow::Result<BackendSettings>;
    fn load_provider_sync_targets(&self) -> ProviderSyncTargetList;
    fn run_provider_sync(&self, target: &str) -> ProviderSyncResult;
    fn save_provider_sync_enabled(
        &self,
        expected: &ProviderSyncRevision,
        enabled: bool,
    ) -> Result<(), ProviderSyncError>;
    fn save_provider_sync_target(&self, target: &str) -> Result<(), ProviderSyncError>;
}

pub trait ProviderSyncSource: Send + Sync + 'static {
    fn load_provider_sync_workspace(&self) -> Result<ProviderSyncWorkspace, ProviderSyncError>;
    fn run_provider_sync(
        &self,
        request: RunProviderSync,
    ) -> Result<ProviderSyncOutcome, ProviderSyncError>;
    fn set_provider_auto_repair(
        &self,
        request: SetProviderAutoRepair,
    ) -> Result<ProviderSyncWorkspace, ProviderSyncError>;
}

#[derive(Clone)]
pub struct ProviderSyncService<E> {
    environment: E,
}

impl<E> ProviderSyncService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: ProviderSyncEnvironment> ProviderSyncService<E> {
    fn load_workspace_inner(&self) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        let settings = self
            .environment
            .load_provider_sync_settings()
            .map_err(|error| {
                ProviderSyncError::with_compatibility_detail(
                    ProviderSyncErrorKind::LoadFailed,
                    format!("{error:#}"),
                )
            })?;
        let mut targets = self.environment.load_provider_sync_targets();
        merge_settings_targets(&mut targets, &settings);
        let selected_target = if settings
            .provider_sync_last_selected_provider
            .trim()
            .is_empty()
        {
            targets.current_provider.clone()
        } else {
            settings
                .provider_sync_last_selected_provider
                .trim()
                .to_owned()
        };
        Ok(ProviderSyncWorkspace {
            targets,
            selected_target,
            auto_repair: settings.provider_sync_enabled,
            revision: provider_sync_revision(&settings),
        })
    }
}

impl<E: ProviderSyncEnvironment> ProviderSyncSource for ProviderSyncService<E> {
    fn load_provider_sync_workspace(&self) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        self.load_workspace_inner()
    }

    fn run_provider_sync(
        &self,
        request: RunProviderSync,
    ) -> Result<ProviderSyncOutcome, ProviderSyncError> {
        let target = request.target_provider.trim();
        if target.is_empty() || target != request.confirmed_target_provider.trim() {
            return Err(ProviderSyncError::new(
                ProviderSyncErrorKind::ConfirmationMismatch,
            ));
        }
        let result = self.environment.run_provider_sync(target);
        if result.status == ProviderSyncStatus::Synced {
            self.environment.save_provider_sync_target(target)?;
        }
        let workspace = self.load_workspace_inner()?;
        Ok(ProviderSyncOutcome { result, workspace })
    }

    fn set_provider_auto_repair(
        &self,
        request: SetProviderAutoRepair,
    ) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        let current = self.load_workspace_inner()?;
        if current.revision != request.expected_revision {
            return Err(ProviderSyncError::new(
                ProviderSyncErrorKind::SettingsConflict,
            ));
        }
        self.environment
            .save_provider_sync_enabled(&request.expected_revision, request.enabled)?;
        self.load_workspace_inner()
    }
}

pub(crate) fn provider_sync_revision(settings: &BackendSettings) -> ProviderSyncRevision {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, &[u8::from(settings.provider_sync_enabled)]);
    for provider in normalized_provider_ids(&settings.provider_sync_manual_providers) {
        hash_field(&mut hasher, b"manual");
        hash_field(&mut hasher, provider.as_bytes());
    }
    for provider in normalized_provider_ids(&settings.provider_sync_saved_providers) {
        hash_field(&mut hasher, b"saved");
        hash_field(&mut hasher, provider.as_bytes());
    }
    hash_field(
        &mut hasher,
        settings
            .provider_sync_last_selected_provider
            .trim()
            .as_bytes(),
    );
    ProviderSyncRevision(hasher.finalize().into())
}

pub(crate) fn normalized_provider_ids(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn merge_settings_targets(targets: &mut ProviderSyncTargetList, settings: &BackendSettings) {
    let manual = normalized_provider_ids(&settings.provider_sync_manual_providers);
    let saved = normalized_provider_ids(&settings.provider_sync_saved_providers);
    for id in manual.iter().chain(&saved) {
        if let Some(existing) = targets.targets.iter_mut().find(|target| target.id == *id) {
            if !existing.sources.contains(&ProviderSyncTargetSource::Manual) {
                existing.sources.push(ProviderSyncTargetSource::Manual);
                existing.sources.sort();
            }
            existing.is_manual = manual.contains(id);
            existing.is_saved = saved.contains(id);
        } else {
            targets.targets.push(ProviderSyncTargetOption {
                id: id.clone(),
                sources: vec![ProviderSyncTargetSource::Manual],
                is_current_provider: *id == targets.current_provider,
                is_manual: manual.contains(id),
                is_saved: saved.contains(id),
            });
        }
    }
    targets.targets.sort_by(|left, right| {
        right
            .is_current_provider
            .cmp(&left.is_current_provider)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}
