use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::Arc;

use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, RelayMode, RelayProfile,
};
use codex_plus_manager_service::{
    ApplyActiveProvider, BackfillActiveProvider, ClearLiveProvider, ExtractProviderCommonConfig,
    ProviderActivationErrorKind, ProviderCommonConfigExtraction, ProviderDoctorReport,
    ProviderDocument, ProviderLiveFileKind, ProviderLiveWorkspace, ProviderModelsResult,
    ProviderMutationGuard, ProviderMutationOutcome, ProviderNetworkFailureKind, ProviderProfile,
    ProviderRollbackOutcome, ProviderTestResult, ProviderWorkspace, SaveLiveFile,
    SaveProviderWorkspace, SwitchProvider, apply_provider_preset, provider_presets,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLoadFailureKind {
    LoadFailed,
    WorkerStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationPhase {
    #[default]
    Idle,
    Running,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSaveFailureKind {
    Conflict,
    Validation,
    SaveFailed,
    WorkerStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionResult {
    Applied,
    GuardRequired,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardResolution {
    Save,
    Discard,
    Stay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardOutcome {
    NeedsSave,
    NeedsLiveSave(ProviderLiveFileKind),
    Applied,
    Stayed,
    NoPendingGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveLoadFailureKind {
    Activation(ProviderActivationErrorKind),
    WorkerStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveMutationKind {
    Switch { target_profile_id: String },
    Reapply,
    Backfill,
    Clear,
    SaveFile(ProviderLiveFileKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMutationRequestResult {
    ConfirmationRequired,
    ConfirmationPending,
    Running,
    DirtyProvider,
    DirtyLiveFile,
    MissingWorkspace,
    InvalidTarget,
}

pub enum LiveMutationCommand {
    Switch(SwitchProvider),
    Reapply(ApplyActiveProvider),
    Backfill(BackfillActiveProvider),
    Clear(ClearLiveProvider),
    SaveFile(SaveLiveFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMutationFailure {
    pub kind: LiveMutationFailureKind,
    pub rollback: ProviderRollbackOutcome,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMutationFailureKind {
    Activation(ProviderActivationErrorKind),
    WorkerStopped,
}

impl LiveMutationFailure {
    pub fn new(
        kind: ProviderActivationErrorKind,
        rollback: ProviderRollbackOutcome,
        backup_path: Option<String>,
    ) -> Self {
        Self {
            kind: LiveMutationFailureKind::Activation(kind),
            rollback,
            backup_path,
        }
    }

    pub fn worker_stopped() -> Self {
        Self {
            kind: LiveMutationFailureKind::WorkerStopped,
            rollback: ProviderRollbackOutcome::NotRequired,
            backup_path: None,
        }
    }
}

pub struct LiveProviderState {
    pub load_phase: ProviderLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<LiveLoadFailureKind>,
    pub workspace: Option<Arc<ProviderLiveWorkspace>>,
    pub mutation_phase: OperationPhase,
    pub current_mutation_request_id: u64,
    pub mutation_kind: Option<LiveMutationKind>,
    pub failure: Option<LiveMutationFailure>,
    pub backup_path: Option<String>,
    pub rollback: ProviderRollbackOutcome,
    confirmation: Option<LiveMutationKind>,
    next_request_id: u64,
    config_draft: String,
    auth_draft: String,
    config_editing: bool,
    auth_editing: bool,
    config_revealed: bool,
    auth_revealed: bool,
}

impl Default for LiveProviderState {
    fn default() -> Self {
        Self {
            load_phase: ProviderLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            workspace: None,
            mutation_phase: OperationPhase::Idle,
            current_mutation_request_id: 0,
            mutation_kind: None,
            failure: None,
            backup_path: None,
            rollback: ProviderRollbackOutcome::NotRequired,
            confirmation: None,
            next_request_id: 0,
            config_draft: String::new(),
            auth_draft: String::new(),
            config_editing: false,
            auth_editing: false,
            config_revealed: false,
            auth_revealed: false,
        }
    }
}

impl fmt::Debug for LiveProviderState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveProviderState")
            .field("load_phase", &self.load_phase)
            .field("mutation_phase", &self.mutation_phase)
            .field("mutation_kind", &self.mutation_kind)
            .field("has_workspace", &self.workspace.is_some())
            .field("config_editing", &self.config_editing)
            .field("auth_editing", &self.auth_editing)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteProfileError {
    NoSelection,
    ActiveProtected,
    LastOrdinary,
    ConfirmationRequired,
    WouldEmptyAggregate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderEditorTab {
    #[default]
    General,
    Models,
    Config,
    Live,
    Diagnostics,
    Routing,
}

#[derive(Debug, Default)]
pub struct SaveOperationState {
    pub phase: OperationPhase,
    pub current_request_id: u64,
    pub error: Option<ProviderSaveFailureKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonConfigExtractionOutcome {
    Applied,
    NoContent,
}

pub struct CommonConfigExtractionState {
    pub phase: OperationPhase,
    pub current_request_id: u64,
    pub outcome: Option<CommonConfigExtractionOutcome>,
    pub error: Option<ProviderSaveFailureKind>,
    submitted_document: Option<ProviderDocument>,
}

impl Default for CommonConfigExtractionState {
    fn default() -> Self {
        Self {
            phase: OperationPhase::Idle,
            current_request_id: 0,
            outcome: None,
            error: None,
            submitted_document: None,
        }
    }
}

impl fmt::Debug for CommonConfigExtractionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommonConfigExtractionState")
            .field("phase", &self.phase)
            .field("current_request_id", &self.current_request_id)
            .field("outcome", &self.outcome)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl CommonConfigExtractionState {
    fn clear_result(&mut self) {
        self.phase = OperationPhase::Idle;
        self.outcome = None;
        self.error = None;
        self.submitted_document = None;
    }
}

pub struct NetworkOperationState<T> {
    pub phase: OperationPhase,
    pub current_request_id: u64,
    pub profile_id: Option<String>,
    pub edit_generation: u64,
    pub result: Option<T>,
    pub error: Option<ProviderNetworkFailureKind>,
}

impl<T> Default for NetworkOperationState<T> {
    fn default() -> Self {
        Self {
            phase: OperationPhase::Idle,
            current_request_id: 0,
            profile_id: None,
            edit_generation: 0,
            result: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationToken {
    pub request_id: u64,
    pub profile_id: String,
    pub edit_generation: u64,
}

impl OperationToken {
    pub fn with_request_id(&self, request_id: u64) -> Self {
        Self {
            request_id,
            ..self.clone()
        }
    }

    pub fn with_profile_id(&self, profile_id: String) -> Self {
        Self {
            profile_id,
            ..self.clone()
        }
    }

    pub fn with_edit_generation(&self, edit_generation: u64) -> Self {
        Self {
            edit_generation,
            ..self.clone()
        }
    }
}

#[derive(Clone)]
enum PendingGuard {
    SelectProfile(String),
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingGuardSave {
    Provider,
    Live(ProviderLiveFileKind),
}

pub struct ProviderViewState {
    pub load_phase: ProviderLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<ProviderLoadFailureKind>,
    pub baseline: Option<Arc<ProviderWorkspace>>,
    draft: Option<ProviderDocument>,
    pub selected_profile_id: Option<String>,
    pub editor_tab: ProviderEditorTab,
    pub list_collapsed: bool,
    pub edit_generation: u64,
    pub save: SaveOperationState,
    pub common_config_extraction: CommonConfigExtractionState,
    pub test: NetworkOperationState<ProviderTestResult>,
    pub models: NetworkOperationState<ProviderModelsResult>,
    pub doctor: NetworkOperationState<ProviderDoctorReport>,
    pub live: LiveProviderState,
    pub secret_revealed: bool,
    pub config_revealed: bool,
    pub auth_revealed: bool,
    pub delete_confirmation_required: bool,
    pending_guard: Option<PendingGuard>,
    pending_guard_save: Option<PendingGuardSave>,
}

impl Default for ProviderViewState {
    fn default() -> Self {
        Self {
            load_phase: ProviderLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            baseline: None,
            draft: None,
            selected_profile_id: None,
            editor_tab: ProviderEditorTab::General,
            list_collapsed: false,
            edit_generation: 0,
            save: SaveOperationState::default(),
            common_config_extraction: CommonConfigExtractionState::default(),
            test: NetworkOperationState::default(),
            models: NetworkOperationState::default(),
            doctor: NetworkOperationState::default(),
            live: LiveProviderState::default(),
            secret_revealed: false,
            config_revealed: false,
            auth_revealed: false,
            delete_confirmation_required: false,
            pending_guard: None,
            pending_guard_save: None,
        }
    }
}

impl fmt::Debug for ProviderViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderViewState")
            .field("load_phase", &self.load_phase)
            .field("selected_profile_id", &self.selected_profile_id)
            .field(
                "profile_count",
                &self.draft.as_ref().map(|draft| draft.profiles.len()),
            )
            .field("edit_generation", &self.edit_generation)
            .field("dirty", &self.is_dirty())
            .field("common_config_extraction", &self.common_config_extraction)
            .finish_non_exhaustive()
    }
}

impl ProviderViewState {
    pub fn begin_load(&mut self) -> u64 {
        self.current_load_request_id = next_id(self.current_load_request_id, "provider load");
        self.load_phase = if self.baseline.is_some() {
            ProviderLoadPhase::Refreshing
        } else {
            ProviderLoadPhase::Loading
        };
        self.current_load_request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderWorkspace>, ProviderLoadFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                let selected = self.selected_profile_id.clone();
                self.draft = Some(workspace.document.clone());
                self.selected_profile_id = selected
                    .filter(|id| document_contains_id(&workspace.document, id))
                    .or_else(|| workspace.activation.active_profile_id.clone())
                    .filter(|id| document_contains_id(&workspace.document, id))
                    .or_else(|| {
                        workspace
                            .document
                            .profiles
                            .first()
                            .map(|profile| profile.id().to_string())
                    });
                self.baseline = Some(workspace);
                self.load_phase = ProviderLoadPhase::Ready;
                self.load_error = None;
                self.edit_generation = 0;
                self.pending_guard = None;
                self.pending_guard_save = None;
                if self.common_config_extraction.phase != OperationPhase::Running {
                    self.common_config_extraction.clear_result();
                }
                self.reset_secret_and_operations();
            }
            Err(error) => {
                self.load_phase = ProviderLoadPhase::Error;
                self.load_error = Some(error);
            }
        }
        true
    }

    pub fn draft(&self) -> Option<&ProviderDocument> {
        self.draft.as_ref()
    }

    pub fn draft_mut(&mut self) -> Option<&mut ProviderDocument> {
        self.draft.as_mut()
    }

    pub fn selected_profile(&self) -> Option<&ProviderProfile> {
        let selected = self.selected_profile_id.as_deref()?;
        self.draft
            .as_ref()?
            .profiles
            .iter()
            .find(|profile| profile.id() == selected)
    }

    pub fn is_dirty(&self) -> bool {
        match (&self.baseline, &self.draft) {
            (Some(baseline), Some(draft)) => baseline.document != *draft,
            _ => false,
        }
    }

    pub fn has_dirty_live_file(&self) -> bool {
        self.dirty_live_file_kind().is_some()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.is_dirty() || self.has_dirty_live_file()
    }

    pub fn edit_selected(&mut self, edit: impl FnOnce(&mut ProviderProfile)) -> bool {
        let Some(selected) = self.selected_profile_id.as_deref() else {
            return false;
        };
        let Some(profile) = self.draft.as_mut().and_then(|draft| {
            draft
                .profiles
                .iter_mut()
                .find(|profile| profile.id() == selected)
        }) else {
            return false;
        };
        edit(profile);
        self.mark_edited();
        true
    }

    pub fn mark_edited(&mut self) {
        self.edit_generation = next_id(self.edit_generation, "provider edit generation");
        reset_network_operation(&mut self.test);
        reset_network_operation(&mut self.models);
        reset_network_operation(&mut self.doctor);
    }

    pub fn discard_draft(&mut self) {
        if let Some(baseline) = &self.baseline {
            self.draft = Some(baseline.document.clone());
            if self
                .selected_profile_id
                .as_deref()
                .is_none_or(|id| !document_contains_id(&baseline.document, id))
            {
                self.selected_profile_id = baseline
                    .document
                    .profiles
                    .first()
                    .map(|profile| profile.id().to_string());
            }
        }
        self.edit_generation = 0;
        self.save = SaveOperationState::default();
        if self.common_config_extraction.phase != OperationPhase::Running {
            self.common_config_extraction.clear_result();
        }
        self.pending_guard = None;
        self.pending_guard_save = None;
        self.reset_secret_and_operations();
    }

    pub fn request_selection(&mut self, profile_id: &str) -> TransitionResult {
        if !self
            .draft
            .as_ref()
            .is_some_and(|draft| document_contains_id(draft, profile_id))
        {
            return TransitionResult::NotFound;
        }
        if self.selected_profile_id.as_deref() == Some(profile_id) {
            return TransitionResult::Applied;
        }
        if self.is_dirty() {
            self.pending_guard = Some(PendingGuard::SelectProfile(profile_id.to_string()));
            return TransitionResult::GuardRequired;
        }
        self.apply_selection(profile_id.to_string());
        TransitionResult::Applied
    }

    pub fn request_reload(&mut self) -> TransitionResult {
        if self.is_dirty() || self.dirty_live_file_kind().is_some() {
            self.pending_guard = Some(PendingGuard::Reload);
            TransitionResult::GuardRequired
        } else {
            TransitionResult::Applied
        }
    }

    pub fn resolve_guard(&mut self, resolution: GuardResolution) -> GuardOutcome {
        if self.pending_guard.is_none() {
            return GuardOutcome::NoPendingGuard;
        }
        match resolution {
            GuardResolution::Stay => {
                self.pending_guard = None;
                self.pending_guard_save = None;
                GuardOutcome::Stayed
            }
            GuardResolution::Discard => {
                let target = self.pending_guard.take();
                self.discard_draft();
                self.reset_live_file_drafts();
                self.apply_guard_target(target);
                GuardOutcome::Applied
            }
            GuardResolution::Save => {
                if self.is_dirty() {
                    self.pending_guard_save = Some(PendingGuardSave::Provider);
                    GuardOutcome::NeedsSave
                } else if let Some(kind) = self.dirty_live_file_kind() {
                    self.pending_guard_save = Some(PendingGuardSave::Live(kind));
                    GuardOutcome::NeedsLiveSave(kind)
                } else {
                    let target = self.pending_guard.take();
                    self.apply_guard_target(target);
                    GuardOutcome::Applied
                }
            }
        }
    }

    pub fn begin_save(&mut self) -> Option<(u64, SaveProviderWorkspace)> {
        let baseline = self.baseline.as_ref()?;
        let draft = self.draft.clone()?;
        if self.save.phase == OperationPhase::Running
            || self.common_config_extraction.phase == OperationPhase::Running
        {
            return None;
        }
        self.save.current_request_id = next_id(self.save.current_request_id, "provider save");
        self.save.phase = OperationPhase::Running;
        self.save.error = None;
        Some((
            self.save.current_request_id,
            SaveProviderWorkspace {
                expected_revision: baseline.revision.clone(),
                document: draft,
            },
        ))
    }

    pub fn apply_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderWorkspace>, ProviderSaveFailureKind>,
    ) -> bool {
        if request_id != self.save.current_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                let selected = self.selected_profile_id.clone();
                self.draft = Some(workspace.document.clone());
                self.selected_profile_id = selected
                    .filter(|id| document_contains_id(&workspace.document, id))
                    .or_else(|| {
                        workspace
                            .document
                            .profiles
                            .first()
                            .map(|profile| profile.id().to_string())
                    });
                self.baseline = Some(workspace);
                self.save.phase = OperationPhase::Ready;
                self.save.error = None;
                self.edit_generation = 0;
                self.reset_secret_and_operations();
                self.sync_live_provider_workspace();
                if self.pending_guard_save == Some(PendingGuardSave::Provider) {
                    let target = self.pending_guard.take();
                    self.pending_guard_save = None;
                    self.apply_guard_target(target);
                }
            }
            Err(error) => {
                self.save.phase = OperationPhase::Error;
                self.save.error = Some(error);
                self.pending_guard_save = None;
            }
        }
        true
    }

    pub fn begin_common_config_extraction(&mut self) -> Option<(u64, ExtractProviderCommonConfig)> {
        if self.common_config_extraction.phase == OperationPhase::Running
            || self.save.phase == OperationPhase::Running
            || self.live.mutation_phase == OperationPhase::Running
            || matches!(
                self.live.load_phase,
                ProviderLoadPhase::Loading | ProviderLoadPhase::Refreshing
            )
            || self.live.confirmation.is_some()
        {
            return None;
        }
        let baseline = self.baseline.as_ref()?;
        let document = self.draft.clone()?;
        let profile_id = self.selected_profile_id.clone()?;
        if !document.profiles.iter().any(|profile| {
            profile.id() == profile_id
                && profile.kind() == codex_plus_manager_service::ProviderKind::Ordinary
        }) {
            return None;
        }

        self.common_config_extraction.current_request_id = next_id(
            self.common_config_extraction.current_request_id,
            "provider common config extraction",
        );
        self.common_config_extraction.phase = OperationPhase::Running;
        self.common_config_extraction.outcome = None;
        self.common_config_extraction.error = None;
        self.common_config_extraction.submitted_document = Some(document.clone());
        Some((
            self.common_config_extraction.current_request_id,
            ExtractProviderCommonConfig {
                expected_revision: baseline.revision.clone(),
                document,
                profile_id,
            },
        ))
    }

    pub fn apply_common_config_extraction_response(
        &mut self,
        request_id: u64,
        result: Result<ProviderCommonConfigExtraction, ProviderSaveFailureKind>,
    ) -> bool {
        if request_id != self.common_config_extraction.current_request_id
            || self.common_config_extraction.phase != OperationPhase::Running
        {
            return false;
        }

        let submitted = self.common_config_extraction.submitted_document.take();
        match result {
            Ok(ProviderCommonConfigExtraction::Applied(workspace)) => {
                let workspace = *workspace;
                let draft_unchanged = submitted
                    .as_ref()
                    .is_some_and(|submitted| self.draft.as_ref() == Some(submitted));
                let workspace = Arc::new(workspace);
                if draft_unchanged {
                    let selected = self.selected_profile_id.clone();
                    self.draft = Some(workspace.document.clone());
                    self.selected_profile_id = selected
                        .filter(|id| document_contains_id(&workspace.document, id))
                        .or_else(|| {
                            workspace
                                .document
                                .profiles
                                .first()
                                .map(|profile| profile.id().to_string())
                        });
                    self.edit_generation = 0;
                    self.reset_secret_and_operations();
                }
                self.baseline = Some(workspace);
                self.sync_live_provider_workspace();
                self.common_config_extraction.phase = OperationPhase::Ready;
                self.common_config_extraction.outcome =
                    Some(CommonConfigExtractionOutcome::Applied);
                self.common_config_extraction.error = None;
            }
            Ok(ProviderCommonConfigExtraction::NoContent) => {
                self.common_config_extraction.phase = OperationPhase::Ready;
                self.common_config_extraction.outcome =
                    Some(CommonConfigExtractionOutcome::NoContent);
                self.common_config_extraction.error = None;
            }
            Err(error) => {
                self.common_config_extraction.phase = OperationPhase::Error;
                self.common_config_extraction.outcome = None;
                self.common_config_extraction.error = Some(error);
            }
        }
        true
    }

    pub fn begin_live_load(&mut self) -> Option<u64> {
        if self.is_dirty()
            || self.dirty_live_file_kind().is_some()
            || self.live.mutation_phase == OperationPhase::Running
            || self.common_config_extraction.phase == OperationPhase::Running
        {
            return None;
        }
        self.live.next_request_id = next_id(self.live.next_request_id, "provider live request");
        self.live.current_load_request_id = self.live.next_request_id;
        self.live.load_phase = if self.live.workspace.is_some() {
            ProviderLoadPhase::Refreshing
        } else {
            ProviderLoadPhase::Loading
        };
        self.load_phase = if self.baseline.is_some() {
            ProviderLoadPhase::Refreshing
        } else {
            ProviderLoadPhase::Loading
        };
        Some(self.live.current_load_request_id)
    }

    pub fn apply_live_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderLiveWorkspace>, LiveLoadFailureKind>,
    ) -> bool {
        if request_id != self.live.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.install_live_workspace(workspace);
                self.pending_guard = None;
                self.pending_guard_save = None;
                self.live.load_phase = ProviderLoadPhase::Ready;
                self.live.load_error = None;
                self.load_phase = ProviderLoadPhase::Ready;
                self.load_error = None;
            }
            Err(error) => {
                self.live.load_phase = ProviderLoadPhase::Error;
                self.live.load_error = Some(error);
                self.load_phase = ProviderLoadPhase::Error;
                if self.baseline.is_none() {
                    self.load_error = Some(match error {
                        LiveLoadFailureKind::Activation(_) => ProviderLoadFailureKind::LoadFailed,
                        LiveLoadFailureKind::WorkerStopped => {
                            ProviderLoadFailureKind::WorkerStopped
                        }
                    });
                }
            }
        }
        true
    }

    pub fn request_live_mutation(&mut self, kind: LiveMutationKind) -> LiveMutationRequestResult {
        if self.live.mutation_phase == OperationPhase::Running
            || self.common_config_extraction.phase == OperationPhase::Running
        {
            return LiveMutationRequestResult::Running;
        }
        if self.live.confirmation.is_some() {
            return LiveMutationRequestResult::ConfirmationPending;
        }
        let Some(workspace) = self.live.workspace.as_ref() else {
            return LiveMutationRequestResult::MissingWorkspace;
        };
        if self.is_dirty() {
            return LiveMutationRequestResult::DirtyProvider;
        }
        match &kind {
            LiveMutationKind::SaveFile(file_kind) => {
                if !self.live_file_dirty(*file_kind) {
                    return LiveMutationRequestResult::InvalidTarget;
                }
            }
            LiveMutationKind::Switch { target_profile_id } => {
                if self.dirty_live_file_kind().is_some() {
                    return LiveMutationRequestResult::DirtyLiveFile;
                }
                if workspace.provider.activation.active_profile_id.as_deref()
                    == Some(target_profile_id.as_str())
                    || !workspace
                        .provider
                        .document
                        .profiles
                        .iter()
                        .any(|profile| profile.id() == target_profile_id)
                {
                    return LiveMutationRequestResult::InvalidTarget;
                }
            }
            LiveMutationKind::Reapply | LiveMutationKind::Backfill | LiveMutationKind::Clear => {
                if self.dirty_live_file_kind().is_some() {
                    return LiveMutationRequestResult::DirtyLiveFile;
                }
            }
        }
        self.live.confirmation = Some(kind);
        LiveMutationRequestResult::ConfirmationRequired
    }

    pub fn pending_live_confirmation(&self) -> Option<&LiveMutationKind> {
        self.live.confirmation.as_ref()
    }

    pub fn cancel_live_confirmation(&mut self) {
        self.live.confirmation = None;
    }

    pub fn confirm_live_mutation(&mut self) -> Option<(u64, LiveMutationCommand)> {
        if self.live.mutation_phase == OperationPhase::Running
            || self.common_config_extraction.phase == OperationPhase::Running
            || self.is_dirty()
        {
            return None;
        }
        let kind = self.live.confirmation.take()?;
        let workspace = self.live.workspace.as_ref()?;
        let guard = ProviderMutationGuard {
            expected_provider_revision: workspace.provider.revision.clone(),
            expected_live_revision: workspace.revision.clone(),
        };
        let command = match &kind {
            LiveMutationKind::Switch { target_profile_id } => {
                LiveMutationCommand::Switch(SwitchProvider {
                    guard,
                    target_profile_id: target_profile_id.clone(),
                })
            }
            LiveMutationKind::Reapply => {
                LiveMutationCommand::Reapply(ApplyActiveProvider { guard })
            }
            LiveMutationKind::Backfill => {
                LiveMutationCommand::Backfill(BackfillActiveProvider { guard })
            }
            LiveMutationKind::Clear => LiveMutationCommand::Clear(ClearLiveProvider { guard }),
            LiveMutationKind::SaveFile(file_kind) => {
                let contents = match file_kind {
                    ProviderLiveFileKind::Config => self.live.config_draft.clone(),
                    ProviderLiveFileKind::Auth => self.live.auth_draft.clone(),
                };
                LiveMutationCommand::SaveFile(SaveLiveFile {
                    guard,
                    kind: *file_kind,
                    contents,
                })
            }
        };
        self.live.next_request_id = next_id(self.live.next_request_id, "provider live request");
        self.live.current_mutation_request_id = self.live.next_request_id;
        self.live.mutation_phase = OperationPhase::Running;
        self.live.mutation_kind = Some(kind);
        self.live.failure = None;
        self.live.backup_path = None;
        self.live.rollback = ProviderRollbackOutcome::NotRequired;
        Some((self.live.current_mutation_request_id, command))
    }

    pub fn apply_live_mutation_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ProviderMutationOutcome>, LiveMutationFailure>,
    ) -> bool {
        if request_id != self.live.current_mutation_request_id {
            return false;
        }
        match result {
            Ok(outcome) => {
                let switched_target = match &self.live.mutation_kind {
                    Some(LiveMutationKind::Switch { target_profile_id }) => {
                        Some(target_profile_id.clone())
                    }
                    _ => None,
                };
                self.live.backup_path = outcome.backup_path.clone();
                self.live.rollback = outcome.rollback;
                self.live.failure = None;
                self.live.mutation_phase = OperationPhase::Ready;
                self.install_live_workspace(Arc::new(outcome.live.clone()));
                if let Some(target_profile_id) = switched_target.filter(|target| {
                    self.draft
                        .as_ref()
                        .is_some_and(|draft| document_contains_id(draft, target))
                }) {
                    self.apply_selection(target_profile_id);
                }
                if matches!(self.pending_guard_save, Some(PendingGuardSave::Live(_))) {
                    let target = self.pending_guard.take();
                    self.pending_guard_save = None;
                    self.apply_guard_target(target);
                }
            }
            Err(failure) => {
                self.live.backup_path = failure.backup_path.clone();
                self.live.rollback = failure.rollback;
                self.live.failure = Some(failure);
                self.live.mutation_phase = OperationPhase::Error;
                self.pending_guard_save = None;
            }
        }
        true
    }

    pub fn begin_live_file_edit(&mut self, kind: ProviderLiveFileKind) -> bool {
        if self.live.workspace.is_none()
            || self.live.mutation_phase == OperationPhase::Running
            || self.live.config_editing
            || self.live.auth_editing
        {
            return false;
        }
        self.reset_live_file_draft(kind);
        match kind {
            ProviderLiveFileKind::Config => self.live.config_editing = true,
            ProviderLiveFileKind::Auth => self.live.auth_editing = true,
        }
        true
    }

    pub fn edit_live_file(&mut self, kind: ProviderLiveFileKind, contents: String) -> bool {
        match kind {
            ProviderLiveFileKind::Config if self.live.config_editing => {
                self.live.config_draft = contents;
                true
            }
            ProviderLiveFileKind::Auth if self.live.auth_editing => {
                self.live.auth_draft = contents;
                true
            }
            _ => false,
        }
    }

    pub fn cancel_live_file_edit(&mut self, kind: ProviderLiveFileKind) {
        self.reset_live_file_draft(kind);
        match kind {
            ProviderLiveFileKind::Config => {
                self.live.config_editing = false;
                self.live.config_revealed = false;
            }
            ProviderLiveFileKind::Auth => {
                self.live.auth_editing = false;
                self.live.auth_revealed = false;
            }
        }
        if self.pending_guard_save == Some(PendingGuardSave::Live(kind)) {
            self.pending_guard_save = None;
        }
    }

    pub fn live_file_draft(&self, kind: ProviderLiveFileKind) -> Option<&str> {
        self.live.workspace.as_ref()?;
        Some(match kind {
            ProviderLiveFileKind::Config => self.live.config_draft.as_str(),
            ProviderLiveFileKind::Auth => self.live.auth_draft.as_str(),
        })
    }

    pub fn live_file_dirty(&self, kind: ProviderLiveFileKind) -> bool {
        let Some(workspace) = self.live.workspace.as_ref() else {
            return false;
        };
        match kind {
            ProviderLiveFileKind::Config => {
                self.live.config_editing
                    && self.live.config_draft != workspace.files.config_contents
            }
            ProviderLiveFileKind::Auth => {
                self.live.auth_editing && self.live.auth_draft != workspace.files.auth_contents
            }
        }
    }

    pub fn live_file_editing(&self, kind: ProviderLiveFileKind) -> bool {
        match kind {
            ProviderLiveFileKind::Config => self.live.config_editing,
            ProviderLiveFileKind::Auth => self.live.auth_editing,
        }
    }

    pub fn set_live_file_revealed(&mut self, kind: ProviderLiveFileKind, revealed: bool) {
        match kind {
            ProviderLiveFileKind::Config => self.live.config_revealed = revealed,
            ProviderLiveFileKind::Auth => self.live.auth_revealed = revealed,
        }
    }

    pub fn live_file_revealed(&self, kind: ProviderLiveFileKind) -> bool {
        match kind {
            ProviderLiveFileKind::Config => self.live.config_revealed,
            ProviderLiveFileKind::Auth => self.live.auth_revealed,
        }
    }

    pub fn leave_provider_route(&mut self) {
        self.live.confirmation = None;
        self.reset_live_file_drafts();
    }

    pub fn set_secret_revealed(&mut self, revealed: bool) {
        self.secret_revealed = revealed;
    }

    pub fn set_config_revealed(&mut self, revealed: bool) {
        self.config_revealed = revealed;
    }

    pub fn set_auth_revealed(&mut self, revealed: bool) {
        self.auth_revealed = revealed;
    }

    pub fn has_pending_guard(&self) -> bool {
        self.pending_guard.is_some()
    }

    pub fn selected_is_active(&self) -> bool {
        self.selected_profile_id.as_deref()
            == self
                .baseline
                .as_ref()
                .and_then(|workspace| workspace.activation.active_profile_id.as_deref())
    }

    pub fn set_delete_confirmation_required(&mut self, required: bool) {
        self.delete_confirmation_required = required;
    }

    pub fn apply_preset(&mut self, preset_id: &str) -> bool {
        let Ok(presets) = provider_presets() else {
            return false;
        };
        let Some(preset) = presets.iter().find(|preset| preset.id == preset_id) else {
            return false;
        };
        self.edit_selected(|profile| {
            if let Some(profile) = profile.ordinary_mut() {
                apply_provider_preset(profile, preset);
            }
        })
    }

    pub fn update_model_row(&mut self, index: usize, model: &str, window: &str) -> bool {
        let valid = self
            .selected_profile()
            .and_then(ProviderProfile::ordinary)
            .is_some_and(|profile| index < profile_model_rows(profile).len());
        if !valid {
            return false;
        }
        self.edit_selected(|profile| {
            let profile = profile.ordinary_mut().expect("ordinary profile checked");
            let mut rows = profile_model_rows(profile);
            rows[index] = (model.trim().to_string(), window.trim().to_string());
            write_profile_model_rows(profile, rows);
        })
    }

    pub fn add_model_row(&mut self) -> bool {
        if self
            .selected_profile()
            .and_then(ProviderProfile::ordinary)
            .is_none()
        {
            return false;
        }
        self.edit_selected(|profile| {
            let profile = profile.ordinary_mut().expect("ordinary profile checked");
            let mut rows = profile_model_rows(profile);
            let names = rows
                .iter()
                .map(|(model, _)| model.as_str())
                .collect::<HashSet<_>>();
            let mut suffix = 1usize;
            let model = loop {
                let candidate = if suffix == 1 {
                    "new-model".to_string()
                } else {
                    format!("new-model-{suffix}")
                };
                if !names.contains(candidate.as_str()) {
                    break candidate;
                }
                suffix += 1;
            };
            rows.push((model, String::new()));
            write_profile_model_rows(profile, rows);
        })
    }

    pub fn remove_model_row(&mut self, index: usize) -> bool {
        let valid = self
            .selected_profile()
            .and_then(ProviderProfile::ordinary)
            .is_some_and(|profile| index < profile_model_rows(profile).len());
        if !valid {
            return false;
        }
        self.edit_selected(|profile| {
            let profile = profile.ordinary_mut().expect("ordinary profile checked");
            let mut rows = profile_model_rows(profile);
            rows.remove(index);
            write_profile_model_rows(profile, rows);
        })
    }

    pub fn merge_discovered_models(&mut self) -> bool {
        let Some(discovered) = self
            .models
            .result
            .as_ref()
            .map(|result| result.models.clone())
        else {
            return false;
        };
        if self
            .selected_profile()
            .and_then(ProviderProfile::ordinary)
            .is_none()
        {
            return false;
        }
        self.edit_selected(|profile| {
            let profile = profile.ordinary_mut().expect("ordinary profile checked");
            let mut rows = profile_model_rows(profile);
            let mut names = rows
                .iter()
                .map(|(model, _)| model.clone())
                .collect::<HashSet<_>>();
            for model in discovered {
                let model = model.trim();
                if !model.is_empty() && names.insert(model.to_string()) {
                    rows.push((model.to_string(), String::new()));
                }
            }
            write_profile_model_rows(profile, rows);
        })
    }

    pub fn set_aggregate_member(&mut self, profile_id: &str, enabled: bool) -> bool {
        let ordinary_exists = self.draft.as_ref().is_some_and(|document| {
            document.profiles.iter().any(|profile| {
                profile.id() == profile_id
                    && profile.kind() == codex_plus_manager_service::ProviderKind::Ordinary
            })
        });
        let selected_is_aggregate = matches!(
            self.selected_profile(),
            Some(ProviderProfile::Aggregate { .. })
        );
        if !ordinary_exists || !selected_is_aggregate {
            return false;
        }
        self.edit_selected(|profile| {
            let ProviderProfile::Aggregate { routing, .. } = profile else {
                unreachable!("aggregate profile checked")
            };
            if enabled {
                if !routing
                    .members
                    .iter()
                    .any(|member| member.relay_id == profile_id)
                {
                    routing.members.push(AggregateRelayMember {
                        relay_id: profile_id.to_string(),
                        weight: 1,
                    });
                }
            } else {
                routing
                    .members
                    .retain(|member| member.relay_id != profile_id);
            }
        })
    }

    pub fn set_aggregate_weight(&mut self, profile_id: &str, weight: u32) -> bool {
        let valid = matches!(
            self.selected_profile(),
            Some(ProviderProfile::Aggregate { routing, .. })
                if routing.members.iter().any(|member| member.relay_id == profile_id)
        );
        if !valid {
            return false;
        }
        self.edit_selected(|profile| {
            let ProviderProfile::Aggregate { routing, .. } = profile else {
                unreachable!("aggregate profile checked")
            };
            if let Some(member) = routing
                .members
                .iter_mut()
                .find(|member| member.relay_id == profile_id)
            {
                member.weight = weight.clamp(1, 1_000);
            }
        })
    }

    pub fn add_ordinary(&mut self) -> String {
        let id = self.unique_id("provider");
        let profile = RelayProfile {
            id: id.clone(),
            name: "New provider".to_string(),
            relay_mode: RelayMode::MixedApi,
            ..RelayProfile::default()
        };
        if let Some(draft) = &mut self.draft {
            draft.profiles.push(ProviderProfile::Ordinary(profile));
        }
        self.apply_selection(id.clone());
        self.mark_edited();
        id
    }

    pub fn add_aggregate(&mut self) -> String {
        let id = self.unique_id("aggregate");
        let first_member = self.draft.as_ref().and_then(|draft| {
            draft.profiles.iter().find_map(|profile| match profile {
                ProviderProfile::Ordinary(profile) => Some(profile.id.clone()),
                ProviderProfile::Aggregate { .. } => None,
            })
        });
        let shell = RelayProfile {
            id: id.clone(),
            name: "New aggregate".to_string(),
            relay_mode: RelayMode::Aggregate,
            ..RelayProfile::default()
        };
        let routing = AggregateRelayProfile {
            id: id.clone(),
            name: shell.name.clone(),
            strategy: Default::default(),
            members: first_member
                .map(|relay_id| {
                    vec![AggregateRelayMember {
                        relay_id,
                        weight: 1,
                    }]
                })
                .unwrap_or_default(),
        };
        if let Some(draft) = &mut self.draft {
            draft
                .profiles
                .push(ProviderProfile::Aggregate { shell, routing });
        }
        self.apply_selection(id.clone());
        self.mark_edited();
        id
    }

    pub fn duplicate_selected(&mut self) -> Option<String> {
        let selected = self.selected_profile()?.clone();
        let id = self.unique_id("provider-copy");
        let duplicate = match selected {
            ProviderProfile::Ordinary(mut profile) => {
                profile.id = id.clone();
                profile.name = format!("{} Copy", profile.name.trim());
                ProviderProfile::Ordinary(profile)
            }
            ProviderProfile::Aggregate {
                mut shell,
                mut routing,
            } => {
                shell.id = id.clone();
                shell.name = format!("{} Copy", shell.name.trim());
                routing.id = id.clone();
                routing.name = shell.name.clone();
                ProviderProfile::Aggregate { shell, routing }
            }
        };
        let selected_index = self.selected_index()?;
        self.draft
            .as_mut()?
            .profiles
            .insert(selected_index + 1, duplicate);
        self.apply_selection(id.clone());
        self.mark_edited();
        Some(id)
    }

    pub fn move_selected(&mut self, direction: ListDirection) -> bool {
        let Some(index) = self.selected_index() else {
            return false;
        };
        let Some(draft) = &mut self.draft else {
            return false;
        };
        let target = match direction {
            ListDirection::Up if index > 0 => index - 1,
            ListDirection::Down if index + 1 < draft.profiles.len() => index + 1,
            _ => return false,
        };
        draft.profiles.swap(index, target);
        self.mark_edited();
        true
    }

    pub fn delete_selected(
        &mut self,
        confirm_reference_removal: bool,
    ) -> Result<(), DeleteProfileError> {
        let selected_id = self
            .selected_profile_id
            .clone()
            .ok_or(DeleteProfileError::NoSelection)?;
        if self
            .baseline
            .as_ref()
            .and_then(|workspace| workspace.activation.active_profile_id.as_deref())
            == Some(selected_id.as_str())
        {
            return Err(DeleteProfileError::ActiveProtected);
        }
        let selected = self
            .selected_profile()
            .ok_or(DeleteProfileError::NoSelection)?;
        if selected.kind() == codex_plus_manager_service::ProviderKind::Ordinary {
            let ordinary_count = self
                .draft
                .as_ref()
                .map(|draft| {
                    draft
                        .profiles
                        .iter()
                        .filter(|profile| {
                            profile.kind() == codex_plus_manager_service::ProviderKind::Ordinary
                        })
                        .count()
                })
                .unwrap_or_default();
            if ordinary_count <= 1 {
                return Err(DeleteProfileError::LastOrdinary);
            }
            let referencing = self
                .draft
                .as_ref()
                .into_iter()
                .flat_map(|draft| &draft.profiles)
                .filter_map(|profile| match profile {
                    ProviderProfile::Aggregate { routing, .. }
                        if routing
                            .members
                            .iter()
                            .any(|member| member.relay_id == selected_id) =>
                    {
                        Some(routing)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if referencing.iter().any(|routing| {
                routing
                    .members
                    .iter()
                    .filter(|member| member.relay_id != selected_id)
                    .count()
                    == 0
            }) {
                return Err(DeleteProfileError::WouldEmptyAggregate);
            }
            if !referencing.is_empty() && !confirm_reference_removal {
                return Err(DeleteProfileError::ConfirmationRequired);
            }
            if confirm_reference_removal && let Some(draft) = &mut self.draft {
                for profile in &mut draft.profiles {
                    if let ProviderProfile::Aggregate { routing, .. } = profile {
                        routing
                            .members
                            .retain(|member| member.relay_id != selected_id);
                    }
                }
            }
        }

        let index = self
            .selected_index()
            .ok_or(DeleteProfileError::NoSelection)?;
        let draft = self.draft.as_mut().ok_or(DeleteProfileError::NoSelection)?;
        draft.profiles.remove(index);
        self.selected_profile_id = draft
            .profiles
            .get(index.min(draft.profiles.len().saturating_sub(1)))
            .map(|profile| profile.id().to_string());
        self.mark_edited();
        self.reset_secret_and_operations();
        Ok(())
    }

    pub fn begin_test(&mut self) -> Option<OperationToken> {
        begin_network_operation(
            &mut self.test,
            self.selected_profile_id.as_deref()?,
            self.edit_generation,
        )
    }

    pub fn begin_models(&mut self) -> Option<OperationToken> {
        begin_network_operation(
            &mut self.models,
            self.selected_profile_id.as_deref()?,
            self.edit_generation,
        )
    }

    pub fn begin_doctor(&mut self) -> Option<OperationToken> {
        begin_network_operation(
            &mut self.doctor,
            self.selected_profile_id.as_deref()?,
            self.edit_generation,
        )
    }

    pub fn apply_test_response(
        &mut self,
        token: OperationToken,
        result: Result<ProviderTestResult, ProviderNetworkFailureKind>,
    ) -> bool {
        apply_network_response(
            &mut self.test,
            &self.selected_profile_id,
            self.edit_generation,
            token,
            result,
        )
    }

    pub fn apply_models_response(
        &mut self,
        token: OperationToken,
        result: Result<ProviderModelsResult, ProviderNetworkFailureKind>,
    ) -> bool {
        apply_network_response(
            &mut self.models,
            &self.selected_profile_id,
            self.edit_generation,
            token,
            result,
        )
    }

    pub fn apply_models_failure(
        &mut self,
        token: OperationToken,
        error: ProviderNetworkFailureKind,
    ) -> bool {
        self.apply_models_response(token, Err(error))
    }

    pub fn apply_doctor_response(
        &mut self,
        token: OperationToken,
        result: Result<ProviderDoctorReport, ProviderNetworkFailureKind>,
    ) -> bool {
        apply_network_response(
            &mut self.doctor,
            &self.selected_profile_id,
            self.edit_generation,
            token,
            result,
        )
    }

    pub fn apply_doctor_failure(
        &mut self,
        token: OperationToken,
        error: ProviderNetworkFailureKind,
    ) -> bool {
        self.apply_doctor_response(token, Err(error))
    }

    fn install_live_workspace(&mut self, workspace: Arc<ProviderLiveWorkspace>) {
        let provider = Arc::new(workspace.provider.clone());
        let selected = self.selected_profile_id.clone();
        self.draft = Some(provider.document.clone());
        self.selected_profile_id = selected
            .filter(|id| document_contains_id(&provider.document, id))
            .or_else(|| provider.activation.active_profile_id.clone())
            .filter(|id| document_contains_id(&provider.document, id))
            .or_else(|| {
                provider
                    .document
                    .profiles
                    .first()
                    .map(|profile| profile.id().to_string())
            });
        self.baseline = Some(provider);
        self.live.workspace = Some(workspace);
        self.edit_generation = 0;
        self.reset_secret_and_operations();
        self.reset_live_file_drafts();
    }

    fn sync_live_provider_workspace(&mut self) {
        let (Some(live), Some(provider)) = (&self.live.workspace, &self.baseline) else {
            return;
        };
        let mut next = (**live).clone();
        next.provider = (**provider).clone();
        self.live.workspace = Some(Arc::new(next));
    }

    fn dirty_live_file_kind(&self) -> Option<ProviderLiveFileKind> {
        [ProviderLiveFileKind::Config, ProviderLiveFileKind::Auth]
            .into_iter()
            .find(|kind| self.live_file_dirty(*kind))
    }

    fn reset_live_file_draft(&mut self, kind: ProviderLiveFileKind) {
        let contents = self.live.workspace.as_ref().map(|workspace| match kind {
            ProviderLiveFileKind::Config => workspace.files.config_contents.clone(),
            ProviderLiveFileKind::Auth => workspace.files.auth_contents.clone(),
        });
        if let Some(contents) = contents {
            match kind {
                ProviderLiveFileKind::Config => self.live.config_draft = contents,
                ProviderLiveFileKind::Auth => self.live.auth_draft = contents,
            }
        }
    }

    fn reset_live_file_drafts(&mut self) {
        self.reset_live_file_draft(ProviderLiveFileKind::Config);
        self.reset_live_file_draft(ProviderLiveFileKind::Auth);
        self.live.config_editing = false;
        self.live.auth_editing = false;
        self.live.config_revealed = false;
        self.live.auth_revealed = false;
    }

    fn unique_id(&self, prefix: &str) -> String {
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{prefix}-{suffix}");
            if self
                .draft
                .as_ref()
                .is_none_or(|draft| !document_contains_id(draft, &candidate))
            {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_profile_id.as_deref()?;
        self.draft
            .as_ref()?
            .profiles
            .iter()
            .position(|profile| profile.id() == selected)
    }

    fn apply_selection(&mut self, profile_id: String) {
        self.selected_profile_id = Some(profile_id);
        self.editor_tab = ProviderEditorTab::General;
        self.reset_secret_and_operations();
    }

    fn apply_guard_target(&mut self, target: Option<PendingGuard>) {
        match target {
            Some(PendingGuard::SelectProfile(profile_id)) => self.apply_selection(profile_id),
            Some(PendingGuard::Reload) | None => {}
        }
    }

    fn reset_secret_and_operations(&mut self) {
        self.secret_revealed = false;
        self.config_revealed = false;
        self.auth_revealed = false;
        self.delete_confirmation_required = false;
        reset_network_operation(&mut self.test);
        reset_network_operation(&mut self.models);
        reset_network_operation(&mut self.doctor);
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}

fn document_contains_id(document: &ProviderDocument, id: &str) -> bool {
    document.profiles.iter().any(|profile| profile.id() == id)
}

fn profile_model_rows(profile: &RelayProfile) -> Vec<(String, String)> {
    let windows = serde_json::from_str::<BTreeMap<String, String>>(&profile.model_windows)
        .unwrap_or_default();
    profile
        .model_list
        .lines()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(|model| {
            (
                model.to_string(),
                windows.get(model).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

fn write_profile_model_rows(profile: &mut RelayProfile, rows: Vec<(String, String)>) {
    let mut models = Vec::new();
    let mut windows = BTreeMap::new();
    for (model, window) in rows {
        let model = model.trim();
        if model.is_empty() {
            continue;
        }
        models.push(model.to_string());
        let window = window.trim();
        if !window.is_empty() {
            windows.insert(model.to_string(), window.to_string());
        }
    }
    profile.model_list = models.join("\n");
    profile.model_windows = serde_json::to_string(&windows).unwrap_or_default();
}

fn reset_network_operation<T>(operation: &mut NetworkOperationState<T>) {
    operation.phase = OperationPhase::Idle;
    operation.profile_id = None;
    operation.result = None;
    operation.error = None;
}

fn begin_network_operation<T>(
    operation: &mut NetworkOperationState<T>,
    profile_id: &str,
    edit_generation: u64,
) -> Option<OperationToken> {
    operation.current_request_id = next_id(operation.current_request_id, "provider network");
    operation.phase = OperationPhase::Running;
    operation.profile_id = Some(profile_id.to_string());
    operation.edit_generation = edit_generation;
    operation.result = None;
    operation.error = None;
    Some(OperationToken {
        request_id: operation.current_request_id,
        profile_id: profile_id.to_string(),
        edit_generation,
    })
}

fn apply_network_response<T>(
    operation: &mut NetworkOperationState<T>,
    selected_profile_id: &Option<String>,
    edit_generation: u64,
    token: OperationToken,
    result: Result<T, ProviderNetworkFailureKind>,
) -> bool {
    if token.request_id != operation.current_request_id
        || operation.profile_id.as_deref() != Some(token.profile_id.as_str())
        || operation.edit_generation != token.edit_generation
        || selected_profile_id.as_deref() != Some(token.profile_id.as_str())
        || edit_generation != token.edit_generation
    {
        return false;
    }
    match result {
        Ok(result) => {
            operation.phase = OperationPhase::Ready;
            operation.result = Some(result);
            operation.error = None;
        }
        Err(error) => {
            operation.phase = OperationPhase::Error;
            operation.result = None;
            operation.error = Some(error);
        }
    }
    true
}
