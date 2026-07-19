use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_manager_native::path_picker::{PathPickerResponse, PathPickerTarget};
use codex_plus_manager_native::state::Route;
use codex_plus_manager_native::state::settings::{
    SettingsFailure, SettingsFailureKind, SettingsLoadPhase, SettingsOperationPhase,
    SettingsResetRequest, SettingsTab, SettingsTransition, SettingsViewState,
};
use codex_plus_manager_service::{
    ImageOverlayFitMode, SafeSettingsGroup, SecretReplacement, StepwiseSecretChange,
    StepwiseTestOutcome,
};

mod common;

use common::manager_settings_workspace;

#[test]
fn full_response_updates_clean_groups_but_never_rebases_an_unrelated_dirty_group() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_image_path("C:/fixture/unsaved.png".to_owned());
    let original_image_revision = state.image_overlay.revision();
    state.edit_stepwise_model("saved-model".to_owned());

    let (request_id, _) = state.begin_stepwise_save().unwrap();
    assert!(state.apply_stepwise_save_response(request_id, Ok(manager_settings_workspace(2)),));

    assert_eq!(state.image_overlay.draft().path, "C:/fixture/unsaved.png");
    assert_eq!(state.image_overlay.revision(), original_image_revision);
    assert!(state.image_overlay.is_dirty());
    assert!(!state.stepwise.is_dirty());
    assert_eq!(state.extra_args.draft().argument_count(), 2);
}

#[test]
fn stale_and_conflict_responses_preserve_target_draft_with_a_fresh_retry_revision() {
    let mut state = SettingsViewState::default();
    let stale = state.begin_load();
    let current = state.begin_load();
    assert!(!state.apply_load_response(stale, Ok(manager_settings_workspace(1))));
    assert!(state.apply_load_response(current, Ok(manager_settings_workspace(1))));
    assert_eq!(state.load_phase, SettingsLoadPhase::Ready);

    state.edit_stepwise_url("https://draft.invalid/private".to_owned());
    let old_revision = state.stepwise.revision();
    let (request_id, _) = state.begin_stepwise_save().unwrap();
    let fresh = manager_settings_workspace(2);
    let fresh_revision = Some(fresh.stepwise.revision);
    assert!(state.apply_stepwise_save_response(
        request_id,
        Err(SettingsFailure::with_workspace(
            SettingsFailureKind::SettingsConflict,
            SafeSettingsGroup::Stepwise,
            Arc::clone(&fresh),
        )),
    ));

    assert_eq!(
        state.stepwise.draft().base_url,
        "https://draft.invalid/private"
    );
    assert_eq!(state.stepwise.revision(), fresh_revision);
    assert_ne!(state.stepwise.revision(), old_revision);
    assert!(state.stepwise.is_dirty());
    assert!(state.stepwise.conflict_visible());
}

#[test]
fn groups_are_independently_busy_and_prevent_duplicate_commands() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_image_opacity(77);
    state.edit_stepwise_model("other-model".to_owned());

    let (image_id, _) = state.begin_image_save().unwrap();
    assert!(state.begin_image_save().is_none());
    let (stepwise_id, _) = state.begin_stepwise_save().unwrap();
    assert!(state.begin_stepwise_test().is_none());
    assert_eq!(
        state.image_overlay.operation.phase,
        SettingsOperationPhase::Running
    );
    assert_eq!(
        state.stepwise.operation.phase,
        SettingsOperationPhase::Running
    );

    assert!(state.apply_image_save_response(image_id, Ok(manager_settings_workspace(2))));
    assert!(state.apply_stepwise_save_response(stepwise_id, Ok(manager_settings_workspace(3))));
    assert_eq!(
        state.image_overlay.operation.phase,
        SettingsOperationPhase::Ready
    );
    assert_eq!(
        state.stepwise.operation.phase,
        SettingsOperationPhase::Ready
    );
}

#[test]
fn tabs_keep_drafts_and_stepwise_test_has_an_independent_safe_outcome() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_image_fit_mode(ImageOverlayFitMode::Tile);
    state.set_tab(SettingsTab::LaunchArguments);
    state.edit_extra_args("--first\n--second=value".to_owned());
    state.set_tab(SettingsTab::Stepwise);
    state.edit_stepwise_model("tested-model".to_owned());

    let (test_id, request) = state.begin_stepwise_test().unwrap();
    assert_eq!(request.settings.model, "tested-model");
    assert!(
        state.apply_stepwise_test_response(test_id, Ok(StepwiseTestOutcome { item_count: 4 }),)
    );

    assert_eq!(
        state.image_overlay.draft().fit_mode,
        ImageOverlayFitMode::Tile
    );
    assert_eq!(state.extra_args.draft().text, "--first\n--second=value");
    assert_eq!(state.stepwise.test_outcome.unwrap().item_count, 4);
    assert_eq!(state.stepwise.test.phase, SettingsOperationPhase::Ready);
}

