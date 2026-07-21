use std::sync::Arc;

use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::relay_config::RelayStatus;
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, RelayMode, RelayProfile,
};
use codex_plus_manager::state::provider::{
    CommonConfigExtractionOutcome, DeleteProfileError, GuardOutcome, GuardResolution,
    ListDirection, LiveLoadFailureKind, LiveMutationCommand, LiveMutationFailure, LiveMutationKind,
    LiveMutationRequestResult, OperationPhase, ProviderLoadFailureKind, ProviderLoadPhase,
    ProviderSaveFailureKind, ProviderViewState, TransitionResult,
};
use codex_plus_manager_service::{
    ProviderActivationErrorKind, ProviderActivationSummary, ProviderCommonConfigExtraction,
    ProviderDocument, ProviderKind, ProviderLiveFileKind, ProviderLiveFiles, ProviderLiveRevision,
    ProviderLiveWorkspace, ProviderMutationOutcome, ProviderNetworkFailureKind, ProviderProfile,
    ProviderRevision, ProviderRollbackOutcome, ProviderTestOutcome, ProviderTestResult,
    ProviderWorkspace,
};

fn ordinary(id: &str) -> ProviderProfile {
    ProviderProfile::Ordinary(RelayProfile {
        id: id.to_string(),
        name: format!("Relay {id}"),
        relay_mode: RelayMode::Official,
        ..RelayProfile::default()
    })
}

fn workspace(revision: char, active_id: &str) -> ProviderWorkspace {
    ProviderWorkspace {
        revision: ProviderRevision::parse(revision.to_string().repeat(64)).unwrap(),
        document: ProviderDocument {
            profiles: vec![ordinary("a"), ordinary("b")],
            common_config_contents: String::new(),
            context_config_contents: String::new(),
            default_test_model: "model-a".to_string(),
        },
        activation: ProviderActivationSummary {
            enabled: true,
            active_profile_id: Some(active_id.to_string()),
            active_profile_kind: Some(ProviderKind::Ordinary),
        },
        context_options: CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
    }
}

fn loaded_state() -> ProviderViewState {
    let mut state = ProviderViewState::default();
    let request = state.begin_load();
    assert!(state.apply_load_response(request, Ok(Arc::new(workspace('a', "a")))));
    state
}

fn live_workspace(revision: char, active_id: &str) -> ProviderLiveWorkspace {
    ProviderLiveWorkspace {
        provider: workspace(revision, active_id),
        status: RelayStatus {
            authenticated: true,
            auth_source: "fixture".to_string(),
            account_label: None,
            config_path: "C:/isolated/codex/config.toml".to_string(),
            configured: true,
            requires_openai_auth: true,
            has_bearer_token: true,
        },
        files: ProviderLiveFiles {
            config_path: "C:/isolated/codex/config.toml".to_string(),
            auth_path: "C:/isolated/codex/auth.json".to_string(),
            config_exists: true,
            auth_exists: true,
            config_contents: "model = \"model-a\"\n".to_string(),
            auth_contents: r#"{"OPENAI_API_KEY":"state-secret-sentinel"}"#.to_string(),
        },
        revision: ProviderLiveRevision::parse(revision.to_string().repeat(64)).unwrap(),
    }
}

fn live_loaded_state() -> ProviderViewState {
    let mut state = ProviderViewState::default();
    let request_id = state.begin_live_load().unwrap();
    assert!(state.apply_live_load_response(request_id, Ok(Arc::new(live_workspace('a', "a"))),));
    state
}

fn rename_selected(state: &mut ProviderViewState, name: &str) {
    assert!(state.edit_selected(|profile| match profile {
        ProviderProfile::Ordinary(profile) => profile.name = name.to_string(),
        ProviderProfile::Aggregate { shell, .. } => shell.name = name.to_string(),
    }));
}

