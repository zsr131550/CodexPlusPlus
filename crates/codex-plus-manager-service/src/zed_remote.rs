use std::fmt;
use std::path::PathBuf;

use codex_plus_core::settings::BackendSettings;
use codex_plus_core::zed_remote::{
    self, SshTarget, ZedAvailability, ZedLaunchPlan, ZedOpenStrategy,
    ZedRemoteError as CoreZedRemoteError, ZedRemoteProject, ZedRemoteProjectSource,
    ZedRemoteRegistryRevision, ZedRemoteRegistryStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq)]
pub struct ZedSettingsRevision([u8; 32]);

impl ZedSettingsRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for ZedSettingsRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ZedSettingsRevision([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedProjectRevision([u8; 32]);

impl ZedProjectRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for ZedProjectRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ZedProjectRevision([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedRemoteProjectSummary {
    pub id: String,
    pub revision: ZedProjectRevision,
    pub label: String,
    pub host_id: String,
    pub ssh: SshTarget,
    pub remote_path: String,
    pub url: String,
    pub source: ZedRemoteProjectSource,
    pub last_opened_at_ms: Option<i64>,
    pub is_current: bool,
}

impl fmt::Debug for ZedRemoteProjectSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteProjectSummary")
            .field("revision", &self.revision)
            .field("source", &self.source)
            .field("is_current", &self.is_current)
            .field("port_present", &self.ssh.port.is_some())
            .field("last_opened_present", &self.last_opened_at_ms.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedRemoteWorkspace {
    pub settings_revision: ZedSettingsRevision,
    pub registry_revision: ZedRemoteRegistryRevision,
    pub default_strategy: ZedOpenStrategy,
    pub registry_enabled: bool,
    pub availability: ZedAvailability,
    pub projects: Vec<ZedRemoteProjectSummary>,
}

impl fmt::Debug for ZedRemoteWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteWorkspace")
            .field("settings_revision", &self.settings_revision)
            .field("registry_revision", &self.registry_revision)
            .field("default_strategy", &self.default_strategy)
            .field("registry_enabled", &self.registry_enabled)
            .field("availability", &self.availability)
            .field("project_count", &self.projects.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveZedPreferences {
    pub expected_revision: ZedSettingsRevision,
    pub default_strategy: ZedOpenStrategy,
    pub registry_enabled: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OpenZedRemoteProject {
    pub project_id: String,
    pub confirmed_project_id: String,
    pub expected_project_revision: ZedProjectRevision,
    pub expected_registry_revision: ZedRemoteRegistryRevision,
    pub strategy: ZedOpenStrategy,
    pub confirmed_strategy: ZedOpenStrategy,
    pub remember: bool,
    pub confirmed_remember: bool,
}

impl fmt::Debug for OpenZedRemoteProject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenZedRemoteProject")
            .field("expected_project_revision", &self.expected_project_revision)
            .field(
                "expected_registry_revision",
                &self.expected_registry_revision,
            )
            .field("strategy", &self.strategy)
            .field("confirmed_strategy", &self.confirmed_strategy)
            .field("remember", &self.remember)
            .field("confirmed_remember", &self.confirmed_remember)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ForgetZedRemoteProject {
    pub expected_registry_revision: ZedRemoteRegistryRevision,
    pub project_id: String,
    pub confirmed_project_id: String,
}

impl fmt::Debug for ForgetZedRemoteProject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForgetZedRemoteProject")
            .field(
                "expected_registry_revision",
                &self.expected_registry_revision,
            )
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZedRememberOutcome {
    NotRequested,
    Remembered,
    Failed(ZedRemoteErrorKind),
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedRemoteOpenOutcome {
    pub workspace: ZedRemoteWorkspace,
    pub strategy: ZedOpenStrategy,
    pub url: String,
    pub remember: ZedRememberOutcome,
}

impl fmt::Debug for ZedRemoteOpenOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteOpenOutcome")
            .field("strategy", &self.strategy)
            .field("remember", &self.remember)
            .field("workspace_project_count", &self.workspace.projects.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZedRemoteErrorKind {
    StateReadFailed,
    StateParseFailed,
    RegistryReadFailed,
    RegistryParseFailed,
    SettingsReadFailed,
    SettingsConflict,
    RegistryConflict,
    InvalidProject,
    ProjectConflict,
    ZedUnavailable,
    LaunchFailed,
    RegistryWriteFailed,
    WorkerStopped,
}

pub struct ZedRemoteError {
    kind: ZedRemoteErrorKind,
    compatibility_detail: Option<String>,
}

impl ZedRemoteError {
    pub fn new(kind: ZedRemoteErrorKind) -> Self {
        Self {
            kind,
            compatibility_detail: None,
        }
    }

    pub fn kind(&self) -> ZedRemoteErrorKind {
        self.kind
    }

    pub fn compatibility_detail(&self) -> Option<&str> {
        self.compatibility_detail.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ZedRemoteErrorKind::StateReadFailed => "Zed remote state read failed",
            ZedRemoteErrorKind::StateParseFailed => "Zed remote state parse failed",
            ZedRemoteErrorKind::RegistryReadFailed => "Zed remote registry read failed",
            ZedRemoteErrorKind::RegistryParseFailed => "Zed remote registry parse failed",
            ZedRemoteErrorKind::SettingsReadFailed => "Zed remote settings read failed",
            ZedRemoteErrorKind::SettingsConflict => "Zed remote settings changed on disk",
            ZedRemoteErrorKind::RegistryConflict => "Zed remote registry changed on disk",
            ZedRemoteErrorKind::InvalidProject => "Zed remote project is invalid",
            ZedRemoteErrorKind::ProjectConflict => "Zed remote project changed",
            ZedRemoteErrorKind::ZedUnavailable => "Zed is unavailable",
            ZedRemoteErrorKind::LaunchFailed => "Zed launch failed",
            ZedRemoteErrorKind::RegistryWriteFailed => "Zed remote registry write failed",
            ZedRemoteErrorKind::WorkerStopped => "Zed remote worker stopped",
        }
    }
}

impl fmt::Debug for ZedRemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ZedRemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ZedRemoteError {}

pub trait ZedRemoteEnvironment: Send + Sync + 'static {
    fn load_zed_settings(&self) -> anyhow::Result<BackendSettings>;

    fn update_zed_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool;

    fn load_zed_global_state(&self) -> Result<Option<Value>, CoreZedRemoteError>;
    fn zed_request_context(&self) -> Value;
    fn zed_registry_store(&self) -> ZedRemoteRegistryStore;
    fn zed_sqlite_paths(&self) -> Vec<PathBuf>;
    fn zed_availability(&self) -> ZedAvailability;
    fn launch_zed_remote(&self, plan: &ZedLaunchPlan) -> Result<(), CoreZedRemoteError>;
}

pub trait ZedLaunchExecutor: Send + Sync + 'static {
    fn launch(&self, plan: &ZedLaunchPlan) -> Result<(), CoreZedRemoteError>;

    fn availability_override(&self) -> Option<ZedAvailability> {
        None
    }
}

pub trait ZedRemoteSource: Send + Sync + 'static {
    fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError>;
    fn save_preferences(
        &self,
        request: SaveZedPreferences,
    ) -> Result<ZedRemoteWorkspace, ZedRemoteError>;
    fn open_project(
        &self,
        request: OpenZedRemoteProject,
    ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError>;
    fn forget_project(
        &self,
        request: ForgetZedRemoteProject,
    ) -> Result<ZedRemoteWorkspace, ZedRemoteError>;
}

#[derive(Clone)]
pub struct ZedRemoteService<E> {
    environment: E,
}

impl<E> ZedRemoteService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: ZedRemoteEnvironment> ZedRemoteService<E> {
    fn load_workspace_inner(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        let settings = self
            .environment
            .load_zed_settings()
            .map_err(|_| ZedRemoteError::new(ZedRemoteErrorKind::SettingsReadFailed))?;
        let snapshot = self
            .environment
            .zed_registry_store()
            .inspect()
            .map_err(map_core_error)?;
        let state = self
            .environment
            .load_zed_global_state()
            .map_err(map_core_error)?;
        let request_context = self.environment.zed_request_context();
        let sqlite_paths = self.environment.zed_sqlite_paths();
        let mut projects = zed_remote::list_zed_remote_projects_from_sources_with_sqlite_paths(
            state.as_ref(),
            &request_context,
            &snapshot.projects,
            &sqlite_paths,
        )
        .map_err(map_core_error)?;
        projects.sort_by_key(|project| source_priority(project.source));
        Ok(ZedRemoteWorkspace {
            settings_revision: zed_settings_revision(&settings),
            registry_revision: snapshot.revision,
            default_strategy: settings.zed_remote_open_strategy,
            registry_enabled: settings.zed_remote_project_registry_enabled,
            availability: self.environment.zed_availability(),
            projects: projects.into_iter().map(project_summary).collect(),
        })
    }
}

impl<E: ZedRemoteEnvironment> ZedRemoteSource for ZedRemoteService<E> {
    fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        self.load_workspace_inner()
    }

    fn save_preferences(
        &self,
        request: SaveZedPreferences,
    ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        let expected = request.expected_revision;
        let updated = self
            .environment
            .update_zed_settings_if(
                json!({
                    "zedRemoteOpenStrategy": request.default_strategy,
                    "zedRemoteProjectRegistryEnabled": request.registry_enabled,
                }),
                move |current| zed_settings_revision(current) == expected,
            )
            .map_err(|_| ZedRemoteError::new(ZedRemoteErrorKind::SettingsReadFailed))?;
        if updated.is_none() {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::SettingsConflict));
        }
        self.load_workspace_inner()
    }

    fn open_project(
        &self,
        request: OpenZedRemoteProject,
    ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError> {
        if request.project_id != request.confirmed_project_id
            || request.strategy != request.confirmed_strategy
            || request.remember != request.confirmed_remember
        {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::InvalidProject));
        }

        let workspace = self.load_workspace_inner()?;
        let project = workspace
            .projects
            .iter()
            .find(|project| project.id == request.project_id)
            .ok_or_else(|| ZedRemoteError::new(ZedRemoteErrorKind::InvalidProject))?;
        if project.revision != request.expected_project_revision {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::ProjectConflict));
        }

        let remember_requested = request.remember && workspace.registry_enabled;
        if remember_requested && workspace.registry_revision != request.expected_registry_revision {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::RegistryConflict));
        }
        if !workspace.availability.platform_supported
            || (!workspace.availability.cli_found && !workspace.availability.app_found)
        {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::ZedUnavailable));
        }

        let plan = zed_remote::prepare_zed_remote_launch(
            &project.ssh,
            &project.remote_path,
            request.strategy,
        )
        .map_err(|_| ZedRemoteError::new(ZedRemoteErrorKind::InvalidProject))?;
        let url = plan.url().to_string();
        self.environment
            .launch_zed_remote(&plan)
            .map_err(map_core_error)?;

        let remember = if !remember_requested {
            ZedRememberOutcome::NotRequested
        } else {
            let recent = summary_to_recent_project(project);
            match self
                .environment
                .zed_registry_store()
                .remember_if_revision(&request.expected_registry_revision, recent)
            {
                Ok(_) => ZedRememberOutcome::Remembered,
                Err(error) => ZedRememberOutcome::Failed(map_core_error(error).kind()),
            }
        };
        let fresh_workspace = self.load_workspace_inner()?;
        Ok(ZedRemoteOpenOutcome {
            workspace: fresh_workspace,
            strategy: request.strategy,
            url,
            remember,
        })
    }

    fn forget_project(
        &self,
        request: ForgetZedRemoteProject,
    ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        if request.project_id != request.confirmed_project_id {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::InvalidProject));
        }
        let workspace = self.load_workspace_inner()?;
        let project = workspace
            .projects
            .iter()
            .find(|project| {
                project.id == request.project_id && project.source == ZedRemoteProjectSource::Recent
            })
            .ok_or_else(|| ZedRemoteError::new(ZedRemoteErrorKind::InvalidProject))?;
        let _ = project;
        if workspace.registry_revision != request.expected_registry_revision {
            return Err(ZedRemoteError::new(ZedRemoteErrorKind::RegistryConflict));
        }
        self.environment
            .zed_registry_store()
            .forget_if_revision(&request.expected_registry_revision, &request.project_id)
            .map_err(map_core_error)?;
        self.load_workspace_inner()
    }
}

