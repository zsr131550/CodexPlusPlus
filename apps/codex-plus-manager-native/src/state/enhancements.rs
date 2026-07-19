use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    EnhancementError, EnhancementErrorKind, EnhancementRevision, EnhancementSettings,
    EnhancementWorkspace, ResetEnhancements, SaveEnhancements,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnhancementLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnhancementOperationPhase {
    #[default]
    Idle,
    Saving,
    Resetting,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancementFailureKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    ConfirmationRequired,
    WorkerStopped,
}

impl From<EnhancementErrorKind> for EnhancementFailureKind {
    fn from(kind: EnhancementErrorKind) -> Self {
        match kind {
            EnhancementErrorKind::SettingsReadFailed => Self::SettingsReadFailed,
            EnhancementErrorKind::SettingsWriteFailed => Self::SettingsWriteFailed,
            EnhancementErrorKind::SettingsConflict => Self::SettingsConflict,
            EnhancementErrorKind::InvalidRevision => Self::InvalidRevision,
            EnhancementErrorKind::ConfirmationRequired => Self::ConfirmationRequired,
            EnhancementErrorKind::WorkerStopped => Self::WorkerStopped,
        }
    }
}

#[derive(Clone)]
pub struct EnhancementFailure {
    pub kind: EnhancementFailureKind,
    pub refreshed_workspace: Option<Arc<EnhancementWorkspace>>,
}

impl EnhancementFailure {
    pub fn new(kind: EnhancementFailureKind) -> Self {
        Self {
            kind,
            refreshed_workspace: None,
        }
    }

    pub fn with_workspace(
        kind: EnhancementFailureKind,
        workspace: Arc<EnhancementWorkspace>,
    ) -> Self {
        Self {
            kind,
            refreshed_workspace: Some(workspace),
        }
    }

    pub fn from_service(error: &EnhancementError) -> Self {
        Self {
            kind: error.kind().into(),
            refreshed_workspace: error.refreshed_workspace().cloned().map(Arc::new),
        }
    }
}

impl fmt::Debug for EnhancementFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnhancementFailure")
            .field("kind", &self.kind)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

#[derive(Debug)]
pub struct EnhancementViewState {
    pub initialized: bool,
    pub load_phase: EnhancementLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<EnhancementFailureKind>,
    pub operation_phase: EnhancementOperationPhase,
    pub current_operation_request_id: u64,
    pub error: Option<EnhancementFailureKind>,
    revision: Option<EnhancementRevision>,
    saved: EnhancementSettings,
    draft: EnhancementSettings,
    submitted: Option<EnhancementSettings>,
    conflict_workspace: Option<Arc<EnhancementWorkspace>>,
    reset_confirmation: bool,
    next_request_id: u64,
}

impl Default for EnhancementViewState {
    fn default() -> Self {
        let settings = EnhancementSettings::default();
        Self {
            initialized: false,
            load_phase: EnhancementLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            operation_phase: EnhancementOperationPhase::Idle,
            current_operation_request_id: 0,
            error: None,
            revision: None,
            saved: settings,
            draft: settings,
            submitted: None,
            conflict_workspace: None,
            reset_confirmation: false,
            next_request_id: 0,
        }
    }
}

impl EnhancementViewState {
    pub fn from_workspace(workspace: Arc<EnhancementWorkspace>) -> Self {
        let mut state = Self::default();
        state.install_workspace(&workspace, false);
        state.initialized = true;
        state.load_phase = EnhancementLoadPhase::Ready;
        state
    }

    pub fn draft(&self) -> &EnhancementSettings {
        &self.draft
    }

    pub fn is_dirty(&self) -> bool {
        self.initialized && self.draft != self.saved
    }

    pub fn subcontrols_enabled(&self) -> bool {
        self.initialized && self.draft.enabled && !self.mutation_running()
    }

    pub fn edit(&mut self, settings: EnhancementSettings) {
        self.draft = settings;
    }