#[test]
fn load_edit_cancel_and_save_keep_baseline_and_dirty_state_coherent() {
    let mut state = ProviderViewState::default();
    let request = state.begin_load();
    assert_eq!(state.load_phase, ProviderLoadPhase::Loading);
    assert!(!state.apply_load_response(request + 1, Err(ProviderLoadFailureKind::LoadFailed)));
    assert!(state.apply_load_response(request, Ok(Arc::new(workspace('a', "a")))));
    assert_eq!(state.load_phase, ProviderLoadPhase::Ready);
    assert_eq!(state.selected_profile_id.as_deref(), Some("a"));
    assert!(!state.is_dirty());

    rename_selected(&mut state, "Edited");
    assert!(state.is_dirty());
    assert_eq!(state.edit_generation, 1);
    state.discard_draft();
    assert!(!state.is_dirty());
    assert_eq!(state.selected_profile().unwrap().name(), "Relay a");

    rename_selected(&mut state, "Saved");
    let (save_id, request) = state.begin_save().unwrap();
    assert_eq!(request.expected_revision.as_str(), "a".repeat(64));
    let mut saved = workspace('b', "a");
    saved.document = request.document;
    assert!(state.apply_save_response(save_id, Ok(Arc::new(saved))));
    assert!(!state.is_dirty());
    assert_eq!(state.selected_profile().unwrap().name(), "Saved");
}

#[test]
fn common_config_extraction_installs_the_transformed_workspace_and_syncs_live_provider() {
    let mut state = live_loaded_state();
    assert!(state.edit_selected(|profile| {
        profile.ordinary_mut().unwrap().config_contents =
            "model = \"model-a\"\napproval_policy = \"never\"\n".to_string();
    }));
    let (request_id, request) = state.begin_common_config_extraction().unwrap();
    assert_eq!(request.profile_id, "a");
    assert_eq!(request.expected_revision.as_str(), "a".repeat(64));
    assert_eq!(
        state.common_config_extraction.phase,
        OperationPhase::Running
    );

    let mut saved = workspace('b', "a");
    saved.document = request.document;
    saved.document.common_config_contents = "approval_policy = \"never\"\n".to_string();
    saved.document.profiles[0]
        .ordinary_mut()
        .unwrap()
        .config_contents = "model = \"model-a\"\n".to_string();

    assert!(state.apply_common_config_extraction_response(
        request_id,
        Ok(ProviderCommonConfigExtraction::Applied(Box::new(saved))),
    ));

    assert_eq!(state.common_config_extraction.phase, OperationPhase::Ready);
    assert_eq!(
        state.common_config_extraction.outcome,
        Some(CommonConfigExtractionOutcome::Applied)
    );
    assert_eq!(
        state.baseline.as_ref().unwrap().revision.as_str(),
        "b".repeat(64)
    );
    assert_eq!(
        state.draft().unwrap().common_config_contents,
        "approval_policy = \"never\"\n"
    );
    assert_eq!(
        state
            .live
            .workspace
            .as_ref()
            .unwrap()
            .provider
            .revision
            .as_str(),
        "b".repeat(64)
    );
    assert!(!state.is_dirty());
    assert_eq!(state.edit_generation, 0);
}

#[test]
fn common_config_extraction_preserves_post_submit_edits_and_no_content_is_a_no_op() {
    let mut state = loaded_state();
    let baseline_before = Arc::clone(state.baseline.as_ref().unwrap());
    let draft_before = state.draft().unwrap().clone();
    let generation_before = state.edit_generation;
    let (empty_request_id, _) = state.begin_common_config_extraction().unwrap();

    assert!(state.apply_common_config_extraction_response(
        empty_request_id,
        Ok(ProviderCommonConfigExtraction::NoContent),
    ));
    assert!(Arc::ptr_eq(
        state.baseline.as_ref().unwrap(),
        &baseline_before
    ));
    assert_eq!(state.draft().unwrap(), &draft_before);
    assert_eq!(state.edit_generation, generation_before);
    assert_eq!(
        state.common_config_extraction.outcome,
        Some(CommonConfigExtractionOutcome::NoContent)
    );

    assert!(state.edit_selected(|profile| {
        profile.ordinary_mut().unwrap().config_contents =
            "model = \"model-a\"\napproval_policy = \"never\"\n".to_string();
    }));
    let (request_id, request) = state.begin_common_config_extraction().unwrap();
    rename_selected(&mut state, "Edited after submit");
    let generation_after_edit = state.edit_generation;
    let mut saved = workspace('b', "a");
    saved.document = request.document;
    saved.document.common_config_contents = "approval_policy = \"never\"\n".to_string();

    assert!(state.apply_common_config_extraction_response(
        request_id,
        Ok(ProviderCommonConfigExtraction::Applied(Box::new(saved))),
    ));

    assert_eq!(
        state.selected_profile().unwrap().name(),
        "Edited after submit"
    );
    assert_eq!(state.edit_generation, generation_after_edit);
    assert_eq!(
        state.baseline.as_ref().unwrap().revision.as_str(),
        "b".repeat(64)
    );
    assert!(state.is_dirty());
}

