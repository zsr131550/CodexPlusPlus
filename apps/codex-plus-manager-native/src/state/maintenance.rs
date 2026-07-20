use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    AppPathRevision, LaunchCodex, LaunchOutcome, MaintenanceError, MaintenanceErrorKind,
    MaintenanceWorkspace, PrivatePath, SaveCodexAppPath, SectionValue,
};

use crate::path_picker::{
    PathPickerErrorKind, PathPickerRequest, PathPickerResponse, PathPickerTarget,
};

use super::Route;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaintenanceLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaintenanceOperationPhase {
    #[default]
    Idle,
    Running,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceFailureKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    InvalidPath,
    InvalidPort,
    EntrypointReadFailed,
    StatusReadFailed,
    LogReadFailed,
    LaunchFailed,
    WorkerStopped,
}

impl From<MaintenanceErrorKind> for MaintenanceFailureKind {
    fn from(kind: MaintenanceErrorKind) -> Self {
        match kind {
            MaintenanceErrorKind::SettingsReadFailed => Self::SettingsReadFailed,
            MaintenanceErrorKind::SettingsWriteFailed => Self::SettingsWriteFailed,
            MaintenanceErrorKind::SettingsConflict => Self::SettingsConflict,
            MaintenanceErrorKind::InvalidRevision => Self::InvalidRevision,
            MaintenanceErrorKind::InvalidPath => Self::InvalidPath,
            MaintenanceErrorKind::InvalidPort => Self::InvalidPort,
            MaintenanceErrorKind::EntrypointReadFailed => Self::EntrypointReadFailed,
            MaintenanceErrorKind::StatusReadFailed => Self::StatusReadFailed,
            MaintenanceErrorKind::LogReadFailed => Self::LogReadFailed,
            MaintenanceErrorKind::LaunchFailed => Self::LaunchFailed,
            MaintenanceErrorKind::WorkerStopped => Self::WorkerStopped,
        }
    }
}

#[derive(Clone)]
pub struct MaintenanceFailure {
    pub kind: MaintenanceFailureKind,
    pub refreshed_workspace: Option<Arc<MaintenanceWorkspace>>,
}

impl MaintenanceFailure {
    pub fn new(kind: MaintenanceFailureKind) -> Self {
        Self {
            kind,
            refreshed_workspace: None,
        }
    }

    pub fn with_workspace(
        kind: MaintenanceFailureKind,
        workspace: Arc<MaintenanceWorkspace>,
    ) -> Self {
        Self {
            kind,
            refreshed_workspace: Some(workspace),
        }
    }

    pub fn from_service(error: &MaintenanceError) -> Self {
        Self {
            kind: error.kind().into(),
            refreshed_workspace: error.refreshed_workspace().cloned().map(Arc::new),
        }
    }
}

impl fmt::Debug for MaintenanceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MaintenanceFailure")
            .field("kind", &self.kind)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct AppPathDraft(String);

impl AppPathDraft {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for AppPathDraft {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Debug for AppPathDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppPathDraft")
            .field("configured", &!self.0.trim().is_empty())
            .field("length", &self.0.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaintenanceDocumentTab {
    #[default]
    Logs,
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceTransition {
    Navigate(Route),
    Refresh,
}

#[derive(Debug, Default)]
pub struct MaintenanceOperationState {
    pub phase: MaintenanceOperationPhase,
    pub current_request_id: u64,
    pub error: Option<MaintenanceFailureKind>,
}

pub struct MaintenanceViewState {
    pub load_phase: MaintenanceLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<MaintenanceFailureKind>,
    pub workspace: Option<Arc<MaintenanceWorkspace>>,
    app_path_base: AppPathDraft,
    app_path_revision: Option<AppPathRevision>,
    pub app_path_draft: AppPathDraft,
    pub debug_port: u16,
    pub helper_port: u16,
    pub document_tab: MaintenanceDocumentTab,
    pub log_limit: usize,
    pub save: MaintenanceOperationState,
    pub launch: MaintenanceOperationState,
    pub launch_outcome: Option<LaunchOutcome>,
    pub picker_error: Option<PathPickerErrorKind>,
    pending_picker: Option<(u64, PathPickerTarget)>,
    clear_confirmation: Option<SaveCodexAppPath>,
    pending_transition: Option<MaintenanceTransition>,
    conflict: bool,
    next_request_id: u64,
}

impl Default for MaintenanceViewState {
    fn default() -> Self {
        Self {
            load_phase: MaintenanceLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            workspace: None,
            app_path_base: AppPathDraft::default(),
            app_path_revision: None,
            app_path_draft: AppPathDraft::default(),
            debug_port: 9229,
            helper_port: 57321,
            document_tab: MaintenanceDocumentTab::Logs,
            log_limit: 100,
            save: MaintenanceOperationState::default(),
            launch: MaintenanceOperationState::default(),
            launch_outcome: None,
            picker_error: None,
            pending_picker: None,
            clear_confirmation: None,
            pending_transition: None,
            conflict: false,
            next_request_id: 0,
        }
    }
}

impl fmt::Debug for MaintenanceViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MaintenanceViewState")
            .field("load_phase", &self.load_phase)
            .field("load_request_id", &self.current_load_request_id)
            .field("has_workspace", &self.workspace.is_some())
            .field("path_configured", &!self.app_path_draft.0.trim().is_empty())
            .field("path_dirty", &self.path_dirty())
            .field("debug_port", &self.debug_port)
            .field("helper_port", &self.helper_port)
            .field("document_tab", &self.document_tab)
            .field("log_limit", &self.log_limit)
            .field("save_phase", &self.save.phase)
            .field("launch_phase", &self.launch.phase)
            .field("picker_pending", &self.pending_picker.is_some())
            .field("clear_confirmation", &self.clear_confirmation.is_some())
            .field("discard_confirmation", &self.pending_transition.is_some())
            .field("conflict", &self.conflict)
            .finish()
    }
}

impl MaintenanceViewState {
    pub fn begin_load(&mut self) -> u64 {
        let request_id = self.next_request_id();
        self.current_load_request_id = request_id;
        self.load_phase = if self.workspace.is_some() {
            MaintenanceLoadPhase::Refreshing
        } else {
            MaintenanceLoadPhase::Loading
        };
        request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<MaintenanceWorkspace>, MaintenanceFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.install_workspace(workspace, true);
                self.load_error = None;
                self.load_phase = MaintenanceLoadPhase::Ready;
            }
            Err(error) => {
                self.load_error = Some(error);
                self.load_phase = MaintenanceLoadPhase::Error;
            }
        }
        true
    }

