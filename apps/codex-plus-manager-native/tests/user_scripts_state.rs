use std::sync::Arc;

use codex_plus_manager_native::state::provider::OperationPhase;
use codex_plus_manager_native::state::user_scripts::{
    LocalScriptFilter, MarketScriptFilter, ScriptsTab, UserScriptMutationKind, UserScriptViewState,
};
use codex_plus_manager_service::{
    ScriptIntegrity, ScriptMarketRevision, ScriptMarketSummary, ScriptMarketWorkspace,
    UserScriptBackupEvidence, UserScriptMutationOutcome, UserScriptOrigin, UserScriptRevision,
    UserScriptStatus, UserScriptSummary, UserScriptWorkspace,
};

fn local(revision: u8, globally_enabled: bool) -> Arc<UserScriptWorkspace> {
    Arc::new(UserScriptWorkspace {
        revision: UserScriptRevision::from_digest([revision; 32]),
        globally_enabled,
        scripts: vec![
            UserScriptSummary {
                key: "builtin:base.js".to_string(),
                name: "Base".to_string(),
                origin: UserScriptOrigin::Builtin,
                enabled: true,
                status: UserScriptStatus::NotLoaded,
                market_id: None,
                version: None,
            },
            UserScriptSummary {
                key: "user:custom.js".to_string(),
                name: "Custom".to_string(),
                origin: UserScriptOrigin::User,
                enabled: false,
                status: UserScriptStatus::Disabled,
                market_id: None,
                version: None,
            },
        ],
    })
}

fn market(revision: u8, integrity: ScriptIntegrity) -> Arc<ScriptMarketWorkspace> {
    Arc::new(ScriptMarketWorkspace {
        revision: ScriptMarketRevision::from_digest([revision; 32]),
        updated_at: Some("2026-07-18T00:00:00Z".to_string()),
        entries: vec![ScriptMarketSummary {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            description: "Useful".to_string(),
            version: "2".to_string(),
            author: "Fixture".to_string(),
            tags: vec!["ui".to_string()],
            source_host: "example.invalid".to_string(),
            homepage: None,
            integrity,
            installed_version: Some("1".to_string()),
            update_available: true,
        }],
    })
}

fn ready_state(integrity: ScriptIntegrity) -> UserScriptViewState {
    let mut state = UserScriptViewState::default();
    let local_id = state.begin_local_refresh();
    assert!(state.apply_local_response(local_id, Ok(local(1, true))));
    let market_id = state.begin_market_refresh();
    assert!(state.apply_market_response(market_id, Ok(market(1, integrity))));
    state
}

#[test]
fn unverified_install_requires_the_extra_acknowledgement() {
    let mut state = ready_state(ScriptIntegrity::Unverified);

    assert!(state.request_install("demo"));
    assert!(state.install_confirmation().is_some());
    assert!(state.confirm_install().is_none());
    assert_eq!(state.mutation_phase, OperationPhase::Idle);

    assert!(state.set_unverified_acknowledgement(true));
    let (_, request) = state.confirm_install().unwrap();
    assert_eq!(request.script_id, "demo");
    assert_eq!(request.confirmed_script_id, "demo");
    assert_eq!(request.confirmed_version, "2");
    assert!(request.acknowledge_unverified);
    assert_eq!(state.mutation_phase, OperationPhase::Running);
}

#[test]
fn confirmed_update_records_update_mutation_kind() {
    let mut state = ready_state(ScriptIntegrity::Verified);
    Arc::make_mut(state.local.as_mut().unwrap())
        .scripts
        .push(UserScriptSummary {
            key: "user:market-demo.js".to_string(),
            name: "Demo".to_string(),
            origin: UserScriptOrigin::User,
            enabled: true,
            status: UserScriptStatus::NotLoaded,
            market_id: Some("demo".to_string()),
            version: Some("1".to_string()),
        });

    assert!(state.request_install("demo"));
    assert!(state.install_confirmation().unwrap().update);
    state.confirm_install().unwrap();
    assert_eq!(state.mutation_kind, Some(UserScriptMutationKind::Update));
}