#[test]
fn common_config_extraction_rejects_stale_responses_and_preserves_draft_on_failure() {
    let mut state = loaded_state();
    rename_selected(&mut state, "Unsaved draft");
    let draft_before = state.draft().unwrap().clone();
    let (request_id, _) = state.begin_common_config_extraction().unwrap();

    assert!(!state.apply_common_config_extraction_response(
        request_id + 1,
        Ok(ProviderCommonConfigExtraction::NoContent),
    ));
    assert_eq!(
        state.common_config_extraction.phase,
        OperationPhase::Running
    );
    assert!(state.apply_common_config_extraction_response(
        request_id,
        Err(ProviderSaveFailureKind::Conflict),
    ));
    assert_eq!(state.common_config_extraction.phase, OperationPhase::Error);
    assert_eq!(
        state.common_config_extraction.error,
        Some(ProviderSaveFailureKind::Conflict)
    );
    assert_eq!(state.draft().unwrap(), &draft_before);
    assert_eq!(
        state.baseline.as_ref().unwrap().revision.as_str(),
        "a".repeat(64)
    );
    assert!(!state.apply_common_config_extraction_response(
        request_id,
        Ok(ProviderCommonConfigExtraction::NoContent),
    ));

    let (retry_id, _) = state.begin_common_config_extraction().unwrap();
    assert!(state.apply_common_config_extraction_response(
        retry_id,
        Err(ProviderSaveFailureKind::WorkerStopped),
    ));
    assert_eq!(state.draft().unwrap(), &draft_before);
}

#[test]
fn common_config_extraction_is_mutually_exclusive_with_provider_and_live_mutations() {
    let mut state = loaded_state();
    let (request_id, _) = state.begin_common_config_extraction().unwrap();

    assert!(state.begin_save().is_none());
    assert!(state.begin_live_load().is_none());
    assert_eq!(
        state.request_live_mutation(LiveMutationKind::Clear),
        LiveMutationRequestResult::Running
    );

    assert!(state.apply_common_config_extraction_response(
        request_id,
        Ok(ProviderCommonConfigExtraction::NoContent),
    ));
    let _save = state.begin_save().unwrap();
    assert!(state.begin_common_config_extraction().is_none());
}

#[test]
fn dirty_selection_guard_supports_stay_discard_and_save() {
    let mut state = loaded_state();
    rename_selected(&mut state, "Dirty");

    assert_eq!(
        state.request_selection("b"),
        TransitionResult::GuardRequired
    );
    assert_eq!(
        state.resolve_guard(GuardResolution::Stay),
        GuardOutcome::Stayed
    );
    assert_eq!(state.selected_profile_id.as_deref(), Some("a"));
    assert!(state.is_dirty());

    assert_eq!(
        state.request_selection("b"),
        TransitionResult::GuardRequired
    );
    assert_eq!(
        state.resolve_guard(GuardResolution::Discard),
        GuardOutcome::Applied
    );
    assert_eq!(state.selected_profile_id.as_deref(), Some("b"));
    assert!(!state.is_dirty());

    rename_selected(&mut state, "Dirty B");
    assert_eq!(
        state.request_selection("a"),
        TransitionResult::GuardRequired
    );
    assert_eq!(
        state.resolve_guard(GuardResolution::Save),
        GuardOutcome::NeedsSave
    );
    let (save_id, request) = state.begin_save().unwrap();
    let mut saved = workspace('c', "a");
    saved.document = request.document;
    state.apply_save_response(save_id, Ok(Arc::new(saved)));
    assert_eq!(state.selected_profile_id.as_deref(), Some("a"));
    assert!(!state.is_dirty());
}