pub(crate) fn zed_settings_revision(settings: &BackendSettings) -> ZedSettingsRevision {
    let mut hasher = Sha256::new();
    let strategy = serde_json::to_vec(&settings.zed_remote_open_strategy).unwrap_or_default();
    hash_field(&mut hasher, &strategy);
    hash_field(
        &mut hasher,
        &[u8::from(settings.zed_remote_project_registry_enabled)],
    );
    ZedSettingsRevision(hasher.finalize().into())
}

fn project_summary(project: ZedRemoteProject) -> ZedRemoteProjectSummary {
    let revision = zed_project_revision(&project);
    ZedRemoteProjectSummary {
        id: project.id,
        revision,
        label: project.label,
        host_id: project.host_id,
        ssh: project.ssh,
        remote_path: project.path,
        url: project.url,
        source: project.source,
        last_opened_at_ms: project.last_opened_at_ms,
        is_current: project.is_current,
    }
}

fn summary_to_recent_project(project: &ZedRemoteProjectSummary) -> ZedRemoteProject {
    ZedRemoteProject {
        id: project.id.clone(),
        label: project.label.clone(),
        host_id: project.host_id.clone(),
        ssh: project.ssh.clone(),
        path: project.remote_path.clone(),
        url: project.url.clone(),
        source: ZedRemoteProjectSource::Recent,
        last_opened_at_ms: Some(now_ms()),
        is_current: false,
    }
}

