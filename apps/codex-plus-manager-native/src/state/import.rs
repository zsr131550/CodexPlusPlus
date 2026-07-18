use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    CcsDiscovery, ConfirmPendingImport, DismissPendingImport, ImportCcsProviders,
    PendingImportSnapshot, PendingImportSummary, ProviderImportErrorKind, ProviderImportOutcome,
    ProviderRevision, ProviderWorkspace,
};

use super::provider::OperationPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFailureKind {
    Service(ProviderImportErrorKind),
    DirtyProvider,
    MissingProviderWorkspace,
    WorkerStopped,
}

#[derive(Debug, Default)]
pub struct ImportOperationState {
    pub phase: OperationPhase,
    pub current_request_id: u64,
    pub error: Option<ImportFailureKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOutcomeSummary {
    pub imported: usize,
    pub duplicates: usize,
}

pub struct ImportApplyResult {
    pub accepted: bool,
    pub workspace: Option<Arc<ProviderWorkspace>>,
}

impl ImportApplyResult {
    fn stale() -> Self {
        Self {
            accepted: false,
            workspace: None,
        }
    }

    fn accepted(workspace: Option<Arc<ProviderWorkspace>>) -> Self {
        Self {
            accepted: true,
            workspace,
        }
    }
}

#[derive(Default)]
pub struct ImportViewState {
    pub discovery: ImportOperationState,
    pub discovery_result: Option<Arc<CcsDiscovery>>,
    pub discovery_open: bool,
    pub batch_import: ImportOperationState,
    pub batch_outcome: Option<ImportOutcomeSummary>,
    pub pending_load: ImportOperationState,
    pub pending: Option<PendingImportSummary>,
    pub pending_confirm: ImportOperationState,
    pub pending_dismiss: ImportOperationState,
    pub pending_outcome: Option<ImportOutcomeSummary>,
}

impl fmt::Debug for ImportViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportViewState")
            .field("discovery", &self.discovery)
            .field(
                "discovered_count",
                &self
                    .discovery_result
                    .as_ref()
                    .map(|result| result.providers.len()),
            )
            .field("discovery_open", &self.discovery_open)
            .field("batch_import", &self.batch_import)
            .field("batch_outcome", &self.batch_outcome)
            .field("pending_load", &self.pending_load)
            .field("has_pending", &self.pending.is_some())
            .field("pending_confirm", &self.pending_confirm)
            .field("pending_dismiss", &self.pending_dismiss)
            .field("pending_outcome", &self.pending_outcome)
            .finish()
    }
}

impl ImportViewState {
    pub fn begin_discovery(&mut self) -> u64 {
        let request_id = begin_operation(&mut self.discovery, "provider import discovery");
        self.discovery_open = true;
        self.batch_import.error = None;
        self.batch_outcome = None;
        request_id
    }

