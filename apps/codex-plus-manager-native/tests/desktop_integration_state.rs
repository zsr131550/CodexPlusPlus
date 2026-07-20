use std::sync::Arc;

use codex_plus_core::desktop_integration::{ShortcutSnapshot, WindowsDesktopSnapshot};
use codex_plus_core::startup_registration::{
    OwnedStringValueSnapshot, StartAtSignInHealth, StartupRegistrationSnapshot,
};
use codex_plus_manager_native::state::desktop_integration::{
    DesktopIntegrationFailure, DesktopIntegrationFailureKind, DesktopIntegrationLoadPhase,
    DesktopIntegrationOperation, DesktopIntegrationOperationPhase, DesktopIntegrationViewState,
};
use codex_plus_manager_service::{
    DesktopIntegrationEnvironment, DesktopIntegrationEnvironmentError,
    DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationErrorKind, DesktopIntegrationMutation,
    DesktopIntegrationMutationKind, DesktopIntegrationService,
};

#[derive(Clone)]
struct StaticEnvironment(DesktopIntegrationEnvironmentSnapshot);

impl DesktopIntegrationEnvironment for StaticEnvironment {
    fn inspect_desktop_integration(
        &self,
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError> {
        Ok(self.0.clone())
    }

    fn apply_desktop_repair_operation(
        &self,
        _operation: &codex_plus_core::desktop_integration::DesktopRepairOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        Ok(())
    }

    fn apply_startup_registration_operation(
        &self,
        _operation: &codex_plus_core::startup_registration::StartupRegistrationOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        Ok(())
    }
}

fn environment(needs_repair: bool, legacy_enabled: bool) -> DesktopIntegrationEnvironmentSnapshot {
    let manager =
        std::path::PathBuf::from(r"C:\Program Files\CodexPlusPlus\codex-plus-plus-manager.exe");
    let launcher = std::path::PathBuf::from(r"C:\Program Files\CodexPlusPlus\codex-plus-plus.exe");
    let shortcut = |target| ShortcutSnapshot {
        target,
        arguments: String::new(),
    };
    DesktopIntegrationEnvironmentSnapshot::Windows {
        repair: Box::new(WindowsDesktopSnapshot {
            current_exe: manager.clone(),
            launcher_is_file: true,
            desktop_dir: Some(std::path::PathBuf::from(r"C:\Users\fixture\Desktop")),
            programs_dir: Some(std::path::PathBuf::from(r"C:\Users\fixture\Programs")),
            desktop_manager: (!needs_repair).then(|| shortcut(manager.clone())),
            start_menu_launcher: (!needs_repair).then(|| shortcut(launcher.clone())),
            start_menu_manager: (!needs_repair).then(|| shortcut(manager.clone())),
            protocol_command: (!needs_repair).then(|| format!("\"{}\" \"%1\"", manager.display())),
        }),
        sign_in: StartupRegistrationSnapshot {
            launcher_path: launcher.clone(),
            launcher_is_file: true,
            canonical_run: OwnedStringValueSnapshot::Absent,
            legacy_run: if legacy_enabled {
                OwnedStringValueSnapshot::String(format!(
                    "\"{}\" --debug-port 9229",
                    launcher.display()
                ))
            } else {
                OwnedStringValueSnapshot::Absent
            },
            legacy_shortcut: None,
        },
    }
}

fn workspace(
    needs_repair: bool,
    legacy_enabled: bool,
) -> codex_plus_manager_service::DesktopIntegrationWorkspace {
    DesktopIntegrationService::new(StaticEnvironment(environment(needs_repair, legacy_enabled)))
        .inspect()
        .unwrap()
}

#[test]
fn load_refresh_preserves_last_good_and_rejects_stale_responses() {
    let first = Arc::new(workspace(false, true));
    let replacement = Arc::new(workspace(true, false));
    let mut state = DesktopIntegrationViewState::default();

    let first_id = state.begin_load();
    assert_eq!(state.load_phase, DesktopIntegrationLoadPhase::Loading);
    assert!(state.apply_load_response(first_id, Ok(Arc::clone(&first))));
    assert_eq!(state.load_phase, DesktopIntegrationLoadPhase::Ready);

    let refresh = state.begin_load();
    assert_eq!(state.load_phase, DesktopIntegrationLoadPhase::Refreshing);
    assert!(!state.apply_load_response(first_id, Ok(replacement)));
    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &first));
    assert!(state.apply_load_response(refresh, Err(DesktopIntegrationFailureKind::InspectFailed)));
    assert_eq!(state.load_phase, DesktopIntegrationLoadPhase::Error);
    assert!(Arc::ptr_eq(state.workspace.as_ref().unwrap(), &first));
}