#[test]
fn list_mutations_are_deterministic_and_active_or_invalid_deletes_are_blocked() {
    let mut state = loaded_state();
    assert_eq!(
        state.delete_selected(false),
        Err(DeleteProfileError::ActiveProtected)
    );

    assert_eq!(state.request_selection("b"), TransitionResult::Applied);
    let duplicate = state.duplicate_selected().unwrap();
    assert_ne!(duplicate, "b");
    assert_eq!(
        state.selected_profile_id.as_deref(),
        Some(duplicate.as_str())
    );
    assert!(state.move_selected(ListDirection::Up));
    let ordinary = state.add_ordinary();
    assert_eq!(
        state.selected_profile_id.as_deref(),
        Some(ordinary.as_str())
    );
    let aggregate = state.add_aggregate();
    assert_eq!(
        state.selected_profile_id.as_deref(),
        Some(aggregate.as_str())
    );

    let only_member = state
        .draft_mut()
        .unwrap()
        .profiles
        .iter_mut()
        .find_map(|profile| match profile {
            ProviderProfile::Aggregate { routing, .. } if routing.id == aggregate => Some(routing),
            _ => None,
        })
        .unwrap();
    only_member.members = vec![AggregateRelayMember {
        relay_id: ordinary.clone(),
        weight: 1,
    }];
    state.mark_edited();
    let (save_id, request) = state.begin_save().unwrap();
    let mut saved = workspace('e', "a");
    saved.document = request.document;
    state.apply_save_response(save_id, Ok(Arc::new(saved)));
    assert_eq!(
        state.request_selection(&ordinary),
        TransitionResult::Applied
    );
    assert_eq!(
        state.delete_selected(true),
        Err(DeleteProfileError::WouldEmptyAggregate)
    );
}

#[test]
fn save_conflict_keeps_draft_and_current_selection() {
    let mut state = loaded_state();
    rename_selected(&mut state, "Unsaved Secret-Free Name");
    let (save_id, _) = state.begin_save().unwrap();

    assert!(state.apply_save_response(save_id, Err(ProviderSaveFailureKind::Conflict)));
    assert!(state.is_dirty());
    assert_eq!(state.save.phase, OperationPhase::Error);
    assert_eq!(state.save.error, Some(ProviderSaveFailureKind::Conflict));
    assert_eq!(
        state.selected_profile().unwrap().name(),
        "Unsaved Secret-Free Name"
    );
}

#[test]
fn secret_reveal_resets_on_selection_discard_and_save() {
    let mut state = loaded_state();
    state.set_secret_revealed(true);
    assert!(state.secret_revealed);
    assert_eq!(state.request_selection("b"), TransitionResult::Applied);
    assert!(!state.secret_revealed);

    state.set_secret_revealed(true);
    rename_selected(&mut state, "Dirty");
    state.discard_draft();
    assert!(!state.secret_revealed);

    state.set_secret_revealed(true);
    rename_selected(&mut state, "Saved");
    let (save_id, request) = state.begin_save().unwrap();
    let mut saved = workspace('d', "a");
    saved.document = request.document;
    state.apply_save_response(save_id, Ok(Arc::new(saved)));
    assert!(!state.secret_revealed);
}

