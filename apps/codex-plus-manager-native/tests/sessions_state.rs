use std::sync::Arc;

use codex_plus_manager_native::state::sessions::{
    SessionFailureKind, SessionFilter, SessionViewState,
};
use codex_plus_manager_service::{
    ProviderSyncRevision, ProviderSyncTargetList, ProviderSyncTargetOption,
    ProviderSyncTargetSource, ProviderSyncWorkspace, SessionDeleteBatchOutcome, SessionRevision,
    SessionSummary, SessionWorkspace,
};

#[test]
fn selection_spans_filtered_pages_and_is_reconciled_after_refresh() {
    let mut state = SessionViewState::default();
    state.workspace = Some(Arc::new(workspace(120)));
    state.filter = SessionFilter::Active;
    state.query = "active".to_owned();
    state.page = 2;

    assert_eq!(state.filtered_sessions().len(), 120);
    assert_eq!(state.page_count(), 3);
    assert!(state.select_all_filtered());
    assert_eq!(state.selected_ids.len(), 120);

    let request_id = state.begin_workspace_refresh();
    let refreshed = workspace(119);
    assert!(state.apply_workspace_response(request_id, Ok(Arc::new(refreshed))));
    assert_eq!(state.selected_ids.len(), 119);
    assert_eq!(state.page, 2);
}

#[test]
fn delete_confirmation_requires_exact_selected_ids() {
    let mut state = SessionViewState::default();
    state.workspace = Some(Arc::new(workspace(8)));
    state.selected_ids.insert("session-1".to_owned());
    state.selected_ids.insert("session-4".to_owned());

    assert!(state.request_delete());
    let (request_id, request) = state.confirm_delete().unwrap();

    assert_eq!(request_id, state.current_delete_request_id);
    assert_eq!(
        request.confirmed_ids,
        vec!["session-1".to_owned(), "session-4".to_owned()]
    );
    assert_eq!(
        request
            .selections
            .iter()
            .map(|selection| selection.id.as_str())
            .collect::<Vec<_>>(),
        vec!["session-1", "session-4"]
    );
    assert!(state.confirm_delete().is_none());
}

#[test]
fn stale_refresh_response_cannot_replace_post_delete_workspace() {
    let mut state = SessionViewState::default();
    let initial_id = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(initial_id, Ok(Arc::new(workspace(2)))));
    let refresh_id = state.begin_workspace_refresh();
    state.selected_ids.insert("session-0".to_owned());
    assert!(state.request_delete());
    let (delete_id, _) = state.confirm_delete().unwrap();
    let post_delete = Arc::new(SessionDeleteBatchOutcome {
        outcomes: Vec::new(),
        workspace: workspace(1),
    });

    assert!(state.apply_delete_response(delete_id, Ok(post_delete)));
    assert_eq!(state.workspace.as_ref().unwrap().sessions.len(), 1);
    assert!(!state.apply_workspace_response(refresh_id, Ok(Arc::new(workspace(99)))));
    assert_eq!(state.workspace.as_ref().unwrap().sessions.len(), 1);
}

#[test]
fn worker_stop_keeps_metadata_and_disables_mutations() {
    let mut state = SessionViewState::default();
    state.workspace = Some(Arc::new(workspace(1)));
    state.mark_worker_stopped();

    assert!(state.workspace.is_some());
    assert!(!state.request_delete());
    assert_eq!(
        state.worker_failure(),
        Some(SessionFailureKind::WorkerStopped)
    );
}

#[test]
fn provider_repair_confirmation_freezes_the_exact_target_and_rejects_duplicates() {
    let mut state = SessionViewState::default();
    let request_id = state.begin_provider_workspace_refresh().unwrap();
    assert!(
        state.apply_provider_workspace_response(request_id, Ok(Arc::new(provider_workspace())),)
    );

    assert!(state.request_provider_run_confirmation());
    assert!(!state.set_provider_target("other".to_owned()));
    let (_, request) = state.confirm_provider_run().unwrap();
    assert_eq!(request.target_provider, "openai");
    assert_eq!(request.confirmed_target_provider, "openai");
    assert!(state.confirm_provider_run().is_none());
}

fn workspace(count: usize) -> SessionWorkspace {
    SessionWorkspace {
        db_paths: vec!["db.sqlite".to_owned()],
        sessions: (0..count)
            .map(|index| {
                let mut session = SessionSummary::new(
                    format!("session-{index}"),
                    format!("Active session {index}"),
                    SessionRevision::from_digest([index as u8; 32]),
                );
                session.cwd = "C:/active".to_owned();
                session.model_provider = "openai".to_owned();
                session.updated_at_ms = Some((count - index) as i64);
                session.source_db_paths = vec!["db.sqlite".to_owned()];
                session
            })
            .collect(),
        read_issues: Vec::new(),
    }
}

fn provider_workspace() -> ProviderSyncWorkspace {
    ProviderSyncWorkspace {
        targets: ProviderSyncTargetList {
            current_provider: "openai".to_owned(),
            targets: ["openai", "other"]
                .into_iter()
                .map(|id| ProviderSyncTargetOption {
                    id: id.to_owned(),
                    sources: vec![ProviderSyncTargetSource::Config],
                    is_current_provider: id == "openai",
                    is_manual: false,
                    is_saved: true,
                })
                .collect(),
        },
        selected_target: "openai".to_owned(),
        auto_repair: false,
        revision: ProviderSyncRevision::from_digest([7; 32]),
    }
}