#[test]
fn stepwise_test_conflict_installs_fresh_revision_without_discarding_the_draft() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_stepwise_model("locally-edited-model".to_owned());
    let stale_revision = state.stepwise.revision().unwrap();

    let (request_id, request) = state.begin_stepwise_test().unwrap();
    assert_eq!(request.expected_revision, stale_revision);
    let fresh = manager_settings_workspace(2);
    let fresh_revision = fresh.stepwise.revision;
    assert!(state.apply_stepwise_test_response(
        request_id,
        Err(SettingsFailure::with_workspace(
            SettingsFailureKind::SettingsConflict,
            SafeSettingsGroup::Stepwise,
            Arc::clone(&fresh),
        )),
    ));

    assert_eq!(state.stepwise.revision(), Some(fresh_revision));
    assert_eq!(state.stepwise.draft().model, "locally-edited-model");
    assert!(state.stepwise.is_dirty());
    assert!(state.stepwise.conflict_visible());

    let (_, retry) = state.begin_stepwise_test().unwrap();
    assert_eq!(retry.expected_revision, fresh_revision);
}

#[test]
fn reset_and_clear_secret_confirmations_freeze_exact_typed_requests() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    let frozen_image_revision = state.image_overlay.revision().unwrap();
    assert!(state.request_reset(SafeSettingsGroup::ImageOverlay));
    let load = state.begin_load();
    assert!(state.apply_load_response(load, Ok(manager_settings_workspace(2))));
    let (_, reset) = state.confirm_reset().unwrap();
    let SettingsResetRequest::Image(request) = reset else {
        panic!("expected image reset")
    };
    assert_eq!(request.expected_revision, frozen_image_revision);
    assert_eq!(request.confirmed_group, SafeSettingsGroup::ImageOverlay);

    assert!(state.request_secret_clear());
    state.edit_stepwise_model("changed-after-prompt".to_owned());
    let (_, clear) = state.confirm_secret_clear().unwrap();
    assert_ne!(clear.settings.model, "changed-after-prompt");
    assert!(matches!(
        clear.secret_change,
        StepwiseSecretChange::Clear(_)
    ));
}

#[test]
fn secret_visibility_replacement_and_picker_results_stay_typed_and_stale_safe() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    assert!(!state.stepwise.password_visible);
    state.set_password_visible(true);
    state.edit_secret_replacement(SecretReplacement::new("replacement-key-sentinel"));
    assert!(state.stepwise.password_visible);
    assert!(state.stepwise.is_dirty());

    let request = state.begin_image_picker().unwrap();
    assert!(!state.apply_picker_response(PathPickerResponse {
        request_id: request.request_id + 1,
        target: PathPickerTarget::SettingsOverlayImage,
        path: Some(PathBuf::from("C:/fixture/stale.png")),
        error: None,
    }));
    assert!(state.apply_picker_response(PathPickerResponse {
        request_id: request.request_id,
        target: request.target,
        path: Some(PathBuf::from("C:/fixture/selected.png")),
        error: None,
    }));
    assert_eq!(state.image_overlay.draft().path, "C:/fixture/selected.png");

    let debug = format!("{state:?}");
    assert!(!debug.contains("replacement-key-sentinel"));
    assert!(!debug.contains("selected.png"));
}

#[test]
fn route_refresh_guard_freezes_dirty_groups_and_cancel_never_mutates_drafts() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_stepwise_model("dirty-model".to_owned());
    state.edit_extra_args("--dirty".to_owned());

    assert!(!state.request_transition(SettingsTransition::Navigate(Route::About)));
    let frozen = state.pending_dirty_groups().unwrap();
    assert!(frozen.contains(SafeSettingsGroup::Stepwise));
    assert!(frozen.contains(SafeSettingsGroup::ExtraArgs));
    assert!(!frozen.contains(SafeSettingsGroup::ImageOverlay));
    state.cancel_transition();
    assert_eq!(state.stepwise.draft().model, "dirty-model");
    assert_eq!(state.extra_args.draft().text, "--dirty");

    assert!(!state.request_transition(SettingsTransition::Refresh));
    let transition = state.confirm_discard_transition().unwrap();
    assert_eq!(transition, SettingsTransition::Refresh);
    assert!(!state.any_dirty());
}

