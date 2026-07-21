use std::fmt;
use std::sync::Arc;

use codex_plus_core::zed_remote::{ZedAvailability, ZedOpenStrategy, ZedRemoteProjectSource};
use codex_plus_manager_service::{
    ForgetZedRemoteProject, OpenZedRemoteProject, SaveZedPreferences, ZedProjectRevision,
    ZedRemoteErrorKind, ZedRemoteOpenOutcome, ZedRemoteWorkspace,
};

use super::provider::OperationPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZedRemoteLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZedRemoteFailureKind {
    Service(ZedRemoteErrorKind),
    WorkerStopped,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedOpenConfirmation {
    pub project_id: String,
    pub label: String,
    pub authority: String,
    pub remote_path: String,
    pub expected_project_revision: ZedProjectRevision,
    pub expected_registry_revision: codex_plus_core::zed_remote::ZedRemoteRegistryRevision,
    pub strategy: ZedOpenStrategy,
    pub remember: bool,
}

impl fmt::Debug for ZedOpenConfirmation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedOpenConfirmation")
            .field("expected_project_revision", &self.expected_project_revision)
            .field(
                "expected_registry_revision",
                &self.expected_registry_revision,
            )
            .field("strategy", &self.strategy)
            .field("remember", &self.remember)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ZedForgetConfirmation {
    pub project_id: String,
    pub label: String,
    pub authority: String,
    pub remote_path: String,
    pub expected_registry_revision: codex_plus_core::zed_remote::ZedRemoteRegistryRevision,
}

impl fmt::Debug for ZedForgetConfirmation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedForgetConfirmation")
            .field(
                "expected_registry_revision",
                &self.expected_registry_revision,
            )
            .finish_non_exhaustive()
    }
}

/// Native UI state for the Zed remote workflow.
///
/// The state stores only opaque revisions and user-visible labels.  It never
/// formats a URL, SSH target, or path in its diagnostic representation.
pub struct ZedRemoteViewState {
    pub load_phase: ZedRemoteLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<ZedRemoteFailureKind>,
    pub workspace: Option<Arc<ZedRemoteWorkspace>>,

    pub search_query: String,
    pub recent_page: usize,
    pub discovered_page: usize,
    pub draft_strategy: ZedOpenStrategy,
    pub draft_registry_enabled: bool,
    pub preferences_dirty: bool,
    pub save_phase: OperationPhase,
    pub current_save_request_id: u64,
    pub save_error: Option<ZedRemoteFailureKind>,

    pub open_phase: OperationPhase,
    pub current_open_request_id: u64,
    pub open_error: Option<ZedRemoteFailureKind>,
    pub pending_open: Option<ZedOpenConfirmation>,
    pub open_outcome: Option<Arc<ZedRemoteOpenOutcome>>,

    pub forget_phase: OperationPhase,
    pub current_forget_request_id: u64,
    pub forget_error: Option<ZedRemoteFailureKind>,
    pub pending_forget: Option<ZedForgetConfirmation>,
    pub needs_refresh_after_conflict: bool,
}

impl Default for ZedRemoteViewState {
    fn default() -> Self {
        Self {
            load_phase: ZedRemoteLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            workspace: None,
            search_query: String::new(),
            recent_page: 0,
            discovered_page: 0,
            draft_strategy: ZedOpenStrategy::default(),
            draft_registry_enabled: true,
            preferences_dirty: false,
            save_phase: OperationPhase::Idle,
            current_save_request_id: 0,
            save_error: None,
            open_phase: OperationPhase::Idle,
            current_open_request_id: 0,
            open_error: None,
            pending_open: None,
            open_outcome: None,
            forget_phase: OperationPhase::Idle,
            current_forget_request_id: 0,
            forget_error: None,
            pending_forget: None,
            needs_refresh_after_conflict: false,
        }
    }
}