fn zed_project_revision(project: &ZedRemoteProject) -> ZedProjectRevision {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, project.ssh.user.trim().as_bytes());
    hash_field(
        &mut hasher,
        project.ssh.host.trim().to_ascii_lowercase().as_bytes(),
    );
    hash_field(
        &mut hasher,
        &project.ssh.port.map(u16::to_le_bytes).unwrap_or_default(),
    );
    hash_field(&mut hasher, project.path.trim().as_bytes());
    ZedProjectRevision(hasher.finalize().into())
}

fn source_priority(source: ZedRemoteProjectSource) -> u8 {
    match source {
        ZedRemoteProjectSource::CurrentThread => 0,
        ZedRemoteProjectSource::CodexRemoteProject => 1,
        ZedRemoteProjectSource::ThreadWorkspaceHint => 2,
        ZedRemoteProjectSource::SqliteThreadCwd => 3,
        ZedRemoteProjectSource::Recent => 4,
    }
}

fn map_core_error(error: CoreZedRemoteError) -> ZedRemoteError {
    let kind = match error {
        CoreZedRemoteError::StateRead(_) => ZedRemoteErrorKind::StateReadFailed,
        CoreZedRemoteError::StateParse(_) => ZedRemoteErrorKind::StateParseFailed,
        CoreZedRemoteError::RegistryRead(_) => ZedRemoteErrorKind::RegistryReadFailed,
        CoreZedRemoteError::RegistryParse(_) => ZedRemoteErrorKind::RegistryParseFailed,
        CoreZedRemoteError::RegistryConflict => ZedRemoteErrorKind::RegistryConflict,
        CoreZedRemoteError::RegistryWrite(_)
        | CoreZedRemoteError::RegistryLock(_)
        | CoreZedRemoteError::RegistryCommit(_) => ZedRemoteErrorKind::RegistryWriteFailed,
        CoreZedRemoteError::Launch(_) => ZedRemoteErrorKind::LaunchFailed,
        CoreZedRemoteError::Validation(_) => ZedRemoteErrorKind::InvalidProject,
    };
    ZedRemoteError::new(kind)
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}