#[test]
fn repair_confirmation_freezes_only_item_kinds_and_prevents_duplicates() {
    let mut state = DesktopIntegrationViewState::default();
    let load = state.begin_load();
    state.apply_load_response(load, Ok(Arc::new(workspace(true, false))));

    assert!(state.request_repair_confirmation());
    assert!(!state.request_repair_confirmation());
    assert_eq!(state.repair_confirmation_item_kinds().len(), 4);
    let debug = format!("{state:?}");
    assert!(!debug.contains("Program Files"));

    state.cancel_repair_confirmation();
    assert!(!state.repair_confirmation_visible());
    assert!(state.request_repair_confirmation());
    let (request_id, request) = state.confirm_repair().expect("confirmed repair");
    assert!(request.confirmed);
    assert_eq!(
        state.operation.phase,
        DesktopIntegrationOperationPhase::Running
    );
    assert_eq!(
        state.operation.operation,
        Some(DesktopIntegrationOperation::Repair)
    );

    state.invalidate_operation();
    let mutation = DesktopIntegrationMutation {
        kind: DesktopIntegrationMutationKind::Repair,
        applied_operation_count: 4,
        workspace: workspace(false, false),
    };
    assert!(!state.apply_mutation_response(
        request_id,
        DesktopIntegrationOperation::Repair,
        Ok(Arc::new(mutation)),
    ));
}

#[test]
fn legacy_enabled_truth_exposes_migrate_but_does_not_duplicate_toggle_work() {
    let mut state = DesktopIntegrationViewState::default();
    let load = state.begin_load();
    state.apply_load_response(load, Ok(Arc::new(workspace(false, true))));

    assert_eq!(
        state
            .workspace
            .as_ref()
            .unwrap()
            .sign_in
            .as_ref()
            .unwrap()
            .health,
        StartAtSignInHealth::NeedsMigration
    );
    assert_eq!(state.effective_enabled(), Some(true));
    assert!(state.migrate_visible());
    assert!(state.begin_set_start_at_sign_in(true).is_none());

    let (request_id, request) = state
        .begin_migrate_sign_in()
        .expect("one explicit migrate request");
    assert!(!state.migrate_visible());
    assert!(state.begin_set_start_at_sign_in(false).is_none());
    assert_eq!(
        state.operation.operation,
        Some(DesktopIntegrationOperation::MigrateSignIn)
    );

    let failure = DesktopIntegrationFailure::new(DesktopIntegrationFailureKind::Service(
        DesktopIntegrationErrorKind::Conflict,
    ))
    .with_workspace(Arc::new(workspace(false, false)));
    assert!(state.apply_mutation_response(
        request_id,
        DesktopIntegrationOperation::MigrateSignIn,
        Err(failure),
    ));
    assert_eq!(state.effective_enabled(), Some(false));
    assert_eq!(
        state.operation.phase,
        DesktopIntegrationOperationPhase::Error
    );
    assert_eq!(request.expected_revision, request.expected_revision);
}

#[test]
fn worker_failure_is_typed_and_late_results_are_ignored() {
    let mut state = DesktopIntegrationViewState::default();
    let load = state.begin_load();
    assert!(state.apply_load_response(load, Err(DesktopIntegrationFailureKind::WorkerStopped)));
    assert_eq!(
        state.load_error,
        Some(DesktopIntegrationFailureKind::WorkerStopped)
    );

    let newer = state.begin_load();
    assert!(!state.apply_load_response(load, Err(DesktopIntegrationFailureKind::InspectFailed)));
    assert!(state.apply_load_response(newer, Ok(Arc::new(workspace(false, false)))));
}