#[test]
fn network_results_require_request_profile_and_edit_generation_to_match() {
    let mut state = loaded_state();
    let test_token = state.begin_test().unwrap();
    let models_token = state.begin_models().unwrap();
    let doctor_token = state.begin_doctor().unwrap();
    rename_selected(&mut state, "Changed");
    let result = ProviderTestResult {
        http_status: Some(200),
        endpoint: None,
        outcome: ProviderTestOutcome::Success,
    };

    assert!(!state.apply_test_response(test_token, Ok(result.clone())));
    assert!(!state.apply_models_failure(models_token, ProviderNetworkFailureKind::Timeout));
    assert!(!state.apply_doctor_failure(doctor_token, ProviderNetworkFailureKind::Network));

    let current = state.begin_test().unwrap();
    let wrong_profile = current.with_profile_id("b".to_string());
    assert!(!state.apply_test_response(wrong_profile, Ok(result.clone())));
    let wrong_generation = current.with_edit_generation(current.edit_generation + 1);
    assert!(!state.apply_test_response(wrong_generation, Ok(result.clone())));
    let wrong_request = current.with_request_id(current.request_id + 1);
    assert!(!state.apply_test_response(wrong_request, Ok(result.clone())));
    assert!(state.apply_test_response(current, Ok(result)));
    assert_eq!(state.test.phase, OperationPhase::Ready);
}

#[test]
fn aggregate_fixture_shape_remains_supported_by_state() {
    let mut state = loaded_state();
    let document = state.draft_mut().unwrap();
    document.profiles.push(ProviderProfile::Aggregate {
        shell: RelayProfile {
            id: "aggregate".to_string(),
            name: "Aggregate".to_string(),
            relay_mode: RelayMode::Aggregate,
            ..RelayProfile::default()
        },
        routing: AggregateRelayProfile {
            id: "aggregate".to_string(),
            name: "Aggregate".to_string(),
            strategy: Default::default(),
            members: vec![AggregateRelayMember {
                relay_id: "a".to_string(),
                weight: 1,
            }],
        },
    });
    state.mark_edited();
    let (save_id, request) = state.begin_save().unwrap();
    let mut saved = workspace('f', "a");
    saved.document = request.document;
    state.apply_save_response(save_id, Ok(Arc::new(saved)));

    assert_eq!(
        state.request_selection("aggregate"),
        TransitionResult::Applied
    );
    assert_eq!(
        state.selected_profile().unwrap().kind(),
        ProviderKind::Aggregate
    );
}

#[test]
fn preset_and_model_row_edits_preserve_secrets_and_serialize_windows() {
    let mut state = loaded_state();
    assert!(state.edit_selected(|profile| {
        let profile = profile.ordinary_mut().unwrap();
        profile.api_key = "keep-secret".to_string();
        profile.context_window = "200000".to_string();
        profile.model_list = "old-model".to_string();
    }));

    assert!(state.apply_preset("deepseek"));
    let profile = state.selected_profile().unwrap().ordinary().unwrap();
    assert_eq!(profile.api_key, "keep-secret");
    assert_eq!(profile.context_window, "200000");
    assert_eq!(profile.model_list, "deepseek-v4-flash\ndeepseek-v4-pro");

    assert!(state.update_model_row(0, "deepseek-v4-flash", "1M"));
    assert!(state.add_model_row());
    assert!(state.update_model_row(2, "extra-model", "200K"));
    let profile = state.selected_profile().unwrap().ordinary().unwrap();
    assert_eq!(
        profile.model_list,
        "deepseek-v4-flash\ndeepseek-v4-pro\nextra-model"
    );
    assert_eq!(
        profile.model_windows,
        r#"{"deepseek-v4-flash":"1M","extra-model":"200K"}"#
    );
    assert!(state.remove_model_row(1));
    assert_eq!(
        state
            .selected_profile()
            .unwrap()
            .ordinary()
            .unwrap()
            .model_list,
        "deepseek-v4-flash\nextra-model"
    );
}

