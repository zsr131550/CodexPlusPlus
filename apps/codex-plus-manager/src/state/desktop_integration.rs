use std::fmt;
use std::sync::Arc;

use codex_plus_core::desktop_integration::{
    DesktopIntegrationHealth, DesktopIntegrationItemKind, DesktopIntegrationItemState,
};
use codex_plus_core::startup_registration::StartAtSignInHealth;
use codex_plus_manager_service::{
    DesktopIntegrationError, DesktopIntegrationErrorKind, DesktopIntegrationMutation,
    DesktopIntegrationRevision, DesktopIntegrationWorkspace, MigrateStartAtSignIn,
    RepairDesktopIntegration, SetStartAtSignIn,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopIntegrationLoadPhase {
    #[default]
    Idle,
    Loading,
    Ready,
    Refreshing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopIntegrationOperationPhase {
    #[default]
    Idle,
    Running,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationOperation {
    Repair,
    MigrateSignIn,
    EnableSignIn,
    DisableSignIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationFailureKind {
    InspectFailed,
    Service(DesktopIntegrationErrorKind),
    WorkerStopped,
}

impl From<DesktopIntegrationErrorKind> for DesktopIntegrationFailureKind {
    fn from(kind: DesktopIntegrationErrorKind) -> Self {
        match kind {
            DesktopIntegrationErrorKind::InspectFailed => Self::InspectFailed,
            DesktopIntegrationErrorKind::WorkerStopped => Self::WorkerStopped,
            other => Self::Service(other),
        }
    }
}

#[derive(Clone)]
pub struct DesktopIntegrationFailure {
    pub kind: DesktopIntegrationFailureKind,
    pub refreshed_workspace: Option<Arc<DesktopIntegrationWorkspace>>,
}

impl DesktopIntegrationFailure {
    pub fn new(kind: DesktopIntegrationFailureKind) -> Self {
        Self {
            kind,
            refreshed_workspace: None,
        }
    }

    pub fn with_workspace(mut self, workspace: Arc<DesktopIntegrationWorkspace>) -> Self {
        self.refreshed_workspace = Some(workspace);
        self
    }

    pub fn from_service(error: &DesktopIntegrationError) -> Self {
        Self {
            kind: error.kind().into(),
            refreshed_workspace: error.refreshed_workspace().cloned().map(Arc::new),
        }
    }
}

impl fmt::Debug for DesktopIntegrationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopIntegrationFailure")
            .field("kind", &self.kind)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct DesktopIntegrationOperationState {
    pub phase: DesktopIntegrationOperationPhase,
    pub current_request_id: u64,
    pub operation: Option<DesktopIntegrationOperation>,
    pub error: Option<DesktopIntegrationFailureKind>,
}

struct RepairConfirmation {
    revision: DesktopIntegrationRevision,
    item_kinds: Vec<DesktopIntegrationItemKind>,
}

impl fmt::Debug for RepairConfirmation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepairConfirmation")
            .field("revision", &self.revision)
            .field("item_kinds", &self.item_kinds)
            .finish()
    }
}

#[derive(Default)]
pub struct DesktopIntegrationViewState {
    pub load_phase: DesktopIntegrationLoadPhase,
    pub current_load_request_id: u64,
    pub load_error: Option<DesktopIntegrationFailureKind>,
    pub workspace: Option<Arc<DesktopIntegrationWorkspace>>,
    pub operation: DesktopIntegrationOperationState,
    repair_confirmation: Option<RepairConfirmation>,
    next_request_id: u64,
}

impl fmt::Debug for DesktopIntegrationViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopIntegrationViewState")
            .field("load_phase", &self.load_phase)
            .field("load_request_id", &self.current_load_request_id)
            .field("has_workspace", &self.workspace.is_some())
            .field("operation", &self.operation)
            .field("repair_confirmation", &self.repair_confirmation)
            .finish()
    }
}

impl DesktopIntegrationViewState {
    pub fn begin_load(&mut self) -> u64 {
        let request_id = self.next_request_id();
        self.current_load_request_id = request_id;
        self.load_phase = if self.workspace.is_some() {
            DesktopIntegrationLoadPhase::Refreshing
        } else {
            DesktopIntegrationLoadPhase::Loading
        };
        request_id
    }

