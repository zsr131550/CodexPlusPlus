use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    ConfirmedSecretClear, ExtraArgsRevision, ExtraArgsSettings, ImageOverlayFitMode,
    ImageOverlayRevision, ImageOverlaySettings, ManagerSettingsError, ManagerSettingsErrorKind,
    ManagerSettingsWorkspace, PrivateArgument, PrivatePath, PrivateUrl, ResetExtraArgs,
    ResetImageOverlaySettings, ResetStepwiseSettings, RevisionedExtraArgs,
    RevisionedImageOverlaySettings, RevisionedStepwiseSettings, SafeSettingsGroup, SaveExtraArgs,
    SaveImageOverlaySettings, SaveStepwiseSettings, SecretReplacement, StepwiseRevision,
    StepwiseSecretChange, StepwiseSettings, StepwiseSettingsInput, StepwiseTestOutcome,
    TestStepwiseSettings,
};

use crate::path_picker::{
    PathPickerErrorKind, PathPickerRequest, PathPickerResponse, PathPickerTarget,
};

use super::Route;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Stepwise,
    ImageOverlay,
    LaunchArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsOperationPhase {
    #[default]
    Idle,
    Running,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFailureKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    InvalidUrl,
    InvalidEnvironmentVariable,
    InvalidModel,
    InvalidNumericField,
    InvalidPath,
    InvalidFitMode,
    InvalidArgument,
    InvalidSecret,
    ConfirmationMismatch,
    StepwiseUnauthorized,
    StepwiseTimeout,
    StepwiseRejected,
    StepwiseNetwork,
    WorkerStopped,
}

impl From<ManagerSettingsErrorKind> for SettingsFailureKind {
    fn from(kind: ManagerSettingsErrorKind) -> Self {
        match kind {
            ManagerSettingsErrorKind::SettingsReadFailed => Self::SettingsReadFailed,
            ManagerSettingsErrorKind::SettingsWriteFailed => Self::SettingsWriteFailed,
            ManagerSettingsErrorKind::SettingsConflict => Self::SettingsConflict,
            ManagerSettingsErrorKind::InvalidRevision => Self::InvalidRevision,
            ManagerSettingsErrorKind::InvalidUrl => Self::InvalidUrl,
            ManagerSettingsErrorKind::InvalidEnvironmentVariable => {
                Self::InvalidEnvironmentVariable
            }
            ManagerSettingsErrorKind::InvalidModel => Self::InvalidModel,
            ManagerSettingsErrorKind::InvalidNumericField => Self::InvalidNumericField,
            ManagerSettingsErrorKind::InvalidPath => Self::InvalidPath,
            ManagerSettingsErrorKind::InvalidFitMode => Self::InvalidFitMode,
            ManagerSettingsErrorKind::InvalidArgument => Self::InvalidArgument,
            ManagerSettingsErrorKind::InvalidSecret => Self::InvalidSecret,
            ManagerSettingsErrorKind::ConfirmationMismatch => Self::ConfirmationMismatch,
            ManagerSettingsErrorKind::StepwiseUnauthorized => Self::StepwiseUnauthorized,
            ManagerSettingsErrorKind::StepwiseTimeout => Self::StepwiseTimeout,
            ManagerSettingsErrorKind::StepwiseRejected => Self::StepwiseRejected,
            ManagerSettingsErrorKind::StepwiseNetwork => Self::StepwiseNetwork,
            ManagerSettingsErrorKind::WorkerStopped => Self::WorkerStopped,
        }
    }
}

#[derive(Clone)]
pub struct SettingsFailure {
    pub kind: SettingsFailureKind,
    pub group: Option<SafeSettingsGroup>,
    pub refreshed_workspace: Option<Arc<ManagerSettingsWorkspace>>,
}

impl SettingsFailure {
    pub fn new(kind: SettingsFailureKind, group: Option<SafeSettingsGroup>) -> Self {
        Self {
            kind,
            group,
            refreshed_workspace: None,
        }
    }

    pub fn with_workspace(
        kind: SettingsFailureKind,
        group: SafeSettingsGroup,
        workspace: Arc<ManagerSettingsWorkspace>,
    ) -> Self {
        Self {
            kind,
            group: Some(group),
            refreshed_workspace: Some(workspace),
        }
    }

    pub fn from_service(error: &ManagerSettingsError) -> Self {
        Self {
            kind: error.kind().into(),
            group: error.group(),
            refreshed_workspace: error.refreshed_workspace().cloned().map(Arc::new),
        }
    }
}