impl fmt::Debug for ZedRemoteViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteViewState")
            .field("load_phase", &self.load_phase)
            .field("has_workspace", &self.workspace.is_some())
            .field("search_present", &!self.search_query.is_empty())
            .field("recent_page", &self.recent_page)
            .field("discovered_page", &self.discovered_page)
            .field("draft_strategy", &self.draft_strategy)
            .field("draft_registry_enabled", &self.draft_registry_enabled)
            .field("preferences_dirty", &self.preferences_dirty)
            .field("save_phase", &self.save_phase)
            .field("open_phase", &self.open_phase)
            .field("has_pending_open", &self.pending_open.is_some())
            .field("has_open_outcome", &self.open_outcome.is_some())
            .field("forget_phase", &self.forget_phase)
            .field("has_pending_forget", &self.pending_forget.is_some())
            .field(
                "needs_refresh_after_conflict",
                &self.needs_refresh_after_conflict,
            )
            .finish()
    }
}

impl ZedRemoteViewState {
    pub fn begin_load(&mut self) -> u64 {
        self.current_load_request_id = next_id(self.current_load_request_id, "zed remote load");
        self.load_phase = if self.workspace.is_some() {
            ZedRemoteLoadPhase::Refreshing
        } else {
            ZedRemoteLoadPhase::Loading
        };
        self.load_error = None;
        self.current_load_request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                if !self.preferences_dirty {
                    self.draft_strategy = workspace.default_strategy;
                    self.draft_registry_enabled = workspace.registry_enabled;
                }
                self.workspace = Some(workspace);
                self.load_phase = ZedRemoteLoadPhase::Ready;
                self.load_error = None;
            }
            Err(error) => {
                self.load_phase = ZedRemoteLoadPhase::Error;
                self.load_error = Some(error);
            }
        }
        true
    }

    pub fn set_search_query(&mut self, query: impl Into<String>) {
        self.search_query = query.into();
        self.recent_page = 0;
        self.discovered_page = 0;
    }

    pub fn set_recent_page(&mut self, page: usize) {
        self.recent_page = page.min(
            self.page_count(ZedRemoteProjectSource::Recent)
                .saturating_sub(1),
        );
    }

    pub fn set_discovered_page(&mut self, page: usize) {
        self.discovered_page = page.min(
            self.page_count(ZedRemoteProjectSource::SqliteThreadCwd)
                .saturating_sub(1),
        );
    }

    pub fn set_strategy(&mut self, strategy: ZedOpenStrategy) {
        self.draft_strategy = strategy;
        self.update_preferences_dirty();
    }

    pub fn set_registry_enabled(&mut self, enabled: bool) {
        self.draft_registry_enabled = enabled;
        self.update_preferences_dirty();
    }

    pub fn begin_save_preferences(&mut self) -> Option<(u64, SaveZedPreferences)> {
        if self.mutation_running() || !self.preferences_dirty {
            return None;
        }
        let workspace = self.workspace.as_ref()?;
        self.current_save_request_id = next_id(self.current_save_request_id, "zed remote save");
        self.save_phase = OperationPhase::Running;
        self.save_error = None;
        Some((
            self.current_save_request_id,
            SaveZedPreferences {
                expected_revision: workspace.settings_revision.clone(),
                default_strategy: self.draft_strategy,
                registry_enabled: self.draft_registry_enabled,
            },
        ))
    }

    pub fn apply_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteFailureKind>,
    ) -> bool {
        if request_id != self.current_save_request_id || self.save_phase != OperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.draft_strategy = workspace.default_strategy;
                self.draft_registry_enabled = workspace.registry_enabled;
                self.preferences_dirty = false;
                self.workspace = Some(workspace);
                self.save_phase = OperationPhase::Ready;
                self.save_error = None;
            }
            Err(error) => {
                self.save_phase = OperationPhase::Error;
                self.save_error = Some(error);
                if matches!(
                    error,
                    ZedRemoteFailureKind::Service(ZedRemoteErrorKind::SettingsConflict)
                ) {
                    self.needs_refresh_after_conflict = true;
                }
            }
        }
        true
    }

    pub fn request_open(
        &mut self,
        project_id: impl Into<String>,
        strategy: ZedOpenStrategy,
        remember: bool,
    ) -> bool {
        if self.mutation_running() || self.pending_forget.is_some() {
            return false;
        }
        let project_id = project_id.into();
        let Some(workspace) = self.workspace.as_ref() else {
            return false;
        };
        let Some(project) = workspace
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return false;
        };
        self.pending_open = Some(ZedOpenConfirmation {
            project_id,
            label: project.label.clone(),
            authority: ssh_authority(&project.ssh),
            remote_path: project.remote_path.clone(),
            expected_project_revision: project.revision.clone(),
            expected_registry_revision: workspace.registry_revision.clone(),
            strategy,
            remember,
        });
        true
    }

    pub fn cancel_open(&mut self) -> bool {
        if self.open_phase == OperationPhase::Running {
            return false;
        }
        self.pending_open.take().is_some()
    }

    pub fn set_open_strategy(&mut self, strategy: ZedOpenStrategy) -> bool {
        if self.mutation_running() {
            return false;
        }
        let Some(confirmation) = self.pending_open.as_mut() else {
            return false;
        };
        confirmation.strategy = strategy;
        true
    }

    pub fn set_open_remember(&mut self, remember: bool) -> bool {
        if self.mutation_running() {
            return false;
        }
        let Some(confirmation) = self.pending_open.as_mut() else {
            return false;
        };
        confirmation.remember = remember;
        true
    }

    pub fn begin_open(&mut self) -> Option<(u64, OpenZedRemoteProject)> {
        if self.open_phase == OperationPhase::Running {
            return None;
        }
        if self.mutation_running() {
            return None;
        }
        let confirmation = self.pending_open.as_ref()?.clone();
        self.current_open_request_id = next_id(self.current_open_request_id, "zed remote open");
        self.open_phase = OperationPhase::Running;
        self.open_error = None;
        self.open_outcome = None;
        let request = OpenZedRemoteProject {
            project_id: confirmation.project_id.clone(),
            confirmed_project_id: confirmation.project_id,
            expected_project_revision: confirmation.expected_project_revision,
            expected_registry_revision: confirmation.expected_registry_revision,
            strategy: confirmation.strategy,
            confirmed_strategy: confirmation.strategy,
            remember: confirmation.remember,
            confirmed_remember: confirmation.remember,
        };
        Some((self.current_open_request_id, request))
    }

    pub fn apply_open_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ZedRemoteOpenOutcome>, ZedRemoteFailureKind>,
    ) -> bool {
        if request_id != self.current_open_request_id || self.open_phase != OperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                let workspace = Arc::new(outcome.workspace.clone());
                if !self.preferences_dirty {
                    self.draft_strategy = workspace.default_strategy;
                    self.draft_registry_enabled = workspace.registry_enabled;
                }
                self.workspace = Some(workspace);
                self.open_outcome = Some(outcome);
                self.open_phase = OperationPhase::Ready;
                self.open_error = None;
                self.pending_open = None;
            }
            Err(error) => {
                self.open_phase = OperationPhase::Error;
                self.open_error = Some(error);
                if matches!(
                    error,
                    ZedRemoteFailureKind::Service(
                        ZedRemoteErrorKind::RegistryConflict | ZedRemoteErrorKind::ProjectConflict
                    )
                ) {
                    self.needs_refresh_after_conflict = true;
                }
            }
        }
        true
    }

    pub fn request_forget(&mut self, project_id: impl Into<String>) -> bool {
        if self.mutation_running() || self.pending_open.is_some() {
            return false;
        }
        let project_id = project_id.into();
        let Some(workspace) = self.workspace.as_ref() else {
            return false;
        };
        let Some(project) = workspace.projects.iter().find(|project| {
            project.id == project_id && project.source == ZedRemoteProjectSource::Recent
        }) else {
            return false;
        };
        self.pending_forget = Some(ZedForgetConfirmation {
            project_id,
            label: project.label.clone(),
            authority: ssh_authority(&project.ssh),
            remote_path: project.remote_path.clone(),
            expected_registry_revision: workspace.registry_revision.clone(),
        });
        true
    }

    pub fn cancel_forget(&mut self) -> bool {
        if self.forget_phase == OperationPhase::Running {
            return false;
        }
        self.pending_forget.take().is_some()
    }

    pub fn begin_forget(&mut self) -> Option<(u64, ForgetZedRemoteProject)> {
        if self.forget_phase == OperationPhase::Running {
            return None;
        }
        if self.mutation_running() {
            return None;
        }
        let confirmation = self.pending_forget.as_ref()?.clone();
        self.current_forget_request_id =
            next_id(self.current_forget_request_id, "zed remote forget");
        self.forget_phase = OperationPhase::Running;
        self.forget_error = None;
        Some((
            self.current_forget_request_id,
            ForgetZedRemoteProject {
                expected_registry_revision: confirmation.expected_registry_revision,
                project_id: confirmation.project_id.clone(),
                confirmed_project_id: confirmation.project_id,
            },
        ))
    }

    pub fn apply_forget_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteFailureKind>,
    ) -> bool {
        if request_id != self.current_forget_request_id
            || self.forget_phase != OperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.workspace = Some(workspace);
                self.forget_phase = OperationPhase::Ready;
                self.forget_error = None;
                self.pending_forget = None;
            }
            Err(error) => {
                self.forget_phase = OperationPhase::Error;
                self.forget_error = Some(error);
                if matches!(
                    error,
                    ZedRemoteFailureKind::Service(ZedRemoteErrorKind::RegistryConflict)
                ) {
                    self.needs_refresh_after_conflict = true;
                }
            }
        }
        true
    }

    pub fn availability(&self) -> Option<ZedAvailability> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.availability)
    }

    pub fn filtered_project_ids(&self) -> Vec<String> {
        let needle = self.search_query.trim().to_ascii_lowercase();
        self.workspace
            .as_ref()
            .map(|workspace| {
                workspace
                    .projects
                    .iter()
                    .filter(|project| {
                        needle.is_empty()
                            || project.label.to_ascii_lowercase().contains(&needle)
                            || project.host_id.to_ascii_lowercase().contains(&needle)
                            || project.remote_path.to_ascii_lowercase().contains(&needle)
                    })
                    .map(|project| project.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn mutation_running(&self) -> bool {
        self.save_phase == OperationPhase::Running
            || self.open_phase == OperationPhase::Running
            || self.forget_phase == OperationPhase::Running
    }

    pub fn take_refresh_after_conflict(&mut self) -> bool {
        let refresh = self.needs_refresh_after_conflict;
        self.needs_refresh_after_conflict = false;
        refresh
    }

    pub fn page_count(&self, source: ZedRemoteProjectSource) -> usize {
        let count = self
            .workspace
            .as_ref()
            .map(|workspace| {
                workspace
                    .projects
                    .iter()
                    .filter(|project| {
                        source_in_group(project.source, source) && matches_search(self, project)
                    })
                    .count()
            })
            .unwrap_or_default();
        count.div_ceil(ZED_PROJECT_PAGE_SIZE)
    }

    pub fn visible_project_ids(&self, source: ZedRemoteProjectSource) -> Vec<String> {
        let page = match source {
            ZedRemoteProjectSource::Recent => self.recent_page,
            ZedRemoteProjectSource::CurrentThread => 0,
            _ => self.discovered_page,
        };
        self.workspace
            .as_ref()
            .map(|workspace| {
                workspace
                    .projects
                    .iter()
                    .filter(|project| {
                        source_in_group(project.source, source) && matches_search(self, project)
                    })
                    .skip(page * ZED_PROJECT_PAGE_SIZE)
                    .take(ZED_PROJECT_PAGE_SIZE)
                    .map(|project| project.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn update_preferences_dirty(&mut self) {
        self.preferences_dirty = self.workspace.as_ref().is_some_and(|workspace| {
            self.draft_strategy != workspace.default_strategy
                || self.draft_registry_enabled != workspace.registry_enabled
        });
    }
}

pub const ZED_PROJECT_PAGE_SIZE: usize = 25;

fn matches_search(
    state: &ZedRemoteViewState,
    project: &codex_plus_manager_service::ZedRemoteProjectSummary,
) -> bool {
    let needle = state.search_query.trim().to_ascii_lowercase();
    needle.is_empty()
        || project.label.to_ascii_lowercase().contains(&needle)
        || project.host_id.to_ascii_lowercase().contains(&needle)
        || project.remote_path.to_ascii_lowercase().contains(&needle)
}

fn source_in_group(actual: ZedRemoteProjectSource, group: ZedRemoteProjectSource) -> bool {
    match group {
        ZedRemoteProjectSource::CurrentThread | ZedRemoteProjectSource::Recent => actual == group,
        _ => !matches!(
            actual,
            ZedRemoteProjectSource::CurrentThread | ZedRemoteProjectSource::Recent
        ),
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}

fn ssh_authority(ssh: &codex_plus_core::zed_remote::SshTarget) -> String {
    let host = if ssh.host.contains(':') && !ssh.host.starts_with('[') {
        format!("[{}]", ssh.host)
    } else {
        ssh.host.clone()
    };
    match ssh.port {
        Some(port) => format!("{}@{}:{}", ssh.user, host, port),
        None => format!("{}@{}", ssh.user, host),
    }
}

#[cfg(test)]
mod tests {
    use codex_plus_core::zed_remote::{
        SshTarget, ZedRemoteProjectSource, ZedRemoteRegistryRevision,
    };
    use codex_plus_manager_service::{ZedRemoteProjectSummary, ZedSettingsRevision};

    use super::*;

    fn revision(byte: u8) -> ZedRemoteRegistryRevision {
        ZedRemoteRegistryRevision::from_digest([byte; 32])
    }

    fn workspace() -> ZedRemoteWorkspace {
        ZedRemoteWorkspace {
            settings_revision: ZedSettingsRevision::from_digest([1; 32]),
            registry_revision: revision(2),
            default_strategy: ZedOpenStrategy::ReuseWindow,
            registry_enabled: true,
            availability: ZedAvailability {
                platform_supported: true,
                cli_found: true,
                app_found: false,
            },
            projects: vec![ZedRemoteProjectSummary {
                id: "project-1".to_owned(),
                revision: ZedProjectRevision::from_digest([3; 32]),
                label: "alpha".to_owned(),
                host_id: "host".to_owned(),
                ssh: SshTarget {
                    user: "dev".to_owned(),
                    host: "example.test".to_owned(),
                    port: Some(22),
                },
                remote_path: "/work".to_owned(),
                url: "zed://ssh/example.test/work".to_owned(),
                source: ZedRemoteProjectSource::Recent,
                last_opened_at_ms: None,
                is_current: false,
            }],
        }
    }

    #[test]
    fn load_ignores_stale_response_and_preserves_last_good_workspace() {
        let mut state = ZedRemoteViewState::default();
        let first = state.begin_load();
        assert!(state.apply_load_response(first, Ok(Arc::new(workspace()))));
        let second = state.begin_load();
        assert!(!state.apply_load_response(first, Err(ZedRemoteFailureKind::WorkerStopped)));
        assert_eq!(state.load_phase, ZedRemoteLoadPhase::Refreshing);
        assert!(state.apply_load_response(second, Err(ZedRemoteFailureKind::WorkerStopped)));
        assert!(state.workspace.is_some());
        assert_eq!(state.load_phase, ZedRemoteLoadPhase::Error);
    }

    #[test]
    fn preferences_are_dirty_only_against_zed_settings_and_request_carries_revision() {
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(workspace()))));
        state.set_strategy(ZedOpenStrategy::NewWindow);
        assert!(state.preferences_dirty);
        let (request_id, request) = state.begin_save_preferences().unwrap();
        assert_eq!(request.default_strategy, ZedOpenStrategy::NewWindow);
        assert_eq!(
            request.expected_revision,
            state.workspace.as_ref().unwrap().settings_revision
        );
        assert!(state.apply_save_response(request_id, Ok(Arc::new(workspace()))));
        assert!(!state.preferences_dirty);
    }

    #[test]
    fn open_and_forget_requests_snapshot_exact_confirmations() {
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(workspace()))));
        assert!(state.request_open("project-1", ZedOpenStrategy::NewWindow, true));
        let (open_id, open) = state.begin_open().unwrap();
        assert_eq!(open.project_id, open.confirmed_project_id);
        assert_eq!(open.strategy, open.confirmed_strategy);
        assert_eq!(open.remember, open.confirmed_remember);
        assert!(state.apply_open_response(
            open_id,
            Err(ZedRemoteFailureKind::Service(
                ZedRemoteErrorKind::LaunchFailed
            ))
        ));
        assert!(state.cancel_open());
        assert!(state.request_forget("project-1"));
        let (_, forget) = state.begin_forget().unwrap();
        assert_eq!(forget.project_id, forget.confirmed_project_id);
    }

    #[test]
    fn debug_does_not_include_search_or_paths() {
        let mut state = ZedRemoteViewState::default();
        state.set_search_query("/secret/project");
        let debug = format!("{state:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("project-1"));
    }

    #[test]
    fn launch_confirmation_freezes_metadata_revision_strategy_and_remember() {
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(workspace()))));
        let original_revision = state.workspace.as_ref().unwrap().projects[0]
            .revision
            .clone();
        assert!(state.request_open("project-1", ZedOpenStrategy::ReuseWindow, true));
        assert!(state.set_open_strategy(ZedOpenStrategy::NewWindow));
        assert!(state.set_open_remember(false));

        let mut changed = workspace();
        changed.projects[0].label = "changed label".to_owned();
        changed.projects[0].revision = ZedProjectRevision::from_digest([9; 32]);
        let refresh = state.begin_load();
        assert!(state.apply_load_response(refresh, Ok(Arc::new(changed))));

        let (_, request) = state.begin_open().unwrap();
        assert_eq!(request.expected_project_revision, original_revision);
        assert_eq!(request.strategy, ZedOpenStrategy::NewWindow);
        assert!(!request.remember);
        assert_eq!(state.pending_open.as_ref().unwrap().label, "alpha");
    }

    #[test]
    fn conflicts_preserve_draft_and_confirmation_and_request_one_refresh() {
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(workspace()))));
        state.set_strategy(ZedOpenStrategy::NewWindow);
        let (save_id, _) = state.begin_save_preferences().unwrap();
        assert!(state.apply_save_response(
            save_id,
            Err(ZedRemoteFailureKind::Service(
                ZedRemoteErrorKind::SettingsConflict
            ))
        ));
        assert!(state.preferences_dirty);
        assert_eq!(state.draft_strategy, ZedOpenStrategy::NewWindow);
        assert!(state.take_refresh_after_conflict());
        assert!(!state.take_refresh_after_conflict());

        assert!(state.request_open("project-1", ZedOpenStrategy::Default, true));
        let (open_id, _) = state.begin_open().unwrap();
        assert!(state.apply_open_response(
            open_id,
            Err(ZedRemoteFailureKind::Service(
                ZedRemoteErrorKind::RegistryConflict
            ))
        ));
        assert!(state.pending_open.is_some());
        assert!(state.take_refresh_after_conflict());
    }

    #[test]
    fn only_one_mutation_can_run_and_partial_launch_is_retained() {
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(workspace()))));
        state.set_strategy(ZedOpenStrategy::NewWindow);
        assert!(state.request_open("project-1", ZedOpenStrategy::Default, true));
        let (open_id, _) = state.begin_open().unwrap();
        assert!(state.begin_save_preferences().is_none());
        assert!(!state.request_forget("project-1"));

        let outcome = ZedRemoteOpenOutcome {
            workspace: workspace(),
            strategy: ZedOpenStrategy::Default,
            url: "zed://redacted".to_owned(),
            remember: codex_plus_manager_service::ZedRememberOutcome::Failed(
                ZedRemoteErrorKind::RegistryWriteFailed,
            ),
        };
        assert!(state.apply_open_response(open_id, Ok(Arc::new(outcome))));
        assert!(matches!(
            state.open_outcome.as_ref().unwrap().remember,
            codex_plus_manager_service::ZedRememberOutcome::Failed(
                ZedRemoteErrorKind::RegistryWriteFailed
            )
        ));
    }

    #[test]
    fn recent_and_discovered_groups_are_bounded_and_query_resets_pages() {
        let mut fixture = workspace();
        fixture.projects.clear();
        for index in 0..60 {
            let mut project = workspace().projects.remove(0);
            project.id = format!("recent-{index}");
            project.label = format!("Recent {index}");
            project.source = ZedRemoteProjectSource::Recent;
            fixture.projects.push(project);
        }
        let mut state = ZedRemoteViewState::default();
        let load = state.begin_load();
        assert!(state.apply_load_response(load, Ok(Arc::new(fixture))));
        assert_eq!(state.page_count(ZedRemoteProjectSource::Recent), 3);
        assert_eq!(
            state
                .visible_project_ids(ZedRemoteProjectSource::Recent)
                .len(),
            ZED_PROJECT_PAGE_SIZE
        );
        state.set_recent_page(2);
        assert_eq!(
            state
                .visible_project_ids(ZedRemoteProjectSource::Recent)
                .len(),
            10
        );
        state.set_search_query("Recent 5");
        assert_eq!(state.recent_page, 0);
        assert_eq!(state.page_count(ZedRemoteProjectSource::Recent), 1);
    }
}
