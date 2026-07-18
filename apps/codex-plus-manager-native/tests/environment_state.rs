use std::sync::Arc;

use codex_plus_core::env_conflicts::{
    EnvConflict, EnvConflictRemoval, EnvConflictRemovalFailure, EnvConflictSource,
};
use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_manager_native::state::environment::{EnvironmentFailureKind, EnvironmentViewState};
use codex_plus_manager_native::state::provider::OperationPhase;
use codex_plus_manager_service::{
    EnvironmentRemovalOutcome, RelayEnvironmentErrorKind, RelayEnvironmentWorkspace,
};

fn report() -> RelayEnvironmentReport {
    RelayEnvironmentReport {
        clash_verge_tun: ClashVergeTunCheck {
            enabled: false,
            config_path: None,
        },
        proxy_environment: ProxyEnvironmentCheck {
            variables: Vec::new(),
        },
        codex_env_file: CodexEnvFileCheck {
            exists: false,
            path: "fixture/.env".to_owned(),
        },
    }
}

fn workspace(revision: char, names: &[&str]) -> RelayEnvironmentWorkspace {
    RelayEnvironmentWorkspace {
        report: report(),
        conflicts: names
            .iter()
            .map(|name| EnvConflict {
                name: (*name).to_owned(),
                source: EnvConflictSource::Process,
                value_present: true,
            })
            .collect(),
        revision: revision.to_string().repeat(64),
    }
}

#[test]
fn inspection_rejects_stale_response_and_preserves_still_valid_selection() {
    let mut state = EnvironmentViewState::default();
    let first_id = state.begin_inspection();
    assert!(!state.apply_inspection_response(
        first_id + 1,
        Ok(Arc::new(workspace('a', &["OPENAI_API_KEY"]))),
    ));
    assert!(state.apply_inspection_response(
        first_id,
        Ok(Arc::new(workspace(
            'a',
            &["OPENAI_API_KEY", "OPENAI_BASE_URL"],
        ))),
    ));
    assert!(state.toggle_selection("OPENAI_API_KEY", true));

    let refresh_id = state.begin_inspection();
    assert!(state.apply_inspection_response(
        refresh_id,
        Ok(Arc::new(workspace(
            'b',
            &["OPENAI_API_KEY", "OPENAI_MODEL"],
        ))),
    ));
    assert!(state.is_selected("OPENAI_API_KEY"));
    assert!(!state.is_selected("OPENAI_BASE_URL"));
}

#[test]
fn selection_accepts_only_exact_detected_openai_names_and_starts_unchecked() {
    let mut state = EnvironmentViewState::default();
    let request_id = state.begin_inspection();
    state.apply_inspection_response(
        request_id,
        Ok(Arc::new(workspace('a', &["OPENAI_API_KEY"]))),
    );

    assert_eq!(state.selected_names().count(), 0);
    assert!(!state.toggle_selection("PATH", true));
    assert!(!state.toggle_selection("OPENAI_UNKNOWN", true));
    assert!(state.toggle_selection("OPENAI_API_KEY", true));
    assert_eq!(
        state.selected_names().collect::<Vec<_>>(),
        ["OPENAI_API_KEY"]
    );
}

#[test]
fn partial_cleanup_keeps_failed_remaining_name_selected_and_records_backup_evidence() {
    let mut state = EnvironmentViewState::default();
    let load_id = state.begin_inspection();
    state.apply_inspection_response(
        load_id,
        Ok(Arc::new(workspace(
            'a',
            &["OPENAI_API_KEY", "OPENAI_BASE_URL"],
        ))),
    );
    state.toggle_selection("OPENAI_API_KEY", true);
    state.toggle_selection("OPENAI_BASE_URL", true);
    assert!(state.request_cleanup_confirmation());
    let (cleanup_id, request) = state.begin_cleanup().unwrap();
    assert_eq!(
        request.names,
        ["OPENAI_API_KEY".to_owned(), "OPENAI_BASE_URL".to_owned()]
    );

    assert!(state.apply_cleanup_response(
        cleanup_id,
        Ok(Arc::new(EnvironmentRemovalOutcome {
            removed: vec![EnvConflictRemoval {
                name: "OPENAI_API_KEY".to_owned(),
                removed_process: true,
                removed_user: false,
            }],
            failures: vec![EnvConflictRemovalFailure {
                name: "OPENAI_BASE_URL".to_owned(),
                source: EnvConflictSource::Process,
            }],
            backup_path: Some("fixture/backup.json".to_owned()),
            remaining: workspace('b', &["OPENAI_BASE_URL"]).conflicts,
            report: report(),
            revision: "b".repeat(64),
        })),
    ));
    assert_eq!(state.cleanup_phase, OperationPhase::Ready);
    assert!(state.is_selected("OPENAI_BASE_URL"));
    assert!(!state.is_selected("OPENAI_API_KEY"));
    assert_eq!(state.cleanup_outcome.as_ref().unwrap().failures.len(), 1);
}

#[test]
fn cleanup_worker_and_conflict_failures_are_typed_without_dropping_last_workspace() {
    let mut state = EnvironmentViewState::default();
    let load_id = state.begin_inspection();
    state.apply_inspection_response(load_id, Ok(Arc::new(workspace('a', &["OPENAI_API_KEY"]))));
    state.toggle_selection("OPENAI_API_KEY", true);
    state.request_cleanup_confirmation();
    let (cleanup_id, _) = state.begin_cleanup().unwrap();
    assert!(state.apply_cleanup_response(
        cleanup_id,
        Err(EnvironmentFailureKind::Service(
            RelayEnvironmentErrorKind::Conflict,
        )),
    ));
    assert_eq!(state.cleanup_phase, OperationPhase::Error);
    assert!(state.workspace.is_some());
}