    pub fn apply_load_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<DesktopIntegrationWorkspace>, DesktopIntegrationFailureKind>,
    ) -> bool {
        if request_id != self.current_load_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.workspace = Some(workspace);
                self.load_error = None;
                self.load_phase = DesktopIntegrationLoadPhase::Ready;
            }
            Err(error) => {
                self.load_error = Some(error);
                self.load_phase = DesktopIntegrationLoadPhase::Error;
            }
        }
        true
    }

    pub fn request_repair_confirmation(&mut self) -> bool {
        if self.repair_confirmation.is_some()
            || self.operation.phase == DesktopIntegrationOperationPhase::Running
        {
            return false;
        }
        let Some(workspace) = self.workspace.as_ref() else {
            return false;
        };
        if workspace.repair_health != DesktopIntegrationHealth::NeedsRepair {
            return false;
        }
        let item_kinds = workspace
            .repair_items
            .iter()
            .filter(|item| item.state == DesktopIntegrationItemState::NeedsRepair)
            .map(|item| item.kind)
            .collect::<Vec<_>>();
        if item_kinds.is_empty() {
            return false;
        }
        self.repair_confirmation = Some(RepairConfirmation {
            revision: workspace.revision,
            item_kinds,
        });
        true
    }

    pub fn confirm_repair(&mut self) -> Option<(u64, RepairDesktopIntegration)> {
        if self.operation.phase == DesktopIntegrationOperationPhase::Running {
            return None;
        }
        let confirmation = self.repair_confirmation.take()?;
        let request = RepairDesktopIntegration {
            expected_revision: confirmation.revision,
            confirmed: true,
        };
        let request_id = self.start_operation(DesktopIntegrationOperation::Repair);
        Some((request_id, request))
    }

    pub fn cancel_repair_confirmation(&mut self) {
        self.repair_confirmation = None;
    }

    pub fn repair_confirmation_visible(&self) -> bool {
        self.repair_confirmation.is_some()
    }

    pub fn repair_confirmation_item_kinds(&self) -> &[DesktopIntegrationItemKind] {
        self.repair_confirmation
            .as_ref()
            .map_or(&[], |confirmation| confirmation.item_kinds.as_slice())
    }

    pub fn begin_migrate_sign_in(&mut self) -> Option<(u64, MigrateStartAtSignIn)> {
        if !self.migrate_visible() {
            return None;
        }
        let revision = self.workspace.as_ref()?.revision;
        let request_id = self.start_operation(DesktopIntegrationOperation::MigrateSignIn);
        Some((
            request_id,
            MigrateStartAtSignIn {
                expected_revision: revision,
            },
        ))
    }

    pub fn begin_set_start_at_sign_in(&mut self, enabled: bool) -> Option<(u64, SetStartAtSignIn)> {
        if self.operation.phase == DesktopIntegrationOperationPhase::Running
            || self.effective_enabled() == Some(enabled)
        {
            return None;
        }
        let revision = self.workspace.as_ref()?.revision;
        let operation = if enabled {
            DesktopIntegrationOperation::EnableSignIn
        } else {
            DesktopIntegrationOperation::DisableSignIn
        };
        let request_id = self.start_operation(operation);
        Some((
            request_id,
            SetStartAtSignIn {
                expected_revision: revision,
                enabled,
            },
        ))
    }

    pub fn apply_mutation_response(
        &mut self,
        request_id: u64,
        operation: DesktopIntegrationOperation,
        result: Result<Arc<DesktopIntegrationMutation>, DesktopIntegrationFailure>,
    ) -> bool {
        if self.operation.phase != DesktopIntegrationOperationPhase::Running
            || self.operation.current_request_id != request_id
            || self.operation.operation != Some(operation)
        {
            return false;
        }
        match result {
            Ok(mutation) => {
                self.workspace = Some(Arc::new(mutation.workspace.clone()));
                self.operation.phase = DesktopIntegrationOperationPhase::Ready;
                self.operation.error = None;
            }
            Err(failure) => {
                if let Some(workspace) = failure.refreshed_workspace {
                    self.workspace = Some(workspace);
                }
                self.operation.phase = DesktopIntegrationOperationPhase::Error;
                self.operation.error = Some(failure.kind);
            }
        }
        true
    }

    pub fn invalidate_operation(&mut self) {
        self.operation.phase = DesktopIntegrationOperationPhase::Idle;
        self.operation.operation = None;
        self.operation.error = None;
    }

    pub fn fail_worker(&mut self) {
        if matches!(
            self.load_phase,
            DesktopIntegrationLoadPhase::Loading | DesktopIntegrationLoadPhase::Refreshing
        ) {
            let _ = self.apply_load_response(
                self.current_load_request_id,
                Err(DesktopIntegrationFailureKind::WorkerStopped),
            );
        }
        if self.operation.phase == DesktopIntegrationOperationPhase::Running {
            self.operation.phase = DesktopIntegrationOperationPhase::Error;
            self.operation.error = Some(DesktopIntegrationFailureKind::WorkerStopped);
        }
    }

    pub fn effective_enabled(&self) -> Option<bool> {
        self.workspace
            .as_ref()?
            .sign_in
            .as_ref()
            .map(|status| status.effective_enabled)
    }

    pub fn migrate_visible(&self) -> bool {
        self.operation.phase != DesktopIntegrationOperationPhase::Running
            && self
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.sign_in.as_ref())
                .is_some_and(|status| status.health == StartAtSignInHealth::NeedsMigration)
    }

    fn start_operation(&mut self, operation: DesktopIntegrationOperation) -> u64 {
        let request_id = self.next_request_id();
        self.operation.current_request_id = request_id;
        self.operation.operation = Some(operation);
        self.operation.phase = DesktopIntegrationOperationPhase::Running;
        self.operation.error = None;
        request_id
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("desktop integration request id overflow");
        self.next_request_id
    }
}