    pub fn set_app_path(&mut self, path: String) {
        self.app_path_draft = path.into();
        self.conflict = false;
    }

    pub fn path_dirty(&self) -> bool {
        self.app_path_draft != self.app_path_base
    }

    pub fn begin_save(&mut self) -> Option<(u64, SaveCodexAppPath)> {
        if self.save.phase == MaintenanceOperationPhase::Running || !self.path_dirty() {
            return None;
        }
        let revision = self.app_path_revision?;
        let request = SaveCodexAppPath {
            expected_revision: revision,
            path: PrivatePath::new(self.app_path_draft.0.clone()),
            confirmed_clear: self.app_path_draft.0.trim().is_empty(),
        };
        if request.confirmed_clear && !self.app_path_base.0.trim().is_empty() {
            if self.clear_confirmation.is_none() {
                self.clear_confirmation = Some(request);
            }
            return None;
        }
        Some(self.start_save(request))
    }

    pub fn request_clear(&mut self) -> bool {
        if self.save.phase == MaintenanceOperationPhase::Running
            || self.clear_confirmation.is_some()
            || self.app_path_base.0.trim().is_empty()
        {
            return false;
        }
        let Some(expected_revision) = self.app_path_revision else {
            return false;
        };
        self.clear_confirmation = Some(SaveCodexAppPath {
            expected_revision,
            path: PrivatePath::new(String::new()),
            confirmed_clear: true,
        });
        true
    }

    pub fn confirm_clear(&mut self) -> Option<(u64, SaveCodexAppPath)> {
        let request = self.clear_confirmation.take()?;
        if self.save.phase == MaintenanceOperationPhase::Running {
            return None;
        }
        Some(self.start_save(request))
    }

    pub fn cancel_clear(&mut self) {
        self.clear_confirmation = None;
    }

    pub fn clear_confirmation_visible(&self) -> bool {
        self.clear_confirmation.is_some()
    }

