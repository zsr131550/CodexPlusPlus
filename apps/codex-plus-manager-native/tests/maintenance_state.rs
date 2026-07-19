use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_manager_native::path_picker::{
    PathPickerError, PathPickerErrorKind, PathPickerResponse, PathPickerTarget,
};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::maintenance::{
    MaintenanceDocumentTab, MaintenanceFailure, MaintenanceFailureKind, MaintenanceLoadPhase,
    MaintenanceOperationPhase, MaintenanceTransition, MaintenanceViewState,
};
use codex_plus_manager_service::{MaintenanceSection, SectionValue};

mod common;

use common::maintenance_workspace;

#[test]
fn maintenance_preserves_last_good_and_path_draft_across_refresh_failure_and_conflict() {
    let mut state = MaintenanceViewState::default();
    let first = maintenance_workspace("C:/fixture/Codex");
    let load = state.begin_load();
    assert!(state.apply_load_response(load, Ok(Arc::clone(&first))));
    state.set_app_path("C:/fixture/Other".into());

    let refresh = state.begin_load();
    assert!(state.apply_load_response(refresh, Err(MaintenanceFailureKind::LogReadFailed)));
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Other");
    assert!(state.path_dirty());
    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &first));

    let (save_id, _) = state.begin_save().unwrap();
    let fresh = maintenance_workspace("C:/fixture/External");
    let conflict = MaintenanceFailure::with_workspace(
        MaintenanceFailureKind::SettingsConflict,
        Arc::clone(&fresh),
    );
    assert!(state.apply_save_response(save_id, Err(conflict)));
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Other");
    assert!(state.path_dirty());
    assert!(state.conflict_visible());
    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &fresh));
}

#[test]
fn maintenance_rejects_stale_results_merges_partial_sections_and_prevents_duplicates() {
    let mut state = MaintenanceViewState::default();
    let first_id = state.begin_load();
    let current_id = state.begin_load();
    assert!(!state.apply_load_response(first_id, Ok(maintenance_workspace("C:/fixture/Stale"))));
    assert!(state.apply_load_response(current_id, Ok(maintenance_workspace("C:/fixture/Current"))));

    let mut partial = (*maintenance_workspace("C:/fixture/Current")).clone();
    partial.logs = SectionValue::Unavailable(MaintenanceSection::Logs);
    let previous_log = state
        .workspace
        .as_ref()
        .unwrap()
        .logs
        .value()
        .unwrap()
        .text()
        .to_owned();
    let refresh_id = state.begin_load();
    assert!(state.apply_load_response(refresh_id, Ok(Arc::new(partial))));
    assert_eq!(
        state
            .workspace
            .as_ref()
            .unwrap()
            .logs
            .value()
            .unwrap()
            .text(),
        previous_log
    );

    state.set_app_path("C:/fixture/Edited".into());
    let (save_id, _) = state.begin_save().unwrap();
    assert!(state.begin_save().is_none());
    assert!(state.apply_save_response(save_id, Ok(maintenance_workspace("C:/fixture/Edited"))));
    let (launch_id, _) = state.begin_launch().unwrap();
    assert!(state.begin_launch().is_none());
    assert!(state.apply_launch_response(
        launch_id,
        Ok(codex_plus_manager_service::LaunchOutcome {
            debug_port: 9229,
            helper_port: 57321,
            accepted: true,
        })
    ));
    assert_eq!(state.load_phase, MaintenanceLoadPhase::Ready);
    assert_eq!(state.launch.phase, MaintenanceOperationPhase::Ready);
}

#[test]
fn maintenance_clear_confirmation_freezes_the_clear_request() {
    let mut state = MaintenanceViewState::default();
    let load = state.begin_load();
    state.apply_load_response(load, Ok(maintenance_workspace("C:/fixture/Codex")));
    state.set_app_path(String::new());

    assert!(state.begin_save().is_none());
    assert!(state.clear_confirmation_visible());
    state.set_app_path("C:/fixture/ChangedAfterPrompt".into());
    let (_, request) = state.confirm_clear().unwrap();

    assert!(request.path.as_str().is_empty());
    assert_eq!(
        state.app_path_draft.expose(),
        "C:/fixture/ChangedAfterPrompt"
    );
    assert!(!state.clear_confirmation_visible());
}

#[test]
fn maintenance_documents_limits_and_picker_results_are_exact_and_stale_safe() {
    let mut state = MaintenanceViewState::default();
    let load = state.begin_load();
    state.apply_load_response(load, Ok(maintenance_workspace("C:/fixture/Codex")));

    state.set_document_tab(MaintenanceDocumentTab::Logs);
    assert!(
        state
            .active_document_text()
            .unwrap()
            .contains("native.maintenance.load")
    );
    assert!(!state.set_log_limit(75));
    assert!(state.set_log_limit(50));
    assert_eq!(state.log_limit, 50);
    state.set_document_tab(MaintenanceDocumentTab::Report);
    let report = state.active_document_text().unwrap();
    assert!(report.contains("\"configured\""));
    assert!(!report.contains("private-key-sentinel"));

    let request = state
        .begin_picker(PathPickerTarget::MaintenanceExecutable)
        .unwrap();
    let stale = PathPickerResponse {
        request_id: request.request_id + 1,
        target: request.target,
        path: Some(PathBuf::from("C:/fixture/Stale.exe")),
        error: None,
    };
    assert!(!state.apply_picker_response(stale));
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Codex");
    let selected = PathPickerResponse {
        request_id: request.request_id,
        target: request.target,
        path: Some(PathBuf::from("C:/fixture/Selected.exe")),
        error: None,
    };
    assert!(state.apply_picker_response(selected));
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Selected.exe");

    let cancel = state
        .begin_picker(PathPickerTarget::MaintenanceDirectory)
        .unwrap();
    assert!(state.apply_picker_response(PathPickerResponse {
        request_id: cancel.request_id,
        target: cancel.target,
        path: None,
        error: None,
    }));
    let failed = state
        .begin_picker(PathPickerTarget::MaintenanceExecutable)
        .unwrap();
    assert!(state.apply_picker_response(PathPickerResponse {
        request_id: failed.request_id,
        target: failed.target,
        path: None,
        error: Some(PathPickerError::new(PathPickerErrorKind::DialogFailed)),
    }));
    assert_eq!(state.picker_error, Some(PathPickerErrorKind::DialogFailed));
}

#[test]
fn maintenance_discard_transition_is_frozen_and_cancel_is_non_mutating() {
    let mut state = MaintenanceViewState::default();
    let load = state.begin_load();
    state.apply_load_response(load, Ok(maintenance_workspace("C:/fixture/Codex")));
    state.set_app_path("C:/fixture/Dirty".into());

    assert!(!state.request_transition(MaintenanceTransition::Navigate(Route::About)));
    assert!(state.discard_confirmation_visible());
    state.cancel_transition();
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Dirty");
    assert!(!state.discard_confirmation_visible());

    assert!(!state.request_transition(MaintenanceTransition::Refresh));
    let transition = state.confirm_discard_transition().unwrap();
    assert_eq!(transition, MaintenanceTransition::Refresh);
    assert_eq!(state.app_path_draft.expose(), "C:/fixture/Codex");
    assert!(!state.path_dirty());
}
