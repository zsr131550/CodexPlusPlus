use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use codex_plus_manager_service::{
    EnvironmentRemovalOutcome, RelayEnvironmentErrorKind, RelayEnvironmentWorkspace,
    RemoveEnvironmentConflicts,
};

use super::provider::OperationPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentFailureKind {
    Service(RelayEnvironmentErrorKind),
    WorkerStopped,
}

#[derive(Default)]
pub struct EnvironmentViewState {
    pub inspection_phase: OperationPhase,
    pub current_inspection_request_id: u64,
    pub inspection_error: Option<EnvironmentFailureKind>,
    pub workspace: Option<Arc<RelayEnvironmentWorkspace>>,
    selected_names: BTreeSet<String>,
    pub cleanup_confirmation: bool,
    pub cleanup_phase: OperationPhase,
    pub current_cleanup_request_id: u64,
    pub cleanup_error: Option<EnvironmentFailureKind>,
    pub cleanup_outcome: Option<Arc<EnvironmentRemovalOutcome>>,
}

impl fmt::Debug for EnvironmentViewState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentViewState")
            .field("inspection_phase", &self.inspection_phase)
            .field("has_workspace", &self.workspace.is_some())
            .field("selected_count", &self.selected_names.len())
            .field("cleanup_confirmation", &self.cleanup_confirmation)
            .field("cleanup_phase", &self.cleanup_phase)
            .field("has_cleanup_outcome", &self.cleanup_outcome.is_some())
            .finish()
    }
}

impl EnvironmentViewState {
    pub fn begin_inspection(&mut self) -> u64 {
        self.current_inspection_request_id = next_id(
            self.current_inspection_request_id,
            "relay environment inspection",
        );
        self.inspection_phase = OperationPhase::Running;
        self.inspection_error = None;
        self.current_inspection_request_id
    }

    pub fn apply_inspection_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<RelayEnvironmentWorkspace>, EnvironmentFailureKind>,
    ) -> bool {
        if request_id != self.current_inspection_request_id {
            return false;
        }
        match result {
            Ok(workspace) => {
                let available = conflict_names(&workspace);
                self.selected_names.retain(|name| available.contains(name));
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

    pub fn selected_names(&self) -> impl Iterator<Item = &str> {
        self.selected_names.iter().map(String::as_str)
    }

    pub fn is_selected(&self, name: &str) -> bool {
        self.selected_names.contains(name)
    }

    pub fn toggle_selection(&mut self, name: &str, selected: bool) -> bool {
        let name = name.trim();
        if !name.starts_with("OPENAI_")
            || !self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.conflicts.iter().any(|item| item.name == name))
        {
            return false;
        }
        if selected {
            self.selected_names.insert(name.to_owned())
        } else {
            self.selected_names.remove(name)
        }
    }

    pub fn request_cleanup_confirmation(&mut self) -> bool {
        if self.selected_names.is_empty() || self.cleanup_phase == OperationPhase::Running {
            return false;
        }
        self.cleanup_confirmation = true;
        true
    }

    pub fn cancel_cleanup_confirmation(&mut self) {
        if self.cleanup_phase != OperationPhase::Running {
            self.cleanup_confirmation = false;
        }
    }

    pub fn begin_cleanup(&mut self) -> Option<(u64, RemoveEnvironmentConflicts)> {
        if !self.cleanup_confirmation || self.cleanup_phase == OperationPhase::Running {
            return None;
        }
        let workspace = self.workspace.as_ref()?;
        if self.selected_names.is_empty() {
            return None;
        }
        self.current_cleanup_request_id =
            next_id(self.current_cleanup_request_id, "relay environment cleanup");
        self.cleanup_phase = OperationPhase::Running;
        self.cleanup_error = None;
        self.cleanup_confirmation = false;
        self.cleanup_outcome = None;
        Some((
            self.current_cleanup_request_id,
            RemoveEnvironmentConflicts {
                expected_revision: workspace.revision.clone(),
                names: self.selected_names.iter().cloned().collect(),
            },
        ))
    }

    pub fn apply_cleanup_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<EnvironmentRemovalOutcome>, EnvironmentFailureKind>,
    ) -> bool {
        if request_id != self.current_cleanup_request_id {
            return false;
        }
        match result {
            Ok(outcome) => {
                let workspace = Arc::new(RelayEnvironmentWorkspace {
                    report: outcome.report.clone(),
                    conflicts: outcome.remaining.clone(),
                    revision: outcome.revision.clone(),
                });
                let remaining = conflict_names(&workspace);
                self.selected_names.retain(|name| remaining.contains(name));
                self.workspace = Some(workspace);
                self.cleanup_phase = OperationPhase::Ready;
                self.cleanup_error = None;
                self.cleanup_outcome = Some(outcome);
            }
            Err(error) => {
                self.cleanup_phase = OperationPhase::Error;
                self.cleanup_error = Some(error);
            }
        }
        true
    }
}

fn conflict_names(workspace: &RelayEnvironmentWorkspace) -> BTreeSet<String> {
    workspace
        .conflicts
        .iter()
        .map(|item| item.name.clone())
        .collect()
}

fn next_id(current: u64, label: &str) -> u64 {
    current
        .checked_add(1)
        .unwrap_or_else(|| panic!("{label} request id overflow"))
}
