use std::fmt;
use std::path::Path;

use codex_plus_core::relay_config::{
    RelayStatus, acquire_relay_live_mutation_lock, acquire_relay_live_read_lock,
    apply_relay_auth_file_to_home, apply_relay_config_file_to_home,
    backfill_relay_profile_from_home_with_common,
    clear_relay_config_to_home_with_auth_and_computer_use_guard, relay_profile_api_key,
    relay_profile_base_url, relay_status_from_home,
};
use codex_plus_core::relay_switch::{
    RelayRollbackOutcome, RelaySwitchError, switch_relay_profile_in_home,
};
use codex_plus_core::settings::{BackendSettings, RelayMode, SettingsStore};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{ProviderEnvironment, ProviderRevision, ProviderService, ProviderWorkspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderActivationErrorKind {
    LoadFailed,
    LockFailed,
    Disabled,
    ProfileNotFound,
    ProviderConflict,
    LiveConflict,
    InvalidLiveFile,
    UnsupportedAction,
    MutationFailed,
    RollbackFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRollbackOutcome {
    NotRequired,
    Verified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderLiveRevision(String);

impl ProviderLiveRevision {
    pub fn parse(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderLiveFiles {
    pub config_path: String,
    pub auth_path: String,
    pub config_exists: bool,
    pub auth_exists: bool,
    pub config_contents: String,
    pub auth_contents: String,
}

impl fmt::Debug for ProviderLiveFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderLiveFiles")
            .field("config_path_present", &!self.config_path.is_empty())
            .field("auth_path_present", &!self.auth_path.is_empty())
            .field("config_exists", &self.config_exists)
            .field("auth_exists", &self.auth_exists)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderLiveWorkspace {
    pub provider: ProviderWorkspace,
    pub status: RelayStatus,
    pub files: ProviderLiveFiles,
    pub revision: ProviderLiveRevision,
}

impl fmt::Debug for ProviderLiveWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderLiveWorkspace")
            .field("provider", &self.provider)
            .field("configured", &self.status.configured)
            .field("authenticated", &self.status.authenticated)
            .field("files", &self.files)
            .field("revision", &self.revision)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMutationGuard {
    pub expected_provider_revision: ProviderRevision,
    pub expected_live_revision: ProviderLiveRevision,
}

#[derive(Clone)]
pub struct SwitchProvider {
    pub guard: ProviderMutationGuard,
    pub target_profile_id: String,
}

#[derive(Debug, Clone)]
pub struct ApplyActiveProvider {
    pub guard: ProviderMutationGuard,
}

#[derive(Debug, Clone)]
pub struct ClearLiveProvider {
    pub guard: ProviderMutationGuard,
}

#[derive(Debug, Clone)]
pub struct BackfillActiveProvider {
    pub guard: ProviderMutationGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLiveFileKind {
    Config,
    Auth,
}

#[derive(Clone)]
pub struct SaveLiveFile {
    pub guard: ProviderMutationGuard,
    pub kind: ProviderLiveFileKind,
    pub contents: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderMutationOutcome {
    pub live: ProviderLiveWorkspace,
    pub backup_path: Option<String>,
    pub rollback: ProviderRollbackOutcome,
}

impl fmt::Debug for ProviderMutationOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMutationOutcome")
            .field("live", &self.live)
            .field("has_backup", &self.backup_path.is_some())
            .field("rollback", &self.rollback)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderActivationError {
    kind: ProviderActivationErrorKind,
    rollback: ProviderRollbackOutcome,
    backup_path: Option<String>,
}

impl ProviderActivationError {
    pub fn for_failure(
        kind: ProviderActivationErrorKind,
        rollback: ProviderRollbackOutcome,
        backup_path: Option<String>,
    ) -> Self {
        Self {
            kind,
            rollback,
            backup_path,
        }
    }

    fn new(kind: ProviderActivationErrorKind) -> Self {
        Self {
            kind,
            rollback: ProviderRollbackOutcome::NotRequired,
            backup_path: None,
        }
    }

    fn mutation(
        kind: ProviderActivationErrorKind,
        rollback: ProviderRollbackOutcome,
        backup_path: Option<String>,
    ) -> Self {
        Self {
            kind,
            rollback,
            backup_path,
        }
    }

    pub fn kind(&self) -> ProviderActivationErrorKind {
        self.kind
    }

    pub fn rollback(&self) -> ProviderRollbackOutcome {
        self.rollback
    }

    pub fn backup_path(&self) -> Option<&str> {
        self.backup_path.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ProviderActivationErrorKind::LoadFailed => "provider live state load failed",
            ProviderActivationErrorKind::LockFailed => "provider live state lock failed",
            ProviderActivationErrorKind::Disabled => "provider activation is disabled",
            ProviderActivationErrorKind::ProfileNotFound => "provider profile was not found",
            ProviderActivationErrorKind::ProviderConflict => "provider workspace changed on disk",
            ProviderActivationErrorKind::LiveConflict => "provider live files changed on disk",
            ProviderActivationErrorKind::InvalidLiveFile => "provider live file is invalid",
            ProviderActivationErrorKind::UnsupportedAction => "provider live action is unsupported",
            ProviderActivationErrorKind::MutationFailed => "provider live mutation failed",
            ProviderActivationErrorKind::RollbackFailed => "provider live rollback failed",
        }
    }
}

impl fmt::Debug for ProviderActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderActivationError")
            .field("kind", &self.kind)
            .field("rollback", &self.rollback)
            .field("has_backup", &self.backup_path.is_some())
            .finish()
    }
}

impl fmt::Display for ProviderActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ProviderActivationError {}

pub trait ProviderActivationEnvironment: ProviderEnvironment {
    fn settings_store(&self) -> &SettingsStore;
    fn codex_home(&self) -> &Path;
}

pub trait ProviderActivationSource: Send + Sync + 'static {
    fn load_live_workspace(&self) -> Result<ProviderLiveWorkspace, ProviderActivationError>;
    fn switch_provider(
        &self,
        request: SwitchProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError>;
    fn apply_active_provider(
        &self,
        request: ApplyActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError>;
    fn clear_live_provider(
        &self,
        request: ClearLiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError>;
    fn backfill_active_provider(
        &self,
        request: BackfillActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError>;
    fn save_live_file(
        &self,
        request: SaveLiveFile,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError>;
}

impl<E: ProviderActivationEnvironment> ProviderService<E> {
    fn load_activation_state(
        &self,
    ) -> Result<(BackendSettings, ProviderLiveWorkspace), ProviderActivationError> {
        let settings = self
            .environment()
            .load_settings()
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LoadFailed))?;
        let provider = self
            .workspace_from_settings(&settings)
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LoadFailed))?;
        let (files, revision) = read_live_files(self.environment().codex_home())?;
        let status = relay_status_from_home(self.environment().codex_home());
        Ok((
            settings,
            ProviderLiveWorkspace {
                provider,
                status,
                files,
                revision,
            },
        ))
    }

    fn validate_guard(
        &self,
        current: &ProviderLiveWorkspace,
        guard: &ProviderMutationGuard,
    ) -> Result<(), ProviderActivationError> {
        if current.provider.revision != guard.expected_provider_revision {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::ProviderConflict,
            ));
        }
        if current.revision != guard.expected_live_revision {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::LiveConflict,
            ));
        }
        Ok(())
    }

    fn successful_mutation(
        &self,
        backup_path: Option<String>,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        let (_, live) = self.load_activation_state()?;
        Ok(ProviderMutationOutcome {
            live,
            backup_path,
            rollback: ProviderRollbackOutcome::NotRequired,
        })
    }

    fn switch_failure(&self, error: RelaySwitchError) -> ProviderActivationError {
        let rollback = map_rollback(error.rollback_outcome());
        let kind = if rollback == ProviderRollbackOutcome::Failed {
            ProviderActivationErrorKind::RollbackFailed
        } else {
            ProviderActivationErrorKind::MutationFailed
        };
        ProviderActivationError::mutation(
            kind,
            rollback,
            error.backup_path().map(ToOwned::to_owned),
        )
    }

    fn non_switch_failure(
        &self,
        original_revision: &ProviderLiveRevision,
    ) -> ProviderActivationError {
        let rollback = read_live_files(self.environment().codex_home())
            .ok()
            .map(|(_, revision)| revision)
            .filter(|revision| revision == original_revision)
            .map_or(ProviderRollbackOutcome::Failed, |_| {
                ProviderRollbackOutcome::Verified
            });
        let kind = if rollback == ProviderRollbackOutcome::Failed {
            ProviderActivationErrorKind::RollbackFailed
        } else {
            ProviderActivationErrorKind::MutationFailed
        };
        ProviderActivationError::mutation(kind, rollback, None)
    }
}

