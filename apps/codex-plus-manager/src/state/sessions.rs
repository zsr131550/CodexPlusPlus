use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    DeleteSessionSelection, DeleteSessions, ProviderSyncErrorKind, ProviderSyncOutcome,
    ProviderSyncWorkspace, RunProviderSync, SessionDeleteBatchOutcome, SessionErrorKind,
    SessionSummary, SessionWorkspace, SetProviderAutoRepair,
};

use super::provider::OperationPhase;

pub const SESSION_PAGE_SIZE: usize = 50;
pub const DELETE_PREVIEW_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionFilter {
    #[default]
    All,
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFailureKind {
    Service(SessionErrorKind),
    WorkerStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSyncFailureKind {
    Service(ProviderSyncErrorKind),
    WorkerStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirmation {
    pub session_ids: Vec<String>,
    pub previews: Vec<String>,
}

impl DeleteConfirmation {
    pub fn count(&self) -> usize {
        self.session_ids.len()
    }

    pub fn remaining_preview_count(&self) -> usize {
        self.session_ids.len().saturating_sub(self.previews.len())
    }
}

pub struct SessionViewState {
    pub workspace_phase: OperationPhase,
    pub current_workspace_request_id: u64,
    pub workspace_error: Option<SessionFailureKind>,
    pub workspace: Option<Arc<SessionWorkspace>>,
    pub query: String,
    pub filter: SessionFilter,
    pub selected_ids: BTreeSet<String>,
    pub page: usize,

    pub delete_phase: OperationPhase,
    pub current_delete_request_id: u64,
    pub delete_error: Option<SessionFailureKind>,
    pub delete_confirmation: Option<DeleteConfirmation>,
    pub delete_outcome: Option<Arc<SessionDeleteBatchOutcome>>,

    pub provider_workspace_phase: OperationPhase,
    pub current_provider_workspace_request_id: u64,
    pub provider_workspace_error: Option<ProviderSyncFailureKind>,
    pub provider_workspace: Option<Arc<ProviderSyncWorkspace>>,
    pub selected_provider_target: String,

    pub provider_run_phase: OperationPhase,
    pub current_provider_run_request_id: u64,
    pub provider_run_error: Option<ProviderSyncFailureKind>,
    pub provider_outcome: Option<Arc<ProviderSyncOutcome>>,
    pub provider_run_confirmation: Option<String>,

    pub auto_repair_phase: OperationPhase,
    pub current_auto_repair_request_id: u64,
    pub auto_repair_error: Option<ProviderSyncFailureKind>,

    worker_stopped: bool,
}

impl Default for SessionViewState {
    fn default() -> Self {
        Self {
            workspace_phase: OperationPhase::Idle,
            current_workspace_request_id: 0,
            workspace_error: None,
            workspace: None,
            query: String::new(),
            filter: SessionFilter::All,
            selected_ids: BTreeSet::new(),
            page: 0,
            delete_phase: OperationPhase::Idle,
            current_delete_request_id: 0,
            delete_error: None,
            delete_confirmation: None,
            delete_outcome: None,
            provider_workspace_phase: OperationPhase::Idle,
            current_provider_workspace_request_id: 0,
            provider_workspace_error: None,
            provider_workspace: None,
            selected_provider_target: String::new(),
            provider_run_phase: OperationPhase::Idle,
            current_provider_run_request_id: 0,
            provider_run_error: None,
            provider_outcome: None,
            provider_run_confirmation: None,
            auto_repair_phase: OperationPhase::Idle,
            current_auto_repair_request_id: 0,
            auto_repair_error: None,
            worker_stopped: false,
        }
    }
}

impl fmt::Debug for SessionViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionViewState")
            .field("workspace_phase", &self.workspace_phase)
            .field(
                "session_count",
                &self
                    .workspace
                    .as_ref()
                    .map_or(0, |workspace| workspace.sessions.len()),
            )
            .field("selected_count", &self.selected_ids.len())
            .field("page", &self.page)
            .field("delete_phase", &self.delete_phase)
            .field("has_delete_outcome", &self.delete_outcome.is_some())
            .field("provider_workspace_phase", &self.provider_workspace_phase)
            .field("provider_run_phase", &self.provider_run_phase)
            .field(
                "has_provider_run_confirmation",
                &self.provider_run_confirmation.is_some(),
            )
            .field("auto_repair_phase", &self.auto_repair_phase)
            .field("worker_stopped", &self.worker_stopped)
            .finish_non_exhaustive()
    }
}

impl SessionViewState {
    pub fn filtered_sessions(&self) -> Vec<&SessionSummary> {
        let query = self.query.trim().to_lowercase();
        self.workspace
            .as_ref()
            .into_iter()
            .flat_map(|workspace| &workspace.sessions)
            .filter(|session| match self.filter {
                SessionFilter::All => true,
                SessionFilter::Active => !session.archived,
                SessionFilter::Archived => session.archived,
            })
            .filter(|session| {
                query.is_empty()
                    || session.id.to_lowercase().contains(&query)
                    || session.title.to_lowercase().contains(&query)
                    || session.cwd.to_lowercase().contains(&query)
                    || session.model_provider.to_lowercase().contains(&query)
                    || session
                        .source_db_paths
                        .iter()
                        .any(|path| path.to_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn page_count(&self) -> usize {
        self.filtered_sessions()
            .len()
            .div_ceil(SESSION_PAGE_SIZE)
            .max(1)
    }

    pub fn page_sessions(&self) -> Vec<&SessionSummary> {
        self.filtered_sessions()
            .into_iter()
            .skip(self.page * SESSION_PAGE_SIZE)
            .take(SESSION_PAGE_SIZE)
            .collect()
    }

    pub fn active_count(&self) -> usize {
        self.workspace.as_ref().map_or(0, |workspace| {
            workspace
                .sessions
                .iter()
                .filter(|session| !session.archived)
                .count()
        })
    }

    pub fn archived_count(&self) -> usize {
        self.workspace.as_ref().map_or(0, |workspace| {
            workspace
                .sessions
                .iter()
                .filter(|session| session.archived)
                .count()
        })
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.page = 0;
    }

    pub fn set_filter(&mut self, filter: SessionFilter) {
        self.filter = filter;
        self.page = 0;
    }

    pub fn set_page(&mut self, page: usize) -> bool {
        let clamped = page.min(self.page_count().saturating_sub(1));
        let changed = self.page != clamped;
        self.page = clamped;
        changed
    }

    pub fn set_selected(&mut self, id: &str, selected: bool) -> bool {
        if self.delete_phase == OperationPhase::Running
            || !self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.sessions.iter().any(|item| item.id == id))
        {
            return false;
        }
        if selected {
            self.selected_ids.insert(id.to_owned())
        } else {
            self.selected_ids.remove(id)
        }
    }

    pub fn select_all_filtered(&mut self) -> bool {
        if self.delete_phase == OperationPhase::Running {
            return false;
        }
        let ids = self
            .filtered_sessions()
            .into_iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        let before = self.selected_ids.len();
        self.selected_ids.extend(ids);
        self.selected_ids.len() != before
    }

    pub fn clear_selection(&mut self) -> bool {
        if self.delete_phase == OperationPhase::Running {
            return false;
        }
        let changed = !self.selected_ids.is_empty();
        self.selected_ids.clear();
        changed
    }

    pub fn begin_workspace_refresh(&mut self) -> u64 {
        self.current_workspace_request_id = next_id(
            self.current_workspace_request_id,
            "session workspace refresh",
        );
        self.workspace_phase = OperationPhase::Running;
        self.workspace_error = None;
        self.current_workspace_request_id
    }

    pub fn apply_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<SessionWorkspace>, SessionFailureKind>,
    ) -> bool {
        if request_id != self.current_workspace_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.install_workspace(workspace);
                self.workspace_phase = OperationPhase::Ready;
                self.workspace_error = None;
            }
            Err(error) => {
                self.workspace_phase = OperationPhase::Error;
                self.workspace_error = Some(error);
            }
        }
        true
    }

    pub fn request_delete(&mut self) -> bool {
        if self.worker_stopped
            || self.delete_phase == OperationPhase::Running
            || self.delete_confirmation.is_some()
            || self.provider_run_confirmation.is_some()
            || self.selected_ids.is_empty()
        {
            return false;
        }
        let Some(workspace) = self.workspace.as_ref() else {
            return false;
        };
        let selected = workspace
            .sessions
            .iter()
            .filter(|session| self.selected_ids.contains(&session.id))
            .collect::<Vec<_>>();
        if selected.len() != self.selected_ids.len() {
            return false;
        }
        self.delete_confirmation = Some(DeleteConfirmation {
            session_ids: selected.iter().map(|session| session.id.clone()).collect(),
            previews: selected
                .iter()
                .take(DELETE_PREVIEW_LIMIT)
                .map(|session| {
                    if session.title.trim().is_empty() {
                        session.id.clone()
                    } else {
                        session.title.clone()
                    }
                })
                .collect(),
        });
        self.delete_error = None;
        true
    }

    pub fn cancel_delete(&mut self) -> bool {
        if self.delete_phase == OperationPhase::Running {
            return false;
        }
        self.delete_confirmation.take().is_some()
    }

    pub fn confirm_delete(&mut self) -> Option<(u64, DeleteSessions)> {
        if self.worker_stopped || self.delete_phase == OperationPhase::Running {
            return None;
        }
        let confirmation = self.delete_confirmation.as_ref()?;
        let confirmed = confirmation
            .session_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if confirmed != self.selected_ids {
            self.delete_confirmation = None;
            return None;
        }
        let workspace = self.workspace.as_ref()?;
        let by_id = workspace
            .sessions
            .iter()
            .map(|session| (session.id.as_str(), session))
            .collect::<BTreeMap<_, _>>();
        let selections = confirmation
            .session_ids
            .iter()
            .map(|id| {
                by_id
                    .get(id.as_str())
                    .map(|session| DeleteSessionSelection {
                        id: id.clone(),
                        expected_revision: session.revision.clone(),
                    })
            })
            .collect::<Option<Vec<_>>>()?;
        let confirmed_ids = confirmation.session_ids.clone();
        self.current_delete_request_id = next_id(self.current_delete_request_id, "session delete");
        self.delete_phase = OperationPhase::Running;
        self.delete_error = None;
        self.delete_outcome = None;
        self.delete_confirmation = None;
        Some((
            self.current_delete_request_id,
            DeleteSessions {
                selections,
                confirmed_ids,
            },
        ))
    }

    pub fn apply_delete_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<SessionDeleteBatchOutcome>, SessionFailureKind>,
    ) -> bool {
        if request_id != self.current_delete_request_id {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.current_workspace_request_id = next_id(
                    self.current_workspace_request_id,
                    "session workspace invalidation",
                );
                self.install_workspace(Arc::new(outcome.workspace.clone()));
                self.delete_outcome = Some(outcome);
                self.delete_phase = OperationPhase::Ready;
                self.delete_error = None;
                self.workspace_phase = OperationPhase::Ready;
                self.workspace_error = None;
            }
            Err(error) => {
                self.delete_phase = OperationPhase::Error;
                self.delete_error = Some(error);
            }
        }
        true
    }

    pub fn begin_provider_workspace_refresh(&mut self) -> Option<u64> {
        if self.worker_stopped
            || self.provider_run_phase == OperationPhase::Running
            || self.auto_repair_phase == OperationPhase::Running
        {
            return None;
        }
        self.current_provider_workspace_request_id = next_id(
            self.current_provider_workspace_request_id,
            "provider sync workspace refresh",
        );
        self.provider_workspace_phase = OperationPhase::Running;
        self.provider_workspace_error = None;
        Some(self.current_provider_workspace_request_id)
    }

    pub fn apply_provider_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderSyncWorkspace>, ProviderSyncFailureKind>,
    ) -> bool {
        if request_id != self.current_provider_workspace_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                if self.selected_provider_target.is_empty()
                    || !workspace
                        .targets
                        .targets
                        .iter()
                        .any(|target| target.id == self.selected_provider_target)
                {
                    self.selected_provider_target = workspace.selected_target.clone();
                }
                self.provider_workspace = Some(workspace);
                self.provider_workspace_phase = OperationPhase::Ready;
                self.provider_workspace_error = None;
            }
            Err(error) => {
                self.provider_workspace_phase = OperationPhase::Error;
                self.provider_workspace_error = Some(error);
            }
        }
        true
    }

    pub fn set_provider_target(&mut self, target: String) -> bool {
        if self.provider_run_phase == OperationPhase::Running
            || self.provider_run_confirmation.is_some()
            || !self.provider_workspace.as_ref().is_some_and(|workspace| {
                workspace
                    .targets
                    .targets
                    .iter()
                    .any(|item| item.id == target)
            })
        {
            return false;
        }
        let changed = self.selected_provider_target != target;
        self.selected_provider_target = target;
        changed
    }

    pub fn request_provider_run_confirmation(&mut self) -> bool {
        if self.worker_stopped
            || self.provider_run_phase == OperationPhase::Running
            || self.auto_repair_phase == OperationPhase::Running
            || self.provider_run_confirmation.is_some()
            || self.delete_confirmation.is_some()
            || self.selected_provider_target.trim().is_empty()
        {
            return false;
        }
        self.provider_run_confirmation = Some(self.selected_provider_target.clone());
        self.provider_run_error = None;
        true
    }

    pub fn cancel_provider_run_confirmation(&mut self) -> bool {
        if self.provider_run_phase == OperationPhase::Running {
            return false;
        }
        self.provider_run_confirmation.take().is_some()
    }

    pub fn confirm_provider_run(&mut self) -> Option<(u64, RunProviderSync)> {
        let confirmed_target = self.provider_run_confirmation.take()?;
        if confirmed_target != self.selected_provider_target {
            return None;
        }
        self.begin_provider_run()
    }

    pub fn begin_provider_run(&mut self) -> Option<(u64, RunProviderSync)> {
        if self.worker_stopped
            || self.provider_run_phase == OperationPhase::Running
            || self.auto_repair_phase == OperationPhase::Running
            || self.provider_run_confirmation.is_some()
            || self.selected_provider_target.trim().is_empty()
        {
            return None;
        }
        self.current_provider_run_request_id =
            next_id(self.current_provider_run_request_id, "provider sync run");
        self.provider_run_phase = OperationPhase::Running;
        self.provider_run_error = None;
        self.provider_outcome = None;
        Some((
            self.current_provider_run_request_id,
            RunProviderSync {
                target_provider: self.selected_provider_target.clone(),
                confirmed_target_provider: self.selected_provider_target.clone(),
            },
        ))
    }

    pub fn apply_provider_run_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderSyncOutcome>, ProviderSyncFailureKind>,
    ) -> bool {
        if request_id != self.current_provider_run_request_id {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.provider_workspace = Some(Arc::new(outcome.workspace.clone()));
                self.selected_provider_target = outcome.workspace.selected_target.clone();
                self.provider_outcome = Some(outcome);
                self.provider_run_phase = OperationPhase::Ready;
                self.provider_run_error = None;
            }
            Err(error) => {
                self.provider_run_phase = OperationPhase::Error;
                self.provider_run_error = Some(error);
            }
        }
        true
    }

    pub fn begin_set_auto_repair(&mut self, enabled: bool) -> Option<(u64, SetProviderAutoRepair)> {
        if self.worker_stopped
            || self.provider_run_phase == OperationPhase::Running
            || self.auto_repair_phase == OperationPhase::Running
            || self.provider_run_confirmation.is_some()
        {
            return None;
        }
        let workspace = self.provider_workspace.as_ref()?;
        self.current_auto_repair_request_id =
            next_id(self.current_auto_repair_request_id, "provider auto repair");
        self.auto_repair_phase = OperationPhase::Running;
        self.auto_repair_error = None;
        Some((
            self.current_auto_repair_request_id,
            SetProviderAutoRepair {
                expected_revision: workspace.revision.clone(),
                enabled,
            },
        ))
    }

    pub fn apply_auto_repair_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderSyncWorkspace>, ProviderSyncFailureKind>,
    ) -> bool {
        if request_id != self.current_auto_repair_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.provider_workspace = Some(workspace);
                self.auto_repair_phase = OperationPhase::Ready;
                self.auto_repair_error = None;
            }
            Err(error) => {
                self.auto_repair_phase = OperationPhase::Error;
                self.auto_repair_error = Some(error);
            }
        }
        true
    }

    pub fn mark_worker_stopped(&mut self) {
        self.worker_stopped = true;
        self.provider_run_confirmation = None;
        if self.workspace_phase == OperationPhase::Running {
            self.workspace_phase = OperationPhase::Error;
            self.workspace_error = Some(SessionFailureKind::WorkerStopped);
        }
        if self.delete_phase == OperationPhase::Running {
            self.delete_phase = OperationPhase::Error;
            self.delete_error = Some(SessionFailureKind::WorkerStopped);
        }
        if self.provider_workspace_phase == OperationPhase::Running {
            self.provider_workspace_phase = OperationPhase::Error;
            self.provider_workspace_error = Some(ProviderSyncFailureKind::WorkerStopped);
        }
        if self.provider_run_phase == OperationPhase::Running {
            self.provider_run_phase = OperationPhase::Error;
            self.provider_run_error = Some(ProviderSyncFailureKind::WorkerStopped);
        }
        if self.auto_repair_phase == OperationPhase::Running {
            self.auto_repair_phase = OperationPhase::Error;
            self.auto_repair_error = Some(ProviderSyncFailureKind::WorkerStopped);
        }
    }

    pub fn worker_failure(&self) -> Option<SessionFailureKind> {
        self.worker_stopped
            .then_some(SessionFailureKind::WorkerStopped)
    }

    pub fn mutations_enabled(&self) -> bool {
        !self.worker_stopped
            && self.delete_phase != OperationPhase::Running
            && self.provider_run_phase != OperationPhase::Running
            && self.auto_repair_phase != OperationPhase::Running
    }

    fn install_workspace(&mut self, workspace: Arc<SessionWorkspace>) {
        let valid_ids = workspace
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<BTreeSet<_>>();
        self.selected_ids
            .retain(|session_id| valid_ids.contains(session_id.as_str()));
        self.workspace = Some(workspace);
        self.page = self.page.min(self.page_count().saturating_sub(1));
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}