#[test]
fn discovered_models_and_aggregate_routing_mutations_are_deterministic() {
    let mut state = loaded_state();
    assert!(state.edit_selected(|profile| {
        profile.ordinary_mut().unwrap().model_list = "model-a".to_string();
    }));
    let token = state.begin_models().unwrap();
    assert!(
        state.apply_models_response(
            token,
            Ok(codex_plus_manager_service::ProviderModelsResult {
                models: vec!["model-a".to_string(), "model-b".to_string()],
                endpoint: codex_plus_manager_service::SafeEndpoint::parse(
                    "https://example.test/v1/models",
                )
                .unwrap(),
            }),
        )
    );
    assert!(state.merge_discovered_models());
    assert_eq!(
        state
            .selected_profile()
            .unwrap()
            .ordinary()
            .unwrap()
            .model_list,
        "model-a\nmodel-b"
    );

    let aggregate_id = state.add_aggregate();
    assert!(state.set_aggregate_member("b", true));
    assert!(state.set_aggregate_weight("b", 7));
    let routing = state
        .draft()
        .unwrap()
        .profiles
        .iter()
        .find_map(|profile| match profile {
            ProviderProfile::Aggregate { shell, routing } if shell.id == aggregate_id => {
                Some(routing)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        routing
            .members
            .iter()
            .find(|member| member.relay_id == "b")
            .unwrap()
            .weight,
        7
    );
    assert!(state.set_aggregate_member("b", false));
}

#[test]
fn live_load_is_monotonic_rejects_stale_results_and_replaces_provider_atomically() {
    let mut state = ProviderViewState::default();
    let first = state.begin_live_load().unwrap();
    let second = state.begin_live_load().unwrap();
    assert!(second > first);

    assert!(!state.apply_live_load_response(first, Ok(Arc::new(live_workspace('a', "a"))),));
    assert!(state.live.workspace.is_none());
    assert!(state.apply_live_load_response(second, Ok(Arc::new(live_workspace('b', "b"))),));

    assert_eq!(
        state.baseline.as_ref().unwrap().revision.as_str(),
        "b".repeat(64)
    );
    assert_eq!(state.selected_profile_id.as_deref(), Some("b"));
    assert_eq!(
        state
            .live
            .workspace
            .as_ref()
            .unwrap()
            .provider
            .activation
            .active_profile_id
            .as_deref(),
        Some("b")
    );
}

#[test]
fn live_mutation_confirmation_prevents_duplicates_and_success_updates_active_badges() {
    let mut state = live_loaded_state();
    assert_eq!(
        state.request_live_mutation(LiveMutationKind::Switch {
            target_profile_id: "b".to_string(),
        }),
        LiveMutationRequestResult::ConfirmationRequired
    );
    assert_eq!(
        state.request_live_mutation(LiveMutationKind::Clear),
        LiveMutationRequestResult::ConfirmationPending
    );

    let (request_id, command) = state.confirm_live_mutation().unwrap();
    match command {
        LiveMutationCommand::Switch(request) => {
            assert_eq!(request.target_profile_id, "b");
            assert_eq!(
                request.guard.expected_provider_revision.as_str(),
                "a".repeat(64)
            );
            assert_eq!(
                request.guard.expected_live_revision.as_str(),
                "a".repeat(64)
            );
        }
        _ => panic!("expected switch command"),
    }
    assert!(state.confirm_live_mutation().is_none());
    assert_eq!(
        state.request_live_mutation(LiveMutationKind::Clear),
        LiveMutationRequestResult::Running
    );

    let outcome = ProviderMutationOutcome {
        live: live_workspace('b', "b"),
        backup_path: Some("C:/isolated/backups/switch".to_string()),
        rollback: ProviderRollbackOutcome::NotRequired,
    };
    assert!(state.apply_live_mutation_response(request_id, Ok(Arc::new(outcome))));
    assert_eq!(state.live.mutation_phase, OperationPhase::Ready);
    assert_eq!(state.selected_profile_id.as_deref(), Some("b"));
    assert!(state.selected_is_active());
    assert_eq!(
        state.live.backup_path.as_deref(),
        Some("C:/isolated/backups/switch")
    );
}

#[test]
fn failed_live_save_preserves_provider_live_and_raw_drafts_with_typed_evidence() {
    let mut state = live_loaded_state();
    assert!(state.begin_live_file_edit(ProviderLiveFileKind::Config));
    assert!(state.edit_live_file(
        ProviderLiveFileKind::Config,
        "model = \"edited-but-unsaved\"\n".to_string(),
    ));
    let baseline = Arc::clone(state.baseline.as_ref().unwrap());
    let live = Arc::clone(state.live.workspace.as_ref().unwrap());
    let draft = state.draft().unwrap().clone();
    let selected = state.selected_profile_id.clone();

    assert_eq!(
        state.request_live_mutation(LiveMutationKind::SaveFile(ProviderLiveFileKind::Config,)),
        LiveMutationRequestResult::ConfirmationRequired
    );
    let (request_id, command) = state.confirm_live_mutation().unwrap();
    assert!(matches!(command, LiveMutationCommand::SaveFile(_)));
    let failure = LiveMutationFailure::new(
        ProviderActivationErrorKind::LiveConflict,
        ProviderRollbackOutcome::Verified,
        Some("C:/isolated/backups/conflict".to_string()),
    );
    assert!(state.apply_live_mutation_response(request_id, Err(failure.clone())));

    assert!(Arc::ptr_eq(state.baseline.as_ref().unwrap(), &baseline));
    assert!(Arc::ptr_eq(state.live.workspace.as_ref().unwrap(), &live));
    assert_eq!(state.draft().unwrap(), &draft);
    assert_eq!(state.selected_profile_id, selected);
    assert_eq!(
        state.live_file_draft(ProviderLiveFileKind::Config),
        Some("model = \"edited-but-unsaved\"\n")
    );
    assert!(state.live_file_dirty(ProviderLiveFileKind::Config));
    assert_eq!(state.live.failure.as_ref(), Some(&failure));
}

#[test]
fn raw_live_edits_are_independent_and_extend_the_existing_route_guard() {
    let mut state = live_loaded_state();
    assert!(state.begin_live_file_edit(ProviderLiveFileKind::Config));
    assert!(!state.begin_live_file_edit(ProviderLiveFileKind::Auth));
    assert!(state.edit_live_file(
        ProviderLiveFileKind::Config,
        "model = \"guarded\"\n".to_string(),
    ));
    assert_eq!(state.request_reload(), TransitionResult::GuardRequired);
    assert_eq!(
        state.resolve_guard(GuardResolution::Save),
        GuardOutcome::NeedsLiveSave(ProviderLiveFileKind::Config)
    );

    state.cancel_live_file_edit(ProviderLiveFileKind::Config);
    assert!(!state.live_file_dirty(ProviderLiveFileKind::Config));
    assert_eq!(
        state.live_file_draft(ProviderLiveFileKind::Config),
        Some("model = \"model-a\"\n")
    );
    assert!(state.begin_live_file_edit(ProviderLiveFileKind::Auth));
    state.set_live_file_revealed(ProviderLiveFileKind::Auth, true);
    assert!(state.live_file_revealed(ProviderLiveFileKind::Auth));
    state.leave_provider_route();
    assert!(!state.live_file_revealed(ProviderLiveFileKind::Auth));
    assert!(!state.live_file_editing(ProviderLiveFileKind::Auth));
}

#[test]
fn live_refresh_failure_keeps_last_good_state_and_reports_a_typed_kind() {
    let mut state = live_loaded_state();
    let before = Arc::clone(state.live.workspace.as_ref().unwrap());
    let request_id = state.begin_live_load().unwrap();

    assert!(state.apply_live_load_response(
        request_id,
        Err(LiveLoadFailureKind::Activation(
            ProviderActivationErrorKind::LockFailed,
        )),
    ));
    assert!(Arc::ptr_eq(state.live.workspace.as_ref().unwrap(), &before));
    assert_eq!(
        state.live.load_error,
        Some(LiveLoadFailureKind::Activation(
            ProviderActivationErrorKind::LockFailed,
        ))
    );
}