impl fmt::Debug for SettingsFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsFailure")
            .field("kind", &self.kind)
            .field("group", &self.group)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct SettingsOperationState {
    pub phase: SettingsOperationPhase,
    pub current_request_id: u64,
    pub error: Option<SettingsFailureKind>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct StepwiseDraft {
    pub enabled: bool,
    pub direct_send: bool,
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
    pub max_items: u8,
    pub max_input_chars: u32,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
    secret_replacement: SecretReplacement,
}

impl Default for StepwiseDraft {
    fn default() -> Self {
        Self::from_settings(&StepwiseSettings {
            enabled: false,
            direct_send: false,
            base_url: PrivateUrl::new(String::new()),
            api_key_configured: false,
            api_key_env: "OPENAI_API_KEY".to_owned(),
            api_key_env_configured: false,
            model: String::new(),
            max_items: 0,
            max_input_chars: 8_000,
            max_output_tokens: 1_000,
            timeout_ms: 20_000,
        })
    }
}

impl fmt::Debug for StepwiseDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StepwiseDraft")
            .field("enabled", &self.enabled)
            .field("direct_send", &self.direct_send)
            .field("base_url_configured", &!self.base_url.trim().is_empty())
            .field(
                "environment_configured",
                &!self.api_key_env.trim().is_empty(),
            )
            .field("model_configured", &!self.model.trim().is_empty())
            .field("max_items", &self.max_items)
            .field("max_input_chars", &self.max_input_chars)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("timeout_ms", &self.timeout_ms)
            .field(
                "replacement_configured",
                &!self.secret_replacement.is_empty(),
            )
            .field("replacement_length", &self.secret_replacement.len())
            .finish()
    }
}

impl StepwiseDraft {
    fn from_settings(settings: &StepwiseSettings) -> Self {
        Self {
            enabled: settings.enabled,
            direct_send: settings.direct_send,
            base_url: settings.base_url.as_str().to_owned(),
            api_key_env: settings.api_key_env.clone(),
            model: settings.model.clone(),
            max_items: settings.max_items,
            max_input_chars: settings.max_input_chars,
            max_output_tokens: settings.max_output_tokens,
            timeout_ms: settings.timeout_ms,
            secret_replacement: SecretReplacement::new(String::new()),
        }
    }

    fn input(&self) -> StepwiseSettingsInput {
        StepwiseSettingsInput {
            enabled: self.enabled,
            direct_send: self.direct_send,
            base_url: PrivateUrl::new(self.base_url.clone()),
            api_key_env: self.api_key_env.clone(),
            model: self.model.clone(),
            max_items: self.max_items,
            max_input_chars: self.max_input_chars,
            max_output_tokens: self.max_output_tokens,
            timeout_ms: self.timeout_ms,
        }
    }

    fn secret_change(&self) -> StepwiseSecretChange {
        if self.secret_replacement.is_empty() {
            StepwiseSecretChange::Keep
        } else {
            StepwiseSecretChange::Replace(self.secret_replacement.clone())
        }
    }

    pub fn secret_replacement(&self) -> &SecretReplacement {
        &self.secret_replacement
    }
}

pub struct StepwiseGroupState {
    revision: Option<StepwiseRevision>,
    base: StepwiseDraft,
    draft: StepwiseDraft,
    pub api_key_configured: bool,
    pub api_key_env_configured: bool,
    pub password_visible: bool,
    pub operation: SettingsOperationState,
    pub test: SettingsOperationState,
    pub test_outcome: Option<StepwiseTestOutcome>,
    conflict: bool,
    submitted: Option<StepwiseSubmission>,
}

struct StepwiseSubmission {
    draft: StepwiseDraft,
    consumes_replacement: bool,
}

impl Default for StepwiseGroupState {
    fn default() -> Self {
        let draft = StepwiseDraft::default();
        Self {
            revision: None,
            base: draft.clone(),
            draft,
            api_key_configured: false,
            api_key_env_configured: false,
            password_visible: false,
            operation: SettingsOperationState::default(),
            test: SettingsOperationState::default(),
            test_outcome: None,
            conflict: false,
            submitted: None,
        }
    }
}

impl fmt::Debug for StepwiseGroupState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StepwiseGroupState")
            .field("revision_present", &self.revision.is_some())
            .field("dirty", &self.is_dirty())
            .field("api_key_configured", &self.api_key_configured)
            .field("api_key_env_configured", &self.api_key_env_configured)
            .field("password_visible", &self.password_visible)
            .field("operation_phase", &self.operation.phase)
            .field("test_phase", &self.test.phase)
            .field("has_test_outcome", &self.test_outcome.is_some())
            .field("conflict", &self.conflict)
            .field("draft", &self.draft)
            .finish()
    }
}

impl StepwiseGroupState {
    pub fn revision(&self) -> Option<StepwiseRevision> {
        self.revision
    }