#[test]
fn clean_navigation_and_refresh_invalidate_pending_image_picker_results() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    let original_path = state.image_overlay.draft().path.clone();

    let navigate_picker = state.begin_image_picker().unwrap();
    assert!(state.request_transition(SettingsTransition::Navigate(Route::About)));
    assert!(!state.picker_pending());
    assert!(!state.apply_picker_response(PathPickerResponse {
        request_id: navigate_picker.request_id,
        target: navigate_picker.target,
        path: Some(PathBuf::from("C:/fixture/late-navigation.png")),
        error: None,
    }));
    assert_eq!(state.image_overlay.draft().path, original_path);

    let refresh_picker = state.begin_image_picker().unwrap();
    assert!(state.request_transition(SettingsTransition::Refresh));
    assert!(!state.picker_pending());
    assert!(!state.apply_picker_response(PathPickerResponse {
        request_id: refresh_picker.request_id,
        target: refresh_picker.target,
        path: Some(PathBuf::from("C:/fixture/late-refresh.png")),
        error: None,
    }));
    assert_eq!(state.image_overlay.draft().path, original_path);

    let focus_refresh_picker = state.begin_image_picker().unwrap();
    state.begin_load();
    assert!(!state.picker_pending());
    assert!(!state.apply_picker_response(PathPickerResponse {
        request_id: focus_refresh_picker.request_id,
        target: focus_refresh_picker.target,
        path: Some(PathBuf::from("C:/fixture/late-focus-refresh.png")),
        error: None,
    }));
    assert_eq!(state.image_overlay.draft().path, original_path);
}

#[test]
fn worker_stop_fails_every_concurrent_settings_operation() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.begin_load();
    state.edit_stepwise_model("test-while-worker-stops".to_owned());
    state.edit_image_opacity(81);
    state.edit_extra_args("--pending-worker-stop".to_owned());
    state.begin_stepwise_test().unwrap();
    state.begin_image_save().unwrap();
    state.begin_extra_args_save().unwrap();

    state.fail_running_operations();
    state.fail_running_operations();

    assert_eq!(state.load_phase, SettingsLoadPhase::Error);
    assert_eq!(state.load_error, Some(SettingsFailureKind::WorkerStopped));
    assert_eq!(state.stepwise.test.phase, SettingsOperationPhase::Error);
    assert_eq!(
        state.stepwise.test.error,
        Some(SettingsFailureKind::WorkerStopped)
    );
    assert_eq!(
        state.image_overlay.operation.phase,
        SettingsOperationPhase::Error
    );
    assert_eq!(
        state.image_overlay.operation.error,
        Some(SettingsFailureKind::WorkerStopped)
    );
    assert_eq!(
        state.extra_args.operation.phase,
        SettingsOperationPhase::Error
    );
    assert_eq!(
        state.extra_args.operation.error,
        Some(SettingsFailureKind::WorkerStopped)
    );
}

#[test]
fn successful_stepwise_save_consumes_only_the_submitted_replacement() {
    let mut state = SettingsViewState::from_workspace(manager_settings_workspace(1));
    state.edit_secret_replacement(SecretReplacement::new("submitted-key-sentinel"));
    let (request_id, request) = state.begin_stepwise_save().unwrap();
    assert!(matches!(
        request.secret_change,
        StepwiseSecretChange::Replace(_)
    ));
    state.edit_stepwise_model("edited-after-submit".to_owned());

    assert!(state.apply_stepwise_save_response(request_id, Ok(manager_settings_workspace(2)),));
    assert_eq!(state.stepwise.draft().model, "edited-after-submit");
    assert!(state.stepwise.draft().secret_replacement().is_empty());
    assert!(state.stepwise.is_dirty());
    let (_, retry) = state.begin_stepwise_save().unwrap();
    assert!(matches!(retry.secret_change, StepwiseSecretChange::Keep));

    let mut changed_secret = SettingsViewState::from_workspace(manager_settings_workspace(1));
    changed_secret.edit_secret_replacement(SecretReplacement::new("first-key-sentinel"));
    let (request_id, _) = changed_secret.begin_stepwise_save().unwrap();
    changed_secret.edit_secret_replacement(SecretReplacement::new("new-key-sentinel"));
    assert!(
        changed_secret.apply_stepwise_save_response(request_id, Ok(manager_settings_workspace(2)),)
    );
    assert_eq!(
        changed_secret.stepwise.draft().secret_replacement(),
        &SecretReplacement::new("new-key-sentinel")
    );
    let (_, retry) = changed_secret.begin_stepwise_save().unwrap();
    assert!(matches!(
        retry.secret_change,
        StepwiseSecretChange::Replace(_)
    ));
}