impl<E: ProviderActivationEnvironment> ProviderActivationSource for ProviderService<E> {
    fn load_live_workspace(&self) -> Result<ProviderLiveWorkspace, ProviderActivationError> {
        let _lock = acquire_relay_live_read_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        self.load_activation_state().map(|(_, live)| live)
    }

    fn switch_provider(
        &self,
        request: SwitchProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        let _lock = acquire_relay_live_mutation_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        let (mut settings, current) = self.load_activation_state()?;
        self.validate_guard(&current, &request.guard)?;
        if !settings.relay_profiles_enabled {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::Disabled,
            ));
        }
        if !settings
            .relay_profiles
            .iter()
            .any(|profile| profile.id == request.target_profile_id)
        {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::ProfileNotFound,
            ));
        }
        let previous_active_relay_id = settings.active_relay_id.clone();
        select_target_profile(&mut settings, &request.target_profile_id)?;
        match switch_relay_profile_in_home(
            self.environment().settings_store(),
            self.environment().codex_home(),
            settings,
            &previous_active_relay_id,
        ) {
            Ok(result) => self.successful_mutation(result.backup_path),
            Err(error) => Err(self.switch_failure(error)),
        }
    }

    fn apply_active_provider(
        &self,
        request: ApplyActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        let _lock = acquire_relay_live_mutation_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        let (settings, current) = self.load_activation_state()?;
        self.validate_guard(&current, &request.guard)?;
        if !settings.relay_profiles_enabled {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::Disabled,
            ));
        }
        let active_id = settings.active_relay_id.clone();
        if !settings
            .relay_profiles
            .iter()
            .any(|profile| profile.id == active_id)
        {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::ProfileNotFound,
            ));
        }
        match switch_relay_profile_in_home(
            self.environment().settings_store(),
            self.environment().codex_home(),
            settings,
            &active_id,
        ) {
            Ok(result) => self.successful_mutation(result.backup_path),
            Err(error) => Err(self.switch_failure(error)),
        }
    }

    fn clear_live_provider(
        &self,
        request: ClearLiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        let _lock = acquire_relay_live_mutation_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        let (settings, current) = self.load_activation_state()?;
        self.validate_guard(&current, &request.guard)?;
        let profile = settings
            .relay_profiles
            .iter()
            .find(|profile| profile.id == settings.active_relay_id)
            .ok_or_else(|| {
                ProviderActivationError::new(ProviderActivationErrorKind::ProfileNotFound)
            })?;
        let auth_contents = (profile.relay_mode == RelayMode::Official
            && !profile.official_mix_api_key
            && !profile.auth_contents.trim().is_empty())
        .then_some(profile.auth_contents.as_str());
        match clear_relay_config_to_home_with_auth_and_computer_use_guard(
            self.environment().codex_home(),
            auth_contents,
            settings.computer_use_guard_enabled,
        ) {
            Ok(result) => self.successful_mutation(result.backup_path),
            Err(_) => Err(self.non_switch_failure(&current.revision)),
        }
    }

    fn backfill_active_provider(
        &self,
        request: BackfillActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        let _lock = acquire_relay_live_mutation_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        let (mut settings, current) = self.load_activation_state()?;
        self.validate_guard(&current, &request.guard)?;
        let active_id = settings.active_relay_id.clone();
        let profile = settings
            .relay_profiles
            .iter_mut()
            .find(|profile| profile.id == active_id)
            .ok_or_else(|| {
                ProviderActivationError::new(ProviderActivationErrorKind::ProfileNotFound)
            })?;
        if profile.relay_mode == RelayMode::Aggregate {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::UnsupportedAction,
            ));
        }
        backfill_relay_profile_from_home_with_common(
            self.environment().codex_home(),
            profile,
            &mut settings.relay_context_config_contents,
        )
        .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::MutationFailed))?;
        let expected_revision = request.guard.expected_provider_revision;
        let payload = json!({
            "relayProfiles": settings.relay_profiles,
            "relayContextConfigContents": settings.relay_context_config_contents,
        });
        let updated = self
            .environment()
            .settings_store()
            .update_if(payload, |fresh| {
                fresh.active_relay_id == active_id
                    && self
                        .workspace_from_settings(fresh)
                        .is_ok_and(|workspace| workspace.revision == expected_revision)
            })
            .map_err(|_| {
                ProviderActivationError::new(ProviderActivationErrorKind::MutationFailed)
            })?;
        if updated.is_none() {
            return Err(ProviderActivationError::new(
                ProviderActivationErrorKind::ProviderConflict,
            ));
        }
        self.successful_mutation(None)
    }

    fn save_live_file(
        &self,
        request: SaveLiveFile,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        validate_live_file(request.kind, &request.contents)?;
        let _lock = acquire_relay_live_mutation_lock(self.environment().codex_home())
            .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LockFailed))?;
        let (_, current) = self.load_activation_state()?;
        self.validate_guard(&current, &request.guard)?;
        let result = match request.kind {
            ProviderLiveFileKind::Config => {
                apply_relay_config_file_to_home(self.environment().codex_home(), &request.contents)
            }
            ProviderLiveFileKind::Auth => {
                apply_relay_auth_file_to_home(self.environment().codex_home(), &request.contents)
            }
        };
        match result {
            Ok(result) => self.successful_mutation(result.backup_path),
            Err(_) => Err(self.non_switch_failure(&current.revision)),
        }
    }
}

