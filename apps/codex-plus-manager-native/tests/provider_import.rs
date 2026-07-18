use std::sync::Arc;

use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::settings::{RelayProfile, RelayProtocol};
use codex_plus_manager_native::state::AppState;
use codex_plus_manager_native::state::import::{ImportFailureKind, ImportViewState};
use codex_plus_manager_native::state::provider::{OperationPhase, ProviderViewState};
use codex_plus_manager_service::{
    CcsDiscovery, CcsProviderSummary, PendingImportSnapshot, PendingImportSummary,
    ProviderActivationSummary, ProviderDocument, ProviderImportOutcome, ProviderProfile,
    ProviderRevision, ProviderWorkspace,
};

fn workspace(revision: char, name: &str) -> ProviderWorkspace {
    ProviderWorkspace {
        revision: ProviderRevision::parse(revision.to_string().repeat(64)).unwrap(),
        document: ProviderDocument {
            profiles: vec![ProviderProfile::Ordinary(RelayProfile {
                id: "provider-1".to_owned(),
                name: name.to_owned(),
                ..RelayProfile::default()
            })],
            common_config_contents: String::new(),
            context_config_contents: String::new(),
            default_test_model: String::new(),
        },
        activation: ProviderActivationSummary {
            enabled: false,
            active_profile_id: None,
            active_profile_kind: None,
        },
        context_options: CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
    }
}

fn discovery(revision: char) -> CcsDiscovery {
    CcsDiscovery {
        source_path: "fixture/cc-switch.db".to_owned(),
        source_revision: "a".repeat(64),
        provider_revision: ProviderRevision::parse(revision.to_string().repeat(64)).unwrap(),
        providers: vec![CcsProviderSummary {
            source_id: "source-1".to_owned(),
            name: "Fixture provider".to_owned(),
            base_url: "https://fixture.invalid/v1".to_owned(),
            protocol: RelayProtocol::Responses,
            duplicate: false,
        }],
        importable_count: 1,
        duplicate_count: 0,
    }
}

fn pending() -> PendingImportSummary {
    PendingImportSummary {
        name: "Pending fixture".to_owned(),
        base_url: "https://pending.invalid/v1".to_owned(),
        wire_api: "responses".to_owned(),
        relay_mode: "pureApi".to_owned(),
        api_key_present: true,
        revision: "b".repeat(64),
    }
}

#[test]
fn discovery_rejects_stale_response_and_dirty_provider_blocks_confirmation() {
    let mut state = ImportViewState::default();
    let request_id = state.begin_discovery();
    assert!(!state.apply_discovery_response(request_id + 1, Ok(Arc::new(discovery('c'))),));
    assert_eq!(state.discovery.phase, OperationPhase::Running);
    assert!(state.apply_discovery_response(request_id, Ok(Arc::new(discovery('c')))));
    assert_eq!(state.discovery.phase, OperationPhase::Ready);

    assert!(state.begin_ccs_import(true).is_none());
    assert_eq!(
        state.batch_import.error,
        Some(ImportFailureKind::DirtyProvider)
    );
    let (_, request) = state.begin_ccs_import(false).unwrap();
    assert_eq!(request.source_revision, "a".repeat(64));
}

#[test]
fn import_outcome_returns_workspace_without_retaining_secret_bearing_outcome() {
    let mut state = ImportViewState::default();
    let discovery_id = state.begin_discovery();
    state.apply_discovery_response(discovery_id, Ok(Arc::new(discovery('c'))));
    let (request_id, _) = state.begin_ccs_import(false).unwrap();
    let result = state.apply_ccs_import_response(
        request_id,
        Ok(ProviderImportOutcome {
            imported: 1,
            duplicates: 0,
            profile_id: None,
            profile_name: None,
            workspace: workspace('d', "Imported"),
        }),
    );

    assert!(result.accepted);
    assert_eq!(
        result.workspace.unwrap().document.profiles[0].name(),
        "Imported"
    );
    assert_eq!(state.batch_outcome.unwrap().imported, 1);
    assert!(state.discovery_result.is_none());
    assert!(!format!("{state:?}").contains("api_key"));
}

#[test]
fn pending_confirm_and_dismiss_use_safe_revision_lifecycle() {
    let mut state = ImportViewState::default();
    let load_id = state.begin_pending_load();
    assert!(state.apply_pending_load_response(
        load_id,
        Ok(PendingImportSnapshot {
            pending: Some(pending()),
        }),
    ));
    let revision = ProviderRevision::parse("c".repeat(64)).unwrap();
    let (confirm_id, confirm) = state
        .begin_pending_confirm(false, Some(revision.clone()))
        .unwrap();
    assert_eq!(confirm.pending_revision, "b".repeat(64));
    assert_eq!(confirm.provider_revision, revision);

    let stale = state.apply_pending_confirm_response(
        confirm_id + 1,
        Ok(ProviderImportOutcome {
            imported: 1,
            duplicates: 0,
            profile_id: Some("provider-1".to_owned()),
            profile_name: Some("Pending fixture".to_owned()),
            workspace: workspace('d', "Pending fixture"),
        }),
    );
    assert!(!stale.accepted);
    assert!(state.pending.is_some());

    state.pending_confirm.phase = OperationPhase::Idle;
    let (dismiss_id, dismiss) = state.begin_pending_dismiss().unwrap();
    assert_eq!(dismiss.pending_revision, "b".repeat(64));
    assert!(
        state.apply_pending_dismiss_response(
            dismiss_id,
            Ok(PendingImportSnapshot { pending: None }),
        )
    );
    assert!(state.pending.is_none());
}

#[test]
fn pending_refresh_blocks_confirm_and_dismiss_until_current() {
    let mut state = ImportViewState::default();
    let load_id = state.begin_pending_load();
    state.apply_pending_load_response(
        load_id,
        Ok(PendingImportSnapshot {
            pending: Some(pending()),
        }),
    );
    state.begin_pending_load();

    let revision = ProviderRevision::parse("c".repeat(64)).unwrap();
    assert!(!state.can_confirm_pending(false, true));
    assert!(state.begin_pending_confirm(false, Some(revision)).is_none());
    assert!(state.begin_pending_dismiss().is_none());
}

#[test]
fn app_state_replaces_imported_workspace_only_when_provider_draft_is_clean() {
    let mut app = AppState::default();
    let mut provider = ProviderViewState::default();
    let load_id = provider.begin_load();
    provider.apply_load_response(load_id, Ok(Arc::new(workspace('c', "Baseline"))));
    app.provider = provider;

    assert!(app.apply_imported_provider_workspace(Arc::new(workspace('d', "Imported"))));
    assert_eq!(app.provider.selected_profile().unwrap().name(), "Imported");

    app.provider.draft_mut().unwrap().default_test_model = "dirty".to_owned();
    assert!(!app.apply_imported_provider_workspace(Arc::new(workspace('e', "Rejected"))));
    assert_eq!(app.provider.selected_profile().unwrap().name(), "Imported");
}