    pub fn begin_load(&mut self) -> u64 {
        let request_id = self.next_request_id();
        self.current_load_request_id = request_id;
        self.load_phase = if self.initialized {
            EnhancementLoadPhase::Refreshing
        } else {
            EnhancementLoadPhase::Loading
        };
        request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                let preserve_draft = self.is_dirty();
                self.install_workspace(&workspace, preserve_draft);
                self.initialized = true;
                self.load_phase = EnhancementLoadPhase::Ready;
                self.load_error = None;
            }
            Err(error) => {
                self.load_phase = EnhancementLoadPhase::Error;
                self.load_error = Some(error);
            }
        }
        true
    }

    pub fn begin_save(&mut self) -> Option<(u64, SaveEnhancements)> {
        if !self.initialized || !self.is_dirty() || self.mutation_running() {
            return None;
        }
        let expected_revision = self.revision?;
        let request_id = self.next_request_id();
        self.current_operation_request_id = request_id;
        self.operation_phase = EnhancementOperationPhase::Saving;
        self.error = None;
        self.submitted = Some(self.draft);
        Some((
            request_id,
            SaveEnhancements {
                expected_revision,
                settings: self.draft,
            },
        ))
    }

    pub fn apply_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementFailure>,
    ) -> bool {
        self.apply_mutation_response(request_id, EnhancementOperationPhase::Saving, result)
    }

    pub fn request_reset(&mut self) -> bool {
        if !self.initialized || self.mutation_running() || self.reset_confirmation {
            return false;
        }
        self.reset_confirmation = true;
        true
    }

    pub fn reset_confirmation_pending(&self) -> bool {
        self.reset_confirmation
    }

    pub fn cancel_reset(&mut self) {
        self.reset_confirmation = false;
    }

    pub fn confirm_reset(&mut self) -> Option<(u64, ResetEnhancements)> {
        if !self.reset_confirmation || self.mutation_running() {
            return None;
        }
        self.reset_confirmation = false;
        let expected_revision = self.revision?;
        let request_id = self.next_request_id();
        self.current_operation_request_id = request_id;
        self.operation_phase = EnhancementOperationPhase::Resetting;
        self.error = None;
        self.submitted = Some(self.draft);
        Some((
            request_id,
            ResetEnhancements {
                expected_revision,
                confirmed: true,
            },
        ))
    }

    pub fn apply_reset_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementFailure>,
    ) -> bool {
        self.apply_mutation_response(request_id, EnhancementOperationPhase::Resetting, result)
    }

    pub fn reload_conflict(&mut self) -> bool {
        let Some(workspace) = self.conflict_workspace.take() else {
            return false;
        };
        self.install_workspace(&workspace, false);
        self.initialized = true;
        self.load_phase = EnhancementLoadPhase::Ready;
        self.operation_phase = EnhancementOperationPhase::Ready;
        self.load_error = None;
        self.error = None;
        true
    }

    pub fn discard_changes(&mut self) {
        self.draft = self.saved;
        self.error = None;
        self.conflict_workspace = None;
    }

    pub fn fail_running_operations(&mut self) {
        if matches!(
            self.load_phase,
            EnhancementLoadPhase::Loading | EnhancementLoadPhase::Refreshing
        ) {
            self.load_phase = EnhancementLoadPhase::Error;
            self.load_error = Some(EnhancementFailureKind::WorkerStopped);
        }
        if self.mutation_running() {
            self.operation_phase = EnhancementOperationPhase::Error;
            self.error = Some(EnhancementFailureKind::WorkerStopped);
            self.revision = None;
            self.submitted = None;
        }
        self.reset_confirmation = false;
    }

    fn apply_mutation_response(
        &mut self,
        request_id: u64,
        expected_phase: EnhancementOperationPhase,
        result: Result<Arc<EnhancementWorkspace>, EnhancementFailure>,
    ) -> bool {
        if request_id != self.current_operation_request_id || self.operation_phase != expected_phase
        {
            return false;
        }
        match result {
            Ok(workspace) => {
                let preserve_draft = self.submitted.is_some_and(|value| self.draft != value);
                self.install_workspace(&workspace, preserve_draft);
                self.operation_phase = EnhancementOperationPhase::Ready;
                self.error = None;
                self.conflict_workspace = None;
            }
            Err(failure) => {
                self.operation_phase = EnhancementOperationPhase::Error;
                self.error = Some(failure.kind);
                self.conflict_workspace = failure.refreshed_workspace;
                self.revision = None;
            }
        }
        self.submitted = None;
        true
    }

    fn install_workspace(&mut self, workspace: &EnhancementWorkspace, preserve_draft: bool) {
        let draft = self.draft;
        self.revision = Some(workspace.revision);
        self.saved = workspace.settings;
        self.draft = if preserve_draft {
            draft
        } else {
            workspace.settings
        };
    }

    fn mutation_running(&self) -> bool {
        matches!(
            self.operation_phase,
            EnhancementOperationPhase::Saving | EnhancementOperationPhase::Resetting
        )
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("enhancement request id overflow");
        self.next_request_id
    }
}
