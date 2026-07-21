use std::sync::Arc;

use codex_plus_manager::state::marketplace::{MarketplaceFailureKind, MarketplaceViewState};
use codex_plus_manager::state::provider::OperationPhase;
use codex_plus_manager_service::{
    PluginMarketplaceErrorKind, PluginMarketplaceKind, PluginMarketplaceRepair,
    PluginMarketplaceRepairOutcome, PluginMarketplaceRevision, PluginMarketplaceStatus,
    PluginMarketplaceWorkspace,
};

#[test]
fn stale_inspection_response_is_ignored() {
    let mut state = MarketplaceViewState::default();
    let first = state.begin_inspection().unwrap();
    let latest = state.begin_inspection().unwrap();

    assert!(!state.apply_inspection_response(first, Ok(Arc::new(workspace(1, false, false)))));

    assert_eq!(state.current_inspection_request_id, latest);
    assert!(state.workspace.is_none());
    assert_eq!(state.inspection_phase, OperationPhase::Running);
}

#[test]
fn failed_refresh_preserves_last_good_workspace() {
    let mut state = loaded_state(workspace(1, false, false));
    let before = Arc::clone(state.workspace.as_ref().unwrap());
    let request_id = state.begin_inspection().unwrap();

    assert!(state.apply_inspection_response(
        request_id,
        Err(MarketplaceFailureKind::Service(
            PluginMarketplaceErrorKind::InspectFailed,
        )),
    ));

    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &before));
    assert_eq!(state.inspection_phase, OperationPhase::Error);
    assert_eq!(
        state.inspection_error,
        Some(MarketplaceFailureKind::Service(
            PluginMarketplaceErrorKind::InspectFailed,
        ))
    );
}

#[test]
fn repair_requires_exact_confirmation_and_cannot_be_submitted_twice() {
    let mut state = loaded_state(workspace(1, false, false));

    assert!(state.request_repair_confirmation(PluginMarketplaceKind::Local));
    let (request_id, request) = state.confirm_repair().unwrap();

    assert_eq!(request.kind, PluginMarketplaceKind::Local);
    assert_eq!(request.confirmed_kind, PluginMarketplaceKind::Local);
    assert_eq!(request.expected_revision, revision(1));
    assert_eq!(state.repair_phase, OperationPhase::Running);
    assert_eq!(state.current_repair_request_id, request_id);
    assert!(state.confirm_repair().is_none());
    assert!(!state.request_repair_confirmation(PluginMarketplaceKind::Remote));
    assert!(state.begin_inspection().is_none());
}

#[test]
fn repair_failure_preserves_last_good_workspace() {
    let mut state = loaded_state(workspace(1, false, false));
    let before = Arc::clone(state.workspace.as_ref().unwrap());
    state.request_repair_confirmation(PluginMarketplaceKind::Remote);
    let (request_id, _) = state.confirm_repair().unwrap();

    assert!(state.apply_repair_response(
        request_id,
        PluginMarketplaceKind::Remote,
        Err(MarketplaceFailureKind::Service(
            PluginMarketplaceErrorKind::WriteFailed,
        )),
    ));

    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &before));
    assert_eq!(state.repair_phase, OperationPhase::Error);
    assert_eq!(
        state.repair_error,
        Some(MarketplaceFailureKind::Service(
            PluginMarketplaceErrorKind::WriteFailed,
        ))
    );
}

#[test]
fn successful_repair_installs_fresh_workspace_and_safe_outcome() {
    let mut state = loaded_state(workspace(1, false, false));
    state.request_repair_confirmation(PluginMarketplaceKind::Remote);
    let (request_id, _) = state.confirm_repair().unwrap();
    let repaired = PluginMarketplaceRepair {
        outcome: PluginMarketplaceRepairOutcome::Initialized,
        initialized: true,
        configured: true,
        workspace: workspace(2, false, true),
    };

    assert!(state.apply_repair_response(
        request_id,
        PluginMarketplaceKind::Remote,
        Ok(Arc::new(repaired)),
    ));

    assert_eq!(state.repair_phase, OperationPhase::Ready);
    assert!(state.repair_error.is_none());
    assert_eq!(
        state.last_repair,
        Some((
            PluginMarketplaceKind::Remote,
            PluginMarketplaceRepairOutcome::Initialized,
        ))
    );
    assert!(!state.workspace.as_ref().unwrap().remote.needs_repair);
}

#[test]
fn healthy_target_does_not_open_confirmation() {
    let mut state = loaded_state(workspace(1, true, false));

    assert!(!state.request_repair_confirmation(PluginMarketplaceKind::Local));
    assert!(state.confirmation_kind.is_none());
}

fn loaded_state(workspace: PluginMarketplaceWorkspace) -> MarketplaceViewState {
    let mut state = MarketplaceViewState::default();
    let request_id = state.begin_inspection().unwrap();
    assert!(state.apply_inspection_response(request_id, Ok(Arc::new(workspace))));
    state
}

fn workspace(
    revision_value: u8,
    local_healthy: bool,
    remote_healthy: bool,
) -> PluginMarketplaceWorkspace {
    PluginMarketplaceWorkspace {
        revision: revision(revision_value),
        local: status(local_healthy),
        remote: status(remote_healthy),
    }
}

fn status(healthy: bool) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: usize::from(healthy),
        skill_count: usize::from(healthy),
    }
}

fn revision(value: u8) -> PluginMarketplaceRevision {
    PluginMarketplaceRevision::from_digest([value; 32])
}