    pub fn apply_discovery_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<CcsDiscovery>, ImportFailureKind>,
    ) -> bool {
        if request_id != self.discovery.current_request_id {
            return false;
        }
        match result {
            Ok(discovery) => {
                self.discovery_result = Some(discovery);
                self.discovery.phase = OperationPhase::Ready;
                self.discovery.error = None;
            }
            Err(error) => {
                self.discovery.phase = OperationPhase::Error;
                self.discovery.error = Some(error);
            }
        }
        true
    }

    pub fn close_discovery(&mut self) -> bool {
        if self.batch_import.phase == OperationPhase::Running {
            return false;
        }
        self.discovery_open = false;
        self.batch_import.error = None;
        true
    }

    pub fn can_import_ccs(&self, provider_dirty: bool) -> bool {
        !provider_dirty
            && self.batch_import.phase != OperationPhase::Running
            && self
                .discovery_result
                .as_ref()
                .is_some_and(|discovery| discovery.importable_count > 0)
    }

    pub fn begin_ccs_import(&mut self, provider_dirty: bool) -> Option<(u64, ImportCcsProviders)> {
        if provider_dirty {
            self.batch_import.phase = OperationPhase::Error;
            self.batch_import.error = Some(ImportFailureKind::DirtyProvider);
            return None;
        }
        if self.batch_import.phase == OperationPhase::Running {
            return None;
        }
        let discovery = self.discovery_result.as_ref()?;
        if discovery.importable_count == 0 {
            return None;
        }
        let request = ImportCcsProviders {
            source_revision: discovery.source_revision.clone(),
            provider_revision: discovery.provider_revision.clone(),
        };
        let request_id = begin_operation(&mut self.batch_import, "provider batch import");
        self.batch_outcome = None;
        Some((request_id, request))
    }

    pub fn apply_ccs_import_response(
        &mut self,
        request_id: u64,
        result: Result<ProviderImportOutcome, ImportFailureKind>,
    ) -> ImportApplyResult {
        if request_id != self.batch_import.current_request_id {
            return ImportApplyResult::stale();
        }
        match result {
            Ok(outcome) => {
                self.batch_import.phase = OperationPhase::Ready;
                self.batch_import.error = None;
                self.batch_outcome = Some(ImportOutcomeSummary {
                    imported: outcome.imported,
                    duplicates: outcome.duplicates,
                });
                self.discovery_open = false;
                self.discovery_result = None;
                ImportApplyResult::accepted(Some(Arc::new(outcome.workspace)))
            }
            Err(error) => {
                self.batch_import.phase = OperationPhase::Error;
                self.batch_import.error = Some(error);
                ImportApplyResult::accepted(None)
            }
        }
    }

    pub fn begin_pending_load(&mut self) -> u64 {
        begin_operation(&mut self.pending_load, "pending provider import load")
    }

    pub fn apply_pending_load_response(
        &mut self,
        request_id: u64,
        result: Result<PendingImportSnapshot, ImportFailureKind>,
    ) -> bool {
        if request_id != self.pending_load.current_request_id {
            return false;
        }
        match result {
            Ok(snapshot) => {
                self.pending = snapshot.pending;
                self.pending_load.phase = OperationPhase::Ready;
                self.pending_load.error = None;
                self.pending_confirm = ImportOperationState::default();
                self.pending_dismiss = ImportOperationState::default();
                self.pending_outcome = None;
            }
            Err(error) => {
                self.pending_load.phase = OperationPhase::Error;
                self.pending_load.error = Some(error);
            }
        }
        true
    }

    pub fn can_confirm_pending(&self, provider_dirty: bool, provider_ready: bool) -> bool {
        !provider_dirty
            && provider_ready
            && self.pending.is_some()
            && self.pending_load.phase != OperationPhase::Running
            && self.pending_confirm.phase != OperationPhase::Running
            && self.pending_dismiss.phase != OperationPhase::Running
    }

    pub fn begin_pending_confirm(
        &mut self,
        provider_dirty: bool,
        provider_revision: Option<ProviderRevision>,
    ) -> Option<(u64, ConfirmPendingImport)> {
        if provider_dirty {
            self.pending_confirm.phase = OperationPhase::Error;
            self.pending_confirm.error = Some(ImportFailureKind::DirtyProvider);
            return None;
        }
        if self.pending_load.phase == OperationPhase::Running
            || self.pending_confirm.phase == OperationPhase::Running
            || self.pending_dismiss.phase == OperationPhase::Running
        {
            return None;
        }
        let Some(provider_revision) = provider_revision else {
            self.pending_confirm.phase = OperationPhase::Error;
            self.pending_confirm.error = Some(ImportFailureKind::MissingProviderWorkspace);
            return None;
        };
        let pending_revision = self.pending.as_ref()?.revision.clone();
        let request_id = begin_operation(&mut self.pending_confirm, "pending provider import");
        self.pending_outcome = None;
        Some((
            request_id,
            ConfirmPendingImport {
                pending_revision,
                provider_revision,
            },
        ))
    }

    pub fn apply_pending_confirm_response(
        &mut self,
        request_id: u64,
        result: Result<ProviderImportOutcome, ImportFailureKind>,
    ) -> ImportApplyResult {
        if request_id != self.pending_confirm.current_request_id {
            return ImportApplyResult::stale();
        }
        match result {
            Ok(outcome) => {
                self.pending_confirm.phase = OperationPhase::Ready;
                self.pending_confirm.error = None;
                self.pending_outcome = Some(ImportOutcomeSummary {
                    imported: outcome.imported,
                    duplicates: outcome.duplicates,
                });
                self.pending = None;
                ImportApplyResult::accepted(Some(Arc::new(outcome.workspace)))
            }
            Err(error) => {
                self.pending_confirm.phase = OperationPhase::Error;
                self.pending_confirm.error = Some(error);
                ImportApplyResult::accepted(None)
            }
        }
    }

    pub fn begin_pending_dismiss(&mut self) -> Option<(u64, DismissPendingImport)> {
        if self.pending_load.phase == OperationPhase::Running
            || self.pending_confirm.phase == OperationPhase::Running
            || self.pending_dismiss.phase == OperationPhase::Running
        {
            return None;
        }
        let pending_revision = self.pending.as_ref()?.revision.clone();
        let request_id = begin_operation(&mut self.pending_dismiss, "pending provider dismissal");
        Some((request_id, DismissPendingImport { pending_revision }))
    }

    pub fn apply_pending_dismiss_response(
        &mut self,
        request_id: u64,
        result: Result<PendingImportSnapshot, ImportFailureKind>,
    ) -> bool {
        if request_id != self.pending_dismiss.current_request_id {
            return false;
        }
        match result {
            Ok(snapshot) => {
                self.pending = snapshot.pending;
                self.pending_dismiss.phase = OperationPhase::Ready;
                self.pending_dismiss.error = None;
                self.pending_confirm = ImportOperationState::default();
            }
            Err(error) => {
                self.pending_dismiss.phase = OperationPhase::Error;
                self.pending_dismiss.error = Some(error);
            }
        }
        true
    }

    pub fn reset_route_transients(&mut self) {
        if self.batch_import.phase != OperationPhase::Running {
            self.discovery_open = false;
        }
        self.batch_import.error = None;
    }
}

fn begin_operation(state: &mut ImportOperationState, label: &str) -> u64 {
    state.current_request_id = state
        .current_request_id
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"));
    state.phase = OperationPhase::Running;
    state.error = None;
    state.current_request_id
}
