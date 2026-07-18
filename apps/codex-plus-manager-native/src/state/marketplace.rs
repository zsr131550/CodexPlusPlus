use std::sync::Arc;

use codex_plus_manager_service::{
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRepair,
    PluginMarketplaceRepairOutcome, PluginMarketplaceStatus, PluginMarketplaceWorkspace,
    RepairPluginMarketplace,
};

use super::provider::OperationPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceFailureKind {
    Service(PluginMarketplaceErrorKind),
    WorkerStopped,
}

#[derive(Debug, Default)]
pub struct MarketplaceViewState {
    pub inspection_phase: OperationPhase,
    pub current_inspection_request_id: u64,
    pub inspection_error: Option<MarketplaceFailureKind>,
    pub workspace: Option<Arc<PluginMarketplaceWorkspace>>,

    pub repair_phase: OperationPhase,
    pub current_repair_request_id: u64,
    pub repair_error: Option<MarketplaceFailureKind>,
    pub confirmation_kind: Option<PluginMarketplaceKind>,
    pub active_repair_kind: Option<PluginMarketplaceKind>,
    pub failed_repair_kind: Option<PluginMarketplaceKind>,
    pub last_repair: Option<(PluginMarketplaceKind, PluginMarketplaceRepairOutcome)>,
}

impl MarketplaceViewState {
    pub fn begin_inspection(&mut self) -> Option<u64> {
        if self.repair_phase == OperationPhase::Running {
            return None;
        }
        self.current_inspection_request_id = next_id(
            self.current_inspection_request_id,
            "plugin marketplace inspection",
        );
        self.inspection_phase = OperationPhase::Running;
        self.inspection_error = None;
        Some(self.current_inspection_request_id)
    }

    pub fn apply_inspection_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<PluginMarketplaceWorkspace>, MarketplaceFailureKind>,
    ) -> bool {
        if request_id != self.current_inspection_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                self.workspace = Some(workspace);
                self.inspection_phase = OperationPhase::Ready;
                self.inspection_error = None;
            }
            Err(error) => {
                self.inspection_phase = OperationPhase::Error;
                self.inspection_error = Some(error);
            }
        }
        true
    }

    pub fn request_repair_confirmation(&mut self, kind: PluginMarketplaceKind) -> bool {
        if self.inspection_phase == OperationPhase::Running
            || self.repair_phase == OperationPhase::Running
            || self.confirmation_kind.is_some()
            || !self.repair_enabled(kind)
        {
            return false;
        }
        self.confirmation_kind = Some(kind);
        self.repair_error = None;
        true
    }

    pub fn cancel_repair_confirmation(&mut self) -> bool {
        self.confirmation_kind.take().is_some()
    }

    pub fn confirm_repair(&mut self) -> Option<(u64, RepairPluginMarketplace)> {
        if self.inspection_phase == OperationPhase::Running
            || self.repair_phase == OperationPhase::Running
        {
            return None;
        }
        let kind = self.confirmation_kind?;
        let workspace = self.workspace.as_ref()?;
        if !workspace.status(kind).needs_repair {
            self.confirmation_kind = None;
            return None;
        }
        self.current_repair_request_id =
            next_id(self.current_repair_request_id, "plugin marketplace repair");
        let request_id = self.current_repair_request_id;
        let request = RepairPluginMarketplace {
            expected_revision: workspace.revision.clone(),
            kind,
            confirmed_kind: kind,
        };
        self.confirmation_kind = None;
        self.active_repair_kind = Some(kind);
        self.repair_phase = OperationPhase::Running;
        self.repair_error = None;
        self.failed_repair_kind = None;
        self.last_repair = None;
        Some((request_id, request))
    }

    pub fn apply_repair_response(
        &mut self,
        request_id: u64,
        kind: PluginMarketplaceKind,
        result: Result<Arc<PluginMarketplaceRepair>, MarketplaceFailureKind>,
    ) -> bool {
        if request_id != self.current_repair_request_id || self.active_repair_kind != Some(kind) {
            return false;
        }
        self.active_repair_kind = None;
        match result {
            Ok(repair) => {
                self.workspace = Some(Arc::new(repair.workspace.clone()));
                self.repair_phase = OperationPhase::Ready;
                self.repair_error = None;
                self.failed_repair_kind = None;
                self.last_repair = Some((kind, repair.outcome));
            }
            Err(error) => {
                self.repair_phase = OperationPhase::Error;
                self.repair_error = Some(error);
                self.failed_repair_kind = Some(kind);
            }
        }
        true
    }

    pub fn repair_enabled(&self, kind: PluginMarketplaceKind) -> bool {
        self.workspace
            .as_ref()
            .is_some_and(|workspace| workspace.status(kind).needs_repair)
            && self.inspection_phase != OperationPhase::Running
            && self.repair_phase != OperationPhase::Running
    }

    pub fn status(&self, kind: PluginMarketplaceKind) -> Option<&PluginMarketplaceStatus> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.status(kind))
    }
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}