fn read_live_files(
    home: &Path,
) -> Result<(ProviderLiveFiles, ProviderLiveRevision), ProviderActivationError> {
    let config_path = home.join("config.toml");
    let auth_path = home.join("auth.json");
    let config = read_optional_bytes(&config_path)?;
    let auth = read_optional_bytes(&auth_path)?;
    let revision = live_revision(config.as_deref(), auth.as_deref());
    let config_contents = optional_utf8(config.as_deref())?;
    let auth_contents = optional_utf8(auth.as_deref())?;
    Ok((
        ProviderLiveFiles {
            config_path: config_path.to_string_lossy().to_string(),
            auth_path: auth_path.to_string_lossy().to_string(),
            config_exists: config.is_some(),
            auth_exists: auth.is_some(),
            config_contents,
            auth_contents,
        },
        revision,
    ))
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>, ProviderActivationError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ProviderActivationError::new(
            ProviderActivationErrorKind::LoadFailed,
        )),
    }
}

fn optional_utf8(bytes: Option<&[u8]>) -> Result<String, ProviderActivationError> {
    bytes
        .map(|bytes| String::from_utf8(bytes.to_vec()))
        .transpose()
        .map(|contents| contents.unwrap_or_default())
        .map_err(|_| ProviderActivationError::new(ProviderActivationErrorKind::LoadFailed))
}

