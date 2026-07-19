use std::sync::Arc;

use codex_plus_core::zed_remote::{
    SshTarget, ZedAvailability, ZedOpenStrategy, ZedRemoteProjectSource, ZedRemoteRegistryRevision,
};
use codex_plus_manager_native::state::zed_remote::{
    ZedRemoteFailureKind, ZedRemoteLoadPhase, ZedRemoteViewState,
};
use codex_plus_manager_service::{
    ZedProjectRevision, ZedRemoteErrorKind, ZedRemoteProjectSummary, ZedRemoteWorkspace,
    ZedSettingsRevision,
};

#[test]
fn reducer_preserves_last_good_workspace_and_frozen_launch_confirmation() {
    let mut state = ready_state();
    assert!(state.request_open("project-a", ZedOpenStrategy::ReuseWindow, true));
    assert!(state.set_open_strategy(ZedOpenStrategy::NewWindow));
    assert!(state.set_open_remember(false));
    let original_revision = state
        .pending_open
        .as_ref()
        .unwrap()
        .expected_project_revision
        .clone();

    let refresh = state.begin_load();
    assert_eq!(state.load_phase, ZedRemoteLoadPhase::Refreshing);
    assert!(state.apply_load_response(refresh, Err(ZedRemoteFailureKind::WorkerStopped)));
    assert!(state.workspace.is_some());

    let (_, request) = state.begin_open().unwrap();
    assert_eq!(request.expected_project_revision, original_revision);
    assert_eq!(request.strategy, ZedOpenStrategy::NewWindow);
    assert!(!request.remember);
}

#[test]
fn reducer_preserves_dirty_preferences_and_requests_one_conflict_refresh() {
    let mut state = ready_state();
    state.set_strategy(ZedOpenStrategy::NewWindow);
    let (request_id, _) = state.begin_save_preferences().unwrap();
    assert!(state.apply_save_response(
        request_id,
        Err(ZedRemoteFailureKind::Service(
            ZedRemoteErrorKind::SettingsConflict,
        )),
    ));

    assert!(state.preferences_dirty);
    assert_eq!(state.draft_strategy, ZedOpenStrategy::NewWindow);
    assert!(state.take_refresh_after_conflict());
    assert!(!state.take_refresh_after_conflict());
}

fn ready_state() -> ZedRemoteViewState {
    let mut state = ZedRemoteViewState::default();
    let request_id = state.begin_load();
    assert!(state.apply_load_response(request_id, Ok(Arc::new(workspace()))));
    state
}

fn workspace() -> ZedRemoteWorkspace {
    ZedRemoteWorkspace {
        settings_revision: ZedSettingsRevision::from_digest([1; 32]),
        registry_revision: ZedRemoteRegistryRevision::from_digest([2; 32]),
        default_strategy: ZedOpenStrategy::ReuseWindow,
        registry_enabled: true,
        availability: ZedAvailability {
            platform_supported: true,
            cli_found: true,
            app_found: false,
        },
        projects: vec![ZedRemoteProjectSummary {
            id: "project-a".to_owned(),
            revision: ZedProjectRevision::from_digest([3; 32]),
            label: "Project A".to_owned(),
            host_id: "fixture-host".to_owned(),
            ssh: SshTarget {
                user: "dev".to_owned(),
                host: "fixture.example.test".to_owned(),
                port: Some(22),
            },
            remote_path: "/workspace/a".to_owned(),
            url: "zed://ssh/fixture.example.test/workspace/a".to_owned(),
            source: ZedRemoteProjectSource::Recent,
            last_opened_at_ms: None,
            is_current: false,
        }],
    }
}