    pub fn draft(&self) -> &StepwiseDraft {
        &self.draft
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    pub fn conflict_visible(&self) -> bool {
        self.conflict
    }

    fn is_busy(&self) -> bool {
        self.operation.phase == SettingsOperationPhase::Running
            || self.test.phase == SettingsOperationPhase::Running
    }

    fn install(&mut self, value: &RevisionedStepwiseSettings, preserve_draft: bool) {
        let base = StepwiseDraft::from_settings(&value.settings);
        self.revision = Some(value.revision);
        self.api_key_configured = value.settings.api_key_configured;
        self.api_key_env_configured = value.settings.api_key_env_configured;
        self.base = base.clone();
        if !preserve_draft {
            self.draft = base;
        }
    }

    fn restore(&mut self) {
        self.draft = self.base.clone();
        self.conflict = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOverlayDraft {
    pub enabled: bool,
    pub path: String,
    pub opacity: u8,
    pub fit_mode: ImageOverlayFitMode,
}

impl Default for ImageOverlayDraft {
    fn default() -> Self {
        Self {
            enabled: false,
            path: String::new(),
            opacity: 35,
            fit_mode: ImageOverlayFitMode::Fit,
        }
    }
}

#[derive(Default)]
pub struct ImageOverlayGroupState {
    revision: Option<ImageOverlayRevision>,
    base: ImageOverlayDraft,
    draft: ImageOverlayDraft,
    pub operation: SettingsOperationState,
    conflict: bool,
    submitted: Option<ImageOverlayDraft>,
}

impl fmt::Debug for ImageOverlayGroupState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageOverlayGroupState")
            .field("revision_present", &self.revision.is_some())
            .field("dirty", &self.is_dirty())
            .field("enabled", &self.draft.enabled)
            .field("path_configured", &!self.draft.path.trim().is_empty())
            .field("opacity", &self.draft.opacity)
            .field("fit_mode", &self.draft.fit_mode)
            .field("operation_phase", &self.operation.phase)
            .field("conflict", &self.conflict)
            .finish()
    }
}

impl ImageOverlayGroupState {
    pub fn revision(&self) -> Option<ImageOverlayRevision> {
        self.revision
    }