#[test]
fn stale_market_response_never_replaces_newer_market_state() {
    let mut state = UserScriptViewState::default();
    let old = state.begin_market_refresh();
    let current = state.begin_market_refresh();

    assert!(state.apply_market_response(current, Ok(market(2, ScriptIntegrity::Verified))));
    assert!(!state.apply_market_response(old, Ok(market(1, ScriptIntegrity::Verified))));
    assert_eq!(
        state.market.as_ref().unwrap().revision,
        ScriptMarketRevision::from_digest([2; 32])
    );
}

#[test]
fn mutation_success_installs_fresh_local_state_and_preserves_market_state() {
    let mut state = ready_state(ScriptIntegrity::Verified);
    let market_before = Arc::clone(state.market.as_ref().unwrap());
    assert!(state.request_install("demo"));
    let (request_id, _) = state.confirm_install().unwrap();

    let outcome = Arc::new(UserScriptMutationOutcome {
        workspace: Arc::unwrap_or_clone(local(2, true)),
        backup: UserScriptBackupEvidence {
            id: "opaque".to_string(),
            created: true,
        },
    });
    assert!(state.apply_mutation_response(request_id, Ok(Arc::clone(&outcome))));

    assert_eq!(state.mutation_phase, OperationPhase::Ready);
    assert_eq!(
        state.local.as_ref().unwrap().revision,
        UserScriptRevision::from_digest([2; 32])
    );
    assert!(Arc::ptr_eq(state.market.as_ref().unwrap(), &market_before));
    assert!(Arc::ptr_eq(
        state.mutation_outcome.as_ref().unwrap(),
        &outcome
    ));
}

#[test]
fn global_disable_preserves_individual_filter_and_selection_state() {
    let mut state = ready_state(ScriptIntegrity::Verified);
    state.tab = ScriptsTab::Local;
    state.local_filter = LocalScriptFilter::Disabled;
    state.market_filter = MarketScriptFilter::Updates;
    state.local_query = "custom".to_string();
    state.market_query = "demo".to_string();
    let (request_id, request) = state.request_global_enabled(false).unwrap();
    assert!(!request.enabled);

    let outcome = Arc::new(UserScriptMutationOutcome {
        workspace: Arc::unwrap_or_clone(local(2, false)),
        backup: UserScriptBackupEvidence::none(),
    });
    assert!(state.apply_mutation_response(request_id, Ok(outcome)));

    assert_eq!(state.tab, ScriptsTab::Local);
    assert_eq!(state.local_filter, LocalScriptFilter::Disabled);
    assert_eq!(state.market_filter, MarketScriptFilter::Updates);
    assert_eq!(state.local_query, "custom");
    assert_eq!(state.market_query, "demo");
    assert!(
        !state
            .local
            .as_ref()
            .unwrap()
            .scripts
            .iter()
            .find(|script| script.key == "user:custom.js")
            .unwrap()
            .enabled
    );
}

#[test]
fn delete_confirmation_freezes_the_exact_user_script_key() {
    let mut state = ready_state(ScriptIntegrity::Verified);

    assert!(!state.request_delete("builtin:base.js"));
    assert!(state.request_delete("user:custom.js"));
    let confirmation = state.delete_confirmation().unwrap().clone();
    assert_eq!(confirmation.key, "user:custom.js");
    assert_eq!(confirmation.name, "Custom");

    let (_, request) = state.confirm_delete().unwrap();
    assert_eq!(request.key, "user:custom.js");
    assert_eq!(request.confirmed_key, "user:custom.js");
}

#[test]
fn page_setters_clamp_to_the_current_filtered_range() {
    let mut state = ready_state(ScriptIntegrity::Verified);

    state.set_market_page(99);
    state.set_local_page(99);

    assert_eq!(state.market_page, 0);
    assert_eq!(state.local_page, 0);
}