fn live_revision(config: Option<&[u8]>, auth: Option<&[u8]>) -> ProviderLiveRevision {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-plus-provider-live-v1\0");
    hash_optional_bytes(&mut hasher, config);
    hash_optional_bytes(&mut hasher, auth);
    ProviderLiveRevision(format!("{:x}", hasher.finalize()))
}

fn hash_optional_bytes(hasher: &mut Sha256, bytes: Option<&[u8]>) {
    match bytes {
        Some(bytes) => {
            hasher.update([1]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        None => hasher.update([0]),
    }
}

fn select_target_profile(
    settings: &mut BackendSettings,
    target_profile_id: &str,
) -> Result<(), ProviderActivationError> {
    let profile = settings
        .relay_profiles
        .iter()
        .find(|profile| profile.id == target_profile_id)
        .cloned()
        .ok_or_else(|| {
            ProviderActivationError::new(ProviderActivationErrorKind::ProfileNotFound)
        })?;
    settings.active_relay_id = profile.id.clone();
    settings.active_aggregate_relay_id = if profile.relay_mode == RelayMode::Aggregate {
        profile.id.clone()
    } else {
        String::new()
    };
    settings.relay_base_url = if profile.relay_mode == RelayMode::Aggregate {
        codex_plus_core::protocol_proxy::local_responses_proxy_base_url(
            codex_plus_core::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        )
    } else {
        relay_profile_base_url(&profile)
    };
    settings.relay_api_key = relay_profile_api_key(&profile);
    Ok(())
}

fn validate_live_file(
    kind: ProviderLiveFileKind,
    contents: &str,
) -> Result<(), ProviderActivationError> {
    let contents = contents.trim_start_matches('\u{feff}');
    if contents.trim().is_empty() {
        return Err(ProviderActivationError::new(
            ProviderActivationErrorKind::InvalidLiveFile,
        ));
    }
    let valid = match kind {
        ProviderLiveFileKind::Config => contents.parse::<toml::Table>().is_ok(),
        ProviderLiveFileKind::Auth => serde_json::from_str::<serde_json::Value>(contents).is_ok(),
    };
    if valid {
        Ok(())
    } else {
        Err(ProviderActivationError::new(
            ProviderActivationErrorKind::InvalidLiveFile,
        ))
    }
}

fn map_rollback(outcome: RelayRollbackOutcome) -> ProviderRollbackOutcome {
    match outcome {
        RelayRollbackOutcome::NotRequired => ProviderRollbackOutcome::NotRequired,
        RelayRollbackOutcome::Verified => ProviderRollbackOutcome::Verified,
        RelayRollbackOutcome::Failed => ProviderRollbackOutcome::Failed,
    }
}