    pub fn draft(&self) -> &ImageOverlayDraft {
        &self.draft
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    pub fn conflict_visible(&self) -> bool {
        self.conflict
    }

    fn install(&mut self, value: &RevisionedImageOverlaySettings, preserve_draft: bool) {
        let base = ImageOverlayDraft {
            enabled: value.settings.enabled,
            path: value.settings.path.as_str().to_owned(),
            opacity: value.settings.opacity,
            fit_mode: value.settings.fit_mode,
        };
        self.revision = Some(value.revision);
        self.base = base.clone();
        if !preserve_draft {
            self.draft = base;
        }
    }

    fn restore(&mut self) {
        self.draft = self.base.clone();
        self.conflict = false;
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ExtraArgsDraft {
    pub text: String,
}

impl fmt::Debug for ExtraArgsDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtraArgsDraft")
            .field("argument_count", &self.argument_count())
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

impl ExtraArgsDraft {
    pub fn argument_count(&self) -> usize {
        self.text.lines().filter(|line| !line.is_empty()).count()
    }

    fn settings(&self) -> ExtraArgsSettings {
        ExtraArgsSettings {
            args: self
                .text
                .lines()
                .map(|line| PrivateArgument::new(line.to_owned()))
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct ExtraArgsGroupState {
    revision: Option<ExtraArgsRevision>,
    base: ExtraArgsDraft,
    draft: ExtraArgsDraft,
    pub operation: SettingsOperationState,
    conflict: bool,
    submitted: Option<ExtraArgsDraft>,
}

impl fmt::Debug for ExtraArgsGroupState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtraArgsGroupState")
            .field("revision_present", &self.revision.is_some())
            .field("dirty", &self.is_dirty())
            .field("draft", &self.draft)
            .field("operation_phase", &self.operation.phase)
            .field("conflict", &self.conflict)
            .finish()
    }
}

impl ExtraArgsGroupState {
    pub fn revision(&self) -> Option<ExtraArgsRevision> {
        self.revision
    }

    pub fn draft(&self) -> &ExtraArgsDraft {
        &self.draft
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.base
    }

    pub fn conflict_visible(&self) -> bool {
        self.conflict
    }

    fn install(&mut self, value: &RevisionedExtraArgs, preserve_draft: bool) {
        let base = ExtraArgsDraft {
            text: value
                .settings
                .args
                .iter()
                .map(PrivateArgument::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
        };
        self.revision = Some(value.revision);
        self.base = base.clone();
        if !preserve_draft {
            self.draft = base;
        }
    }

    fn restore(&mut self) {
        self.draft = self.base.clone();
        self.conflict = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsResetRequest {
    Stepwise(ResetStepwiseSettings),
    Image(ResetImageOverlaySettings),
    ExtraArgs(ResetExtraArgs),
}

impl SettingsResetRequest {
    pub fn group(self) -> SafeSettingsGroup {
        match self {
            Self::Stepwise(_) => SafeSettingsGroup::Stepwise,
            Self::Image(_) => SafeSettingsGroup::ImageOverlay,
            Self::ExtraArgs(_) => SafeSettingsGroup::ExtraArgs,
        }
    }
}

struct PendingSecretClear {
    request: SaveStepwiseSettings,
    submitted: StepwiseDraft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTransition {
    Navigate(Route),
    Refresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtySettingsGroups {
    stepwise: bool,
    image_overlay: bool,
    extra_args: bool,
}

impl DirtySettingsGroups {
    pub fn contains(self, group: SafeSettingsGroup) -> bool {
        match group {
            SafeSettingsGroup::Stepwise => self.stepwise,
            SafeSettingsGroup::ImageOverlay => self.image_overlay,
            SafeSettingsGroup::ExtraArgs => self.extra_args,
        }
    }

    pub fn count(self) -> usize {
        usize::from(self.stepwise) + usize::from(self.image_overlay) + usize::from(self.extra_args)
    }

    fn any(self) -> bool {
        self.stepwise || self.image_overlay || self.extra_args
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingDirtyTransition {
    groups: DirtySettingsGroups,
    transition: SettingsTransition,
}

pub struct SettingsViewState {
    pub tab: SettingsTab,
    pub load_phase: SettingsLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<SettingsFailureKind>,
    pub stepwise: StepwiseGroupState,
    pub image_overlay: ImageOverlayGroupState,
    pub extra_args: ExtraArgsGroupState,
    pub picker_error: Option<PathPickerErrorKind>,
    initialized: bool,
    pending_picker: Option<(u64, PathPickerTarget)>,
    pending_reset: Option<SettingsResetRequest>,
    pending_secret_clear: Option<PendingSecretClear>,
    pending_transition: Option<PendingDirtyTransition>,
    next_request_id: u64,
}

impl Default for SettingsViewState {
    fn default() -> Self {
        Self {
            tab: SettingsTab::Stepwise,
            load_phase: SettingsLoadPhase::Idle,
            current_load_request_id: 0,
            load_error: None,
            stepwise: StepwiseGroupState::default(),
            image_overlay: ImageOverlayGroupState::default(),
            extra_args: ExtraArgsGroupState::default(),
            picker_error: None,
            initialized: false,
            pending_picker: None,
            pending_reset: None,
            pending_secret_clear: None,
            pending_transition: None,
            next_request_id: 0,
        }
    }
}

impl fmt::Debug for SettingsViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsViewState")
            .field("tab", &self.tab)
            .field("load_phase", &self.load_phase)
            .field("load_request_id", &self.current_load_request_id)
            .field("initialized", &self.initialized)
            .field("stepwise", &self.stepwise)
            .field("image_overlay", &self.image_overlay)
            .field("extra_args", &self.extra_args)
            .field("picker_pending", &self.pending_picker.is_some())
            .field(
                "reset_group",
                &self.pending_reset.map(|request| request.group()),
            )
            .field(
                "secret_clear_confirmation",
                &self.pending_secret_clear.is_some(),
            )
            .field(
                "discard_groups",
                &self.pending_transition.map(|pending| pending.groups),
            )
            .finish()
    }
}

impl SettingsViewState {
    pub fn from_workspace(workspace: Arc<ManagerSettingsWorkspace>) -> Self {
        let mut state = Self::default();
        state.install_all(&workspace, true);
        state.initialized = true;
        state.load_phase = SettingsLoadPhase::Ready;
        state
    }

    pub fn begin_load(&mut self) -> u64 {
        self.invalidate_picker();
        let request_id = self.next_request_id();
        self.current_load_request_id = request_id;
        self.load_phase = if self.initialized {
            SettingsLoadPhase::Refreshing
        } else {
            SettingsLoadPhase::Loading
        };
        request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.install_clean_groups(&workspace);
                self.initialized = true;
                self.load_phase = SettingsLoadPhase::Ready;
                self.load_error = None;
            }
            Err(error) => {
                self.load_phase = SettingsLoadPhase::Error;
                self.load_error = Some(error);
            }
        }
        true
    }

    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.tab = tab;
    }

    pub fn edit_stepwise_enabled(&mut self, enabled: bool) {
        self.stepwise.draft.enabled = enabled;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_direct_send(&mut self, direct_send: bool) {
        self.stepwise.draft.direct_send = direct_send;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_url(&mut self, base_url: String) {
        self.stepwise.draft.base_url = base_url;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_environment(&mut self, environment: String) {
        self.stepwise.draft.api_key_env = environment;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_model(&mut self, model: String) {
        self.stepwise.draft.model = model;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_max_items(&mut self, value: u8) {
        self.stepwise.draft.max_items = value;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_max_input_chars(&mut self, value: u32) {
        self.stepwise.draft.max_input_chars = value;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_max_output_tokens(&mut self, value: u32) {
        self.stepwise.draft.max_output_tokens = value;
        self.stepwise.conflict = false;
    }

    pub fn edit_stepwise_timeout_ms(&mut self, value: u64) {
        self.stepwise.draft.timeout_ms = value;
        self.stepwise.conflict = false;
    }

    pub fn edit_secret_replacement(&mut self, replacement: SecretReplacement) {
        self.stepwise.draft.secret_replacement = replacement;
        self.stepwise.conflict = false;
    }

    pub fn set_password_visible(&mut self, visible: bool) {
        self.stepwise.password_visible = visible;
    }

    pub fn edit_image_enabled(&mut self, enabled: bool) {
        self.image_overlay.draft.enabled = enabled;
        self.image_overlay.conflict = false;
    }

    pub fn edit_image_path(&mut self, path: String) {
        self.image_overlay.draft.path = path;
        self.image_overlay.conflict = false;
    }

    pub fn edit_image_opacity(&mut self, opacity: u8) {
        self.image_overlay.draft.opacity = opacity;
        self.image_overlay.conflict = false;
    }

    pub fn edit_image_fit_mode(&mut self, fit_mode: ImageOverlayFitMode) {
        self.image_overlay.draft.fit_mode = fit_mode;
        self.image_overlay.conflict = false;
    }

    pub fn edit_extra_args(&mut self, text: String) {
        self.extra_args.draft.text = text;
        self.extra_args.conflict = false;
    }

    pub fn begin_stepwise_save(&mut self) -> Option<(u64, SaveStepwiseSettings)> {
        if self.stepwise_command_blocked() || !self.stepwise.is_dirty() {
            return None;
        }
        let expected_revision = self.stepwise.revision?;
        let submitted = self.stepwise.draft.clone();
        let secret_change = submitted.secret_change();
        let consumes_replacement = matches!(&secret_change, StepwiseSecretChange::Replace(_));
        let request = SaveStepwiseSettings {
            expected_revision,
            settings: submitted.input(),
            secret_change,
        };
        let request_id = self.start_stepwise_operation(submitted, consumes_replacement);
        Some((request_id, request))
    }

    pub fn begin_stepwise_test(&mut self) -> Option<(u64, TestStepwiseSettings)> {
        if self.stepwise_command_blocked() {
            return None;
        }
        let expected_revision = self.stepwise.revision?;
        let request_id = self.next_request_id();
        self.stepwise.test.current_request_id = request_id;
        self.stepwise.test.phase = SettingsOperationPhase::Running;
        self.stepwise.test.error = None;
        self.stepwise.test_outcome = None;
        Some((
            request_id,
            TestStepwiseSettings {
                expected_revision,
                settings: self.stepwise.draft.input(),
                secret_change: self.stepwise.draft.secret_change(),
            },
        ))
    }

    pub fn apply_stepwise_test_response(
        &mut self,
        request_id: u64,
        result: Result<StepwiseTestOutcome, SettingsFailure>,
    ) -> bool {
        if request_id != self.stepwise.test.current_request_id
            || self.stepwise.test.phase != SettingsOperationPhase::Running
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.stepwise.test_outcome = Some(outcome);
                self.stepwise.test.phase = SettingsOperationPhase::Ready;
                self.stepwise.test.error = None;
                self.stepwise.conflict = false;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.stepwise.install(&workspace.stepwise, true);
                    self.install_clean_image_and_args(&workspace);
                }
                self.stepwise.test_outcome = None;
                self.stepwise.test.phase = SettingsOperationPhase::Error;
                self.stepwise.test.error = Some(failure.kind);
                self.stepwise.conflict = failure.kind == SettingsFailureKind::SettingsConflict;
            }
        }
        true
    }

    pub fn apply_stepwise_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_stepwise_workspace_response(request_id, result)
    }

    pub fn apply_stepwise_reset_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_stepwise_workspace_response(request_id, result)
    }

    pub fn begin_image_save(&mut self) -> Option<(u64, SaveImageOverlaySettings)> {
        if self.image_command_blocked() || !self.image_overlay.is_dirty() {
            return None;
        }
        let expected_revision = self.image_overlay.revision?;
        let submitted = self.image_overlay.draft.clone();
        let request_id = self.next_request_id();
        self.image_overlay.operation.current_request_id = request_id;
        self.image_overlay.operation.phase = SettingsOperationPhase::Running;
        self.image_overlay.operation.error = None;
        self.image_overlay.submitted = Some(submitted.clone());
        Some((
            request_id,
            SaveImageOverlaySettings {
                expected_revision,
                settings: image_settings(&submitted),
            },
        ))
    }

    pub fn apply_image_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_image_workspace_response(request_id, result)
    }

    pub fn apply_image_reset_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_image_workspace_response(request_id, result)
    }

    pub fn begin_extra_args_save(&mut self) -> Option<(u64, SaveExtraArgs)> {
        if self.args_command_blocked() || !self.extra_args.is_dirty() {
            return None;
        }
        let expected_revision = self.extra_args.revision?;
        let submitted = self.extra_args.draft.clone();
        let request_id = self.next_request_id();
        self.extra_args.operation.current_request_id = request_id;
        self.extra_args.operation.phase = SettingsOperationPhase::Running;
        self.extra_args.operation.error = None;
        self.extra_args.submitted = Some(submitted.clone());
        Some((
            request_id,
            SaveExtraArgs {
                expected_revision,
                settings: submitted.settings(),
            },
        ))
    }

    pub fn apply_extra_args_save_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_args_workspace_response(request_id, result)
    }

    pub fn apply_extra_args_reset_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        self.apply_args_workspace_response(request_id, result)
    }

    pub fn request_reset(&mut self, group: SafeSettingsGroup) -> bool {
        if self.pending_reset.is_some() || self.group_busy(group) {
            return false;
        }
        let request = match group {
            SafeSettingsGroup::Stepwise => {
                let Some(expected_revision) = self.stepwise.revision else {
                    return false;
                };
                SettingsResetRequest::Stepwise(ResetStepwiseSettings {
                    expected_revision,
                    confirmed_group: group,
                })
            }
            SafeSettingsGroup::ImageOverlay => {
                let Some(expected_revision) = self.image_overlay.revision else {
                    return false;
                };
                SettingsResetRequest::Image(ResetImageOverlaySettings {
                    expected_revision,
                    confirmed_group: group,
                })
            }
            SafeSettingsGroup::ExtraArgs => {
                let Some(expected_revision) = self.extra_args.revision else {
                    return false;
                };
                SettingsResetRequest::ExtraArgs(ResetExtraArgs {
                    expected_revision,
                    confirmed_group: group,
                })
            }
        };
        self.pending_reset = Some(request);
        true
    }

    pub fn confirm_reset(&mut self) -> Option<(u64, SettingsResetRequest)> {
        let request = self.pending_reset.take()?;
        if self.group_busy(request.group()) {
            return None;
        }
        let request_id = self.next_request_id();
        match request {
            SettingsResetRequest::Stepwise(_) => {
                self.stepwise.operation.current_request_id = request_id;
                self.stepwise.operation.phase = SettingsOperationPhase::Running;
                self.stepwise.operation.error = None;
                self.stepwise.submitted = Some(StepwiseSubmission {
                    draft: self.stepwise.draft.clone(),
                    consumes_replacement: false,
                });
            }
            SettingsResetRequest::Image(_) => {
                self.image_overlay.operation.current_request_id = request_id;
                self.image_overlay.operation.phase = SettingsOperationPhase::Running;
                self.image_overlay.operation.error = None;
                self.image_overlay.submitted = Some(self.image_overlay.draft.clone());
            }
            SettingsResetRequest::ExtraArgs(_) => {
                self.extra_args.operation.current_request_id = request_id;
                self.extra_args.operation.phase = SettingsOperationPhase::Running;
                self.extra_args.operation.error = None;
                self.extra_args.submitted = Some(self.extra_args.draft.clone());
            }
        }
        Some((request_id, request))
    }

    pub fn cancel_reset(&mut self) {
        self.pending_reset = None;
    }

    pub fn reset_confirmation_group(&self) -> Option<SafeSettingsGroup> {
        self.pending_reset.map(SettingsResetRequest::group)
    }

    pub fn request_secret_clear(&mut self) -> bool {
        if self.pending_secret_clear.is_some()
            || self.stepwise.is_busy()
            || !self.stepwise.api_key_configured
        {
            return false;
        }
        let Some(expected_revision) = self.stepwise.revision else {
            return false;
        };
        let submitted = self.stepwise.draft.clone();
        self.pending_secret_clear = Some(PendingSecretClear {
            request: SaveStepwiseSettings {
                expected_revision,
                settings: submitted.input(),
                secret_change: StepwiseSecretChange::Clear(ConfirmedSecretClear::new(
                    expected_revision,
                )),
            },
            submitted,
        });
        true
    }

    pub fn confirm_secret_clear(&mut self) -> Option<(u64, SaveStepwiseSettings)> {
        let pending = self.pending_secret_clear.take()?;
        if self.stepwise.is_busy() {
            return None;
        }
        let request_id = self.start_stepwise_operation(pending.submitted, false);
        Some((request_id, pending.request))
    }

    pub fn cancel_secret_clear(&mut self) {
        self.pending_secret_clear = None;
    }

    pub fn secret_clear_confirmation_visible(&self) -> bool {
        self.pending_secret_clear.is_some()
    }

    pub fn begin_image_picker(&mut self) -> Option<PathPickerRequest> {
        if self.pending_picker.is_some() {
            return None;
        }
        let request_id = self.next_request_id();
        self.pending_picker = Some((request_id, PathPickerTarget::SettingsOverlayImage));
        self.picker_error = None;
        Some(PathPickerRequest::new(
            request_id,
            PathPickerTarget::SettingsOverlayImage,
        ))
    }

    pub fn apply_picker_response(&mut self, response: PathPickerResponse) -> bool {
        if self.pending_picker != Some((response.request_id, response.target)) {
            return false;
        }
        self.pending_picker = None;
        self.picker_error = response.error.as_ref().map(|error| error.kind());
        if let Some(path) = response.path {
            self.edit_image_path(path.to_string_lossy().into_owned());
        }
        true
    }

    pub fn picker_pending(&self) -> bool {
        self.pending_picker.is_some()
    }

    pub fn invalidate_picker(&mut self) {
        self.pending_picker = None;
        self.picker_error = None;
    }

    pub fn fail_running_operations(&mut self) {
        if matches!(
            self.load_phase,
            SettingsLoadPhase::Loading | SettingsLoadPhase::Refreshing
        ) {
            self.apply_load_response(
                self.current_load_request_id,
                Err(SettingsFailureKind::WorkerStopped),
            );
        }
        if self.stepwise.operation.phase == SettingsOperationPhase::Running {
            self.apply_stepwise_save_response(
                self.stepwise.operation.current_request_id,
                Err(SettingsFailure::new(
                    SettingsFailureKind::WorkerStopped,
                    Some(SafeSettingsGroup::Stepwise),
                )),
            );
        }
        if self.stepwise.test.phase == SettingsOperationPhase::Running {
            self.apply_stepwise_test_response(
                self.stepwise.test.current_request_id,
                Err(SettingsFailure::new(
                    SettingsFailureKind::WorkerStopped,
                    Some(SafeSettingsGroup::Stepwise),
                )),
            );
        }
        if self.image_overlay.operation.phase == SettingsOperationPhase::Running {
            self.apply_image_save_response(
                self.image_overlay.operation.current_request_id,
                Err(SettingsFailure::new(
                    SettingsFailureKind::WorkerStopped,
                    Some(SafeSettingsGroup::ImageOverlay),
                )),
            );
        }
        if self.extra_args.operation.phase == SettingsOperationPhase::Running {
            self.apply_extra_args_save_response(
                self.extra_args.operation.current_request_id,
                Err(SettingsFailure::new(
                    SettingsFailureKind::WorkerStopped,
                    Some(SafeSettingsGroup::ExtraArgs),
                )),
            );
        }
    }

    pub fn dirty_groups(&self) -> DirtySettingsGroups {
        DirtySettingsGroups {
            stepwise: self.stepwise.is_dirty(),
            image_overlay: self.image_overlay.is_dirty(),
            extra_args: self.extra_args.is_dirty(),
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.dirty_groups().any()
    }

    pub fn request_transition(&mut self, transition: SettingsTransition) -> bool {
        let groups = self.dirty_groups();
        if !groups.any() {
            self.invalidate_picker();
            return true;
        }
        if self.pending_transition.is_none() {
            self.pending_transition = Some(PendingDirtyTransition { groups, transition });
        }
        false
    }

    pub fn pending_dirty_groups(&self) -> Option<DirtySettingsGroups> {
        self.pending_transition.map(|pending| pending.groups)
    }

    pub fn confirm_discard_transition(&mut self) -> Option<SettingsTransition> {
        let pending = self.pending_transition.take()?;
        self.invalidate_picker();
        self.pending_reset = None;
        self.pending_secret_clear = None;
        self.stepwise.restore();
        self.image_overlay.restore();
        self.extra_args.restore();
        Some(pending.transition)
    }

    pub fn cancel_transition(&mut self) {
        self.pending_transition = None;
    }

    pub fn discard_confirmation_visible(&self) -> bool {
        self.pending_transition.is_some()
    }

    fn stepwise_command_blocked(&self) -> bool {
        self.stepwise.is_busy()
            || self.pending_secret_clear.is_some()
            || self
                .pending_reset
                .is_some_and(|request| request.group() == SafeSettingsGroup::Stepwise)
    }

    fn image_command_blocked(&self) -> bool {
        self.image_overlay.operation.phase == SettingsOperationPhase::Running
            || self
                .pending_reset
                .is_some_and(|request| request.group() == SafeSettingsGroup::ImageOverlay)
    }

    fn args_command_blocked(&self) -> bool {
        self.extra_args.operation.phase == SettingsOperationPhase::Running
            || self
                .pending_reset
                .is_some_and(|request| request.group() == SafeSettingsGroup::ExtraArgs)
    }

    fn group_busy(&self, group: SafeSettingsGroup) -> bool {
        match group {
            SafeSettingsGroup::Stepwise => self.stepwise.is_busy(),
            SafeSettingsGroup::ImageOverlay => {
                self.image_overlay.operation.phase == SettingsOperationPhase::Running
            }
            SafeSettingsGroup::ExtraArgs => {
                self.extra_args.operation.phase == SettingsOperationPhase::Running
            }
        }
    }

    fn start_stepwise_operation(
        &mut self,
        submitted: StepwiseDraft,
        consumes_replacement: bool,
    ) -> u64 {
        let request_id = self.next_request_id();
        self.stepwise.operation.current_request_id = request_id;
        self.stepwise.operation.phase = SettingsOperationPhase::Running;
        self.stepwise.operation.error = None;
        self.stepwise.submitted = Some(StepwiseSubmission {
            draft: submitted,
            consumes_replacement,
        });
        request_id
    }

    fn apply_stepwise_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        if request_id != self.stepwise.operation.current_request_id
            || self.stepwise.operation.phase != SettingsOperationPhase::Running
        {
            return false;
        }
        let submitted = self.stepwise.submitted.take();
        match result {
            Ok(workspace) => {
                let preserve = submitted
                    .as_ref()
                    .is_some_and(|submission| submission.draft != self.stepwise.draft);
                let consume_replacement = submitted.as_ref().is_some_and(|submission| {
                    submission.consumes_replacement
                        && submission.draft.secret_replacement
                            == self.stepwise.draft.secret_replacement
                });
                self.stepwise.install(&workspace.stepwise, preserve);
                if preserve && consume_replacement {
                    self.stepwise.draft.secret_replacement = SecretReplacement::new(String::new());
                }
                self.install_clean_image_and_args(&workspace);
                self.stepwise.operation.phase = SettingsOperationPhase::Ready;
                self.stepwise.operation.error = None;
                self.stepwise.conflict = false;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.stepwise.install(&workspace.stepwise, true);
                    self.install_clean_image_and_args(&workspace);
                }
                self.stepwise.operation.phase = SettingsOperationPhase::Error;
                self.stepwise.operation.error = Some(failure.kind);
                self.stepwise.conflict = failure.kind == SettingsFailureKind::SettingsConflict;
            }
        }
        true
    }

    fn apply_image_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        if request_id != self.image_overlay.operation.current_request_id
            || self.image_overlay.operation.phase != SettingsOperationPhase::Running
        {
            return false;
        }
        let submitted = self.image_overlay.submitted.take();
        match result {
            Ok(workspace) => {
                let preserve = submitted.is_some_and(|draft| draft != self.image_overlay.draft);
                self.image_overlay
                    .install(&workspace.image_overlay, preserve);
                self.install_clean_stepwise_and_args(&workspace);
                self.image_overlay.operation.phase = SettingsOperationPhase::Ready;
                self.image_overlay.operation.error = None;
                self.image_overlay.conflict = false;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.image_overlay.install(&workspace.image_overlay, true);
                    self.install_clean_stepwise_and_args(&workspace);
                }
                self.image_overlay.operation.phase = SettingsOperationPhase::Error;
                self.image_overlay.operation.error = Some(failure.kind);
                self.image_overlay.conflict = failure.kind == SettingsFailureKind::SettingsConflict;
            }
        }
        true
    }

    fn apply_args_workspace_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, SettingsFailure>,
    ) -> bool {
        if request_id != self.extra_args.operation.current_request_id
            || self.extra_args.operation.phase != SettingsOperationPhase::Running
        {
            return false;
        }
        let submitted = self.extra_args.submitted.take();
        match result {
            Ok(workspace) => {
                let preserve = submitted.is_some_and(|draft| draft != self.extra_args.draft);
                self.extra_args.install(&workspace.extra_args, preserve);
                self.install_clean_stepwise_and_image(&workspace);
                self.extra_args.operation.phase = SettingsOperationPhase::Ready;
                self.extra_args.operation.error = None;
                self.extra_args.conflict = false;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.extra_args.install(&workspace.extra_args, true);
                    self.install_clean_stepwise_and_image(&workspace);
                }
                self.extra_args.operation.phase = SettingsOperationPhase::Error;
                self.extra_args.operation.error = Some(failure.kind);
                self.extra_args.conflict = failure.kind == SettingsFailureKind::SettingsConflict;
            }
        }
        true
    }