    pub fn apply_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<MaintenanceWorkspace>, MaintenanceFailure>,
    ) -> bool {
        if request_id != self.save.current_request_id
            || self.save.phase != MaintenanceOperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.install_workspace(workspace, true);
                self.save.phase = MaintenanceOperationPhase::Ready;
                self.save.error = None;
                self.conflict = false;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.install_workspace(workspace, true);
                }
                self.conflict = failure.kind == MaintenanceFailureKind::SettingsConflict;
                self.save.phase = MaintenanceOperationPhase::Error;
                self.save.error = Some(failure.kind);
            }
        }
        true
    }

    pub fn conflict_visible(&self) -> bool {
        self.conflict
    }

    pub fn begin_launch(&mut self) -> Option<(u64, LaunchCodex)> {
        if self.launch.phase == MaintenanceOperationPhase::Running {
            return None;
        }
        let request_id = self.next_request_id();
        self.launch.current_request_id = request_id;
        self.launch.phase = MaintenanceOperationPhase::Running;
        self.launch.error = None;
        self.launch_outcome = None;
        Some((
            request_id,
            LaunchCodex::native(
                PrivatePath::new(self.app_path_draft.0.clone()),
                self.debug_port,
                self.helper_port,
            ),
        ))
    }

    pub fn apply_launch_response(
        &mut self,
        request_id: u64,
        result: Result<LaunchOutcome, MaintenanceFailureKind>,
    ) -> bool {
        if request_id != self.launch.current_request_id
            || self.launch.phase != MaintenanceOperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.launch_outcome = Some(outcome);
                self.launch.phase = MaintenanceOperationPhase::Ready;
                self.launch.error = None;
            }
            Err(error) => {
                self.launch_outcome = None;
                self.launch.phase = MaintenanceOperationPhase::Error;
                self.launch.error = Some(error);
            }
        }
        true
    }

    pub fn set_document_tab(&mut self, tab: MaintenanceDocumentTab) {
        self.document_tab = tab;
    }

    pub fn set_log_limit(&mut self, limit: usize) -> bool {
        if !matches!(limit, 50 | 100 | 200) {
            return false;
        }
        self.log_limit = limit;
        true
    }

    pub fn active_document_text(&self) -> Option<&str> {
        let workspace = self.workspace.as_ref()?;
        match self.document_tab {
            MaintenanceDocumentTab::Logs => workspace.logs.value().map(|logs| logs.text()),
            MaintenanceDocumentTab::Report => Some(workspace.diagnostics.text()),
        }
    }

    pub fn begin_picker(&mut self, target: PathPickerTarget) -> Option<PathPickerRequest> {
        if self.pending_picker.is_some() {
            return None;
        }
        let request_id = self.next_request_id();
        self.pending_picker = Some((request_id, target));
        self.picker_error = None;
        Some(PathPickerRequest::new(request_id, target))
    }

    pub fn apply_picker_response(&mut self, response: PathPickerResponse) -> bool {
        if self.pending_picker != Some((response.request_id, response.target)) {
            return false;
        }
        self.pending_picker = None;
        self.picker_error = response.error.as_ref().map(|error| error.kind());
        if let Some(path) = response.path {
            self.set_app_path(path.to_string_lossy().into_owned());
        }
        true
    }

    pub fn invalidate_picker(&mut self) {
        self.pending_picker = None;
        self.picker_error = None;
    }

    pub fn picker_pending(&self) -> bool {
        self.pending_picker.is_some()
    }

    pub fn request_transition(&mut self, transition: MaintenanceTransition) -> bool {
        if !self.path_dirty() {
            return true;
        }
        if self.pending_transition.is_none() {
            self.pending_transition = Some(transition);
        }
        false
    }

    pub fn confirm_discard_transition(&mut self) -> Option<MaintenanceTransition> {
        let transition = self.pending_transition.take()?;
        self.invalidate_picker();
        self.cancel_clear();
        self.app_path_draft = self.app_path_base.clone();
        self.conflict = false;
        Some(transition)
    }

    pub fn cancel_transition(&mut self) {
        self.pending_transition = None;
    }

    pub fn discard_confirmation_visible(&self) -> bool {
        self.pending_transition.is_some()
    }

    fn start_save(&mut self, request: SaveCodexAppPath) -> (u64, SaveCodexAppPath) {
        let request_id = self.next_request_id();
        self.save.current_request_id = request_id;
        self.save.phase = MaintenanceOperationPhase::Running;
        self.save.error = None;
        (request_id, request)
    }

    fn install_workspace(&mut self, incoming: Arc<MaintenanceWorkspace>, preserve_dirty: bool) {
        let was_dirty = self.path_dirty();
        let workspace = merge_partial_workspace(self.workspace.as_ref(), incoming);
        if let Some(path) = &workspace.app_path {
            self.app_path_revision = Some(path.revision);
            self.app_path_base = path.value.as_str().to_owned().into();
            if !preserve_dirty || !was_dirty {
                self.app_path_draft = self.app_path_base.clone();
            }
        }
        self.workspace = Some(workspace);
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("maintenance request id overflow");
        self.next_request_id
    }
}

fn merge_partial_workspace(
    previous: Option<&Arc<MaintenanceWorkspace>>,
    incoming: Arc<MaintenanceWorkspace>,
) -> Arc<MaintenanceWorkspace> {
    let Some(previous) = previous else {
        return incoming;
    };
    let needs_merge = incoming.app_path.is_none()
        || incoming.codex_app.is_unavailable()
        || incoming.entrypoints.is_unavailable()
        || incoming.latest_launch.is_unavailable()
        || incoming.logs.is_unavailable();
    if !needs_merge {
        return incoming;
    }
    let mut merged = (*incoming).clone();
    if merged.app_path.is_none() {
        merged.app_path.clone_from(&previous.app_path);
    }
    merge_section(&mut merged.codex_app, &previous.codex_app);
    merge_section(&mut merged.entrypoints, &previous.entrypoints);
    merge_section(&mut merged.latest_launch, &previous.latest_launch);
    merge_section(&mut merged.logs, &previous.logs);
    Arc::new(merged)
}

fn merge_section<T: Clone>(current: &mut SectionValue<T>, previous: &SectionValue<T>) {
    if current.is_unavailable() && previous.is_available() {
        current.clone_from(previous);
    }
}
