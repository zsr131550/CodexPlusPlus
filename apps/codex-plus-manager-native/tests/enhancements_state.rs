use std::sync::Arc;

use codex_plus_core::settings::{BackendSettings, LaunchMode};
use codex_plus_manager_native::state::enhancements::{
    EnhancementFailure, EnhancementFailureKind, EnhancementLoadPhase, EnhancementOperationPhase,
    EnhancementViewState,
};
use codex_plus_manager_service::{
    EnhancementSettings, EnhancementSettingsEnvironment, EnhancementSettingsService,
    EnhancementWorkspace,
};

#[derive(Clone)]
struct StaticEnvironment(BackendSettings);

impl EnhancementSettingsEnvironment for StaticEnvironment {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.0.clone())
    }

    fn update_enhancement_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        Ok(predicate(&self.0).then(|| self.0.clone()))
    }
}

#[test]
fn initial_load_and_master_gating_preserve_subordinate_values() {
    let mut state = EnhancementViewState::default();
    let request_id = state.begin_load();
    assert_eq!(state.load_phase, EnhancementLoadPhase::Loading);

    assert!(state.apply_load_response(request_id, Ok(workspace(true, 1))));
    assert_eq!(state.load_phase, EnhancementLoadPhase::Ready);
    assert!(state.subcontrols_enabled());

    let mut draft = *state.draft();
    draft.enabled = false;
    state.edit(draft);

    assert!(!state.subcontrols_enabled());
    assert!(state.draft().plugin_marketplace_unlock);
    assert!(state.draft().thread_scroll_restore);
    assert!(state.is_dirty());
    let (_, request) = state.begin_save().unwrap();
    assert!(!request.settings.enabled);
    assert!(request.settings.plugin_marketplace_unlock);
}

#[test]
fn successful_save_keeps_edits_made_after_submission_and_installs_new_revision() {
    let mut state = EnhancementViewState::from_workspace(workspace(true, 1));
    let mut submitted = *state.draft();
    submitted.fast_startup = true;
    state.edit(submitted);
    let (request_id, _) = state.begin_save().unwrap();

    let mut edited_after_submit = *state.draft();
    edited_after_submit.markdown_export = false;
    state.edit(edited_after_submit);
    assert!(state.apply_save_response(request_id, Ok(workspace(true, 2))));

    assert_eq!(state.operation_phase, EnhancementOperationPhase::Ready);
    assert!(!state.draft().markdown_export);
    assert!(state.is_dirty());
    assert!(state.begin_save().is_some());
}

#[test]
fn conflict_keeps_the_draft_until_explicit_reload() {
    let mut state = EnhancementViewState::from_workspace(workspace(true, 1));
    let mut draft = *state.draft();
    draft.conversation_view = true;
    state.edit(draft);
    let (request_id, _) = state.begin_save().unwrap();
    let refreshed = workspace(false, 9);

    assert!(state.apply_save_response(
        request_id,
        Err(EnhancementFailure::with_workspace(
            EnhancementFailureKind::SettingsConflict,
            Arc::clone(&refreshed),
        )),
    ));

    assert!(state.draft().conversation_view);
    assert!(state.is_dirty());
    assert_eq!(state.error, Some(EnhancementFailureKind::SettingsConflict));
    assert!(state.reload_conflict());
    assert_eq!(state.draft(), &refreshed.settings);
    assert!(!state.is_dirty());
}

#[test]
fn stale_load_and_save_responses_do_not_mutate_current_state() {
    let mut state = EnhancementViewState::from_workspace(workspace(true, 1));
    let stale_load = state.begin_load();
    let current_load = state.begin_load();
    assert!(!state.apply_load_response(stale_load, Ok(workspace(false, 2))));
    assert!(state.apply_load_response(current_load, Ok(workspace(true, 3))));

    let mut draft = *state.draft();
    draft.paste_fix = true;
    state.edit(draft);
    let (save_id, _) = state.begin_save().unwrap();
    assert!(!state.apply_save_response(save_id + 1, Ok(workspace(false, 4))));
    assert!(state.draft().paste_fix);
    assert_eq!(state.operation_phase, EnhancementOperationPhase::Saving);
}

#[test]
fn reset_requires_an_explicit_confirmation_and_can_be_cancelled() {
    let mut state = EnhancementViewState::from_workspace(workspace(false, 1));

    assert!(state.request_reset());
    assert!(state.reset_confirmation_pending());
    state.cancel_reset();
    assert!(!state.reset_confirmation_pending());
    assert!(state.confirm_reset().is_none());

    assert!(state.request_reset());
    let (request_id, request) = state.confirm_reset().unwrap();
    assert!(request.confirmed);
    assert_eq!(state.operation_phase, EnhancementOperationPhase::Resetting);
    assert!(state.apply_reset_response(request_id, Ok(workspace(true, 1))));
    assert_eq!(state.draft(), &EnhancementSettings::default());
    assert!(!state.is_dirty());
}

#[test]
fn worker_stop_fails_current_load_and_mutation_without_erasing_the_draft() {
    let mut state = EnhancementViewState::from_workspace(workspace(true, 1));
    let mut draft = *state.draft();
    draft.launch_mode = LaunchMode::Relay;
    state.edit(draft);
    state.begin_save().unwrap();
    state.begin_load();

    state.fail_running_operations();
    state.fail_running_operations();

    assert_eq!(state.load_phase, EnhancementLoadPhase::Error);
    assert_eq!(
        state.load_error,
        Some(EnhancementFailureKind::WorkerStopped)
    );
    assert_eq!(state.operation_phase, EnhancementOperationPhase::Error);
    assert_eq!(state.error, Some(EnhancementFailureKind::WorkerStopped));
    assert_eq!(state.draft().launch_mode, LaunchMode::Relay);
}

fn workspace(enabled: bool, seed: u8) -> Arc<EnhancementWorkspace> {
    let settings = BackendSettings {
        enhancements_enabled: enabled,
        computer_use_guard_enabled: !enabled,
        codex_app_fast_startup: seed.is_multiple_of(2),
        codex_app_thread_id_badge: seed.is_multiple_of(3),
        ..BackendSettings::default()
    };
    Arc::new(
        EnhancementSettingsService::new(StaticEnvironment(settings))
            .load()
            .unwrap(),
    )
}