    fn install_all(&mut self, workspace: &ManagerSettingsWorkspace, force: bool) {
        self.stepwise.install(&workspace.stepwise, !force);
        self.image_overlay.install(&workspace.image_overlay, !force);
        self.extra_args.install(&workspace.extra_args, !force);
    }

    fn install_clean_groups(&mut self, workspace: &ManagerSettingsWorkspace) {
        if !self.stepwise.is_dirty() && !self.stepwise.is_busy() {
            self.stepwise.install(&workspace.stepwise, false);
        }
        if !self.image_overlay.is_dirty()
            && self.image_overlay.operation.phase != SettingsOperationPhase::Running
        {
            self.image_overlay.install(&workspace.image_overlay, false);
        }
        if !self.extra_args.is_dirty()
            && self.extra_args.operation.phase != SettingsOperationPhase::Running
        {
            self.extra_args.install(&workspace.extra_args, false);
        }
    }

    fn install_clean_image_and_args(&mut self, workspace: &ManagerSettingsWorkspace) {
        if !self.image_overlay.is_dirty()
            && self.image_overlay.operation.phase != SettingsOperationPhase::Running
        {
            self.image_overlay.install(&workspace.image_overlay, false);
        }
        if !self.extra_args.is_dirty()
            && self.extra_args.operation.phase != SettingsOperationPhase::Running
        {
            self.extra_args.install(&workspace.extra_args, false);
        }
    }

    fn install_clean_stepwise_and_args(&mut self, workspace: &ManagerSettingsWorkspace) {
        if !self.stepwise.is_dirty() && !self.stepwise.is_busy() {
            self.stepwise.install(&workspace.stepwise, false);
        }
        if !self.extra_args.is_dirty()
            && self.extra_args.operation.phase != SettingsOperationPhase::Running
        {
            self.extra_args.install(&workspace.extra_args, false);
        }
    }

    fn install_clean_stepwise_and_image(&mut self, workspace: &ManagerSettingsWorkspace) {
        if !self.stepwise.is_dirty() && !self.stepwise.is_busy() {
            self.stepwise.install(&workspace.stepwise, false);
        }
        if !self.image_overlay.is_dirty()
            && self.image_overlay.operation.phase != SettingsOperationPhase::Running
        {
            self.image_overlay.install(&workspace.image_overlay, false);
        }
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("settings request id overflow");
        self.next_request_id
    }
}

fn image_settings(draft: &ImageOverlayDraft) -> ImageOverlaySettings {
    ImageOverlaySettings {
        enabled: draft.enabled,
        path: PrivatePath::new(draft.path.clone()),
        opacity: draft.opacity,
        fit_mode: draft.fit_mode,
    }
}
