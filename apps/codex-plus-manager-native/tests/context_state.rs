use std::sync::Arc;

use codex_plus_core::context_ownership::ContextOwnershipRevision;
use codex_plus_manager_native::state::context::{ContextFailureKind, ContextViewState};
use codex_plus_manager_native::state::{AppState, Route};
use codex_plus_manager_service::{
    ContextBundle, ContextEntryDraft, ContextEntryKey, ContextEntryLiveState, ContextEntrySummary,
    ContextKind, ContextOwnershipOutcome, ContextSyncDiffSummary, ContextSyncGuard,
    ContextSyncKeys, ContextSyncOutcome, ContextSyncPreview, ContextToolsErrorKind,
    ContextWorkspace, ProviderActivationSummary, ProviderDocument, ProviderLiveRevision,
    ProviderRevision, ProviderWorkspace,
};

const SECRET: &str = "native-context-state-secret-sentinel";

fn revision(character: char) -> ProviderRevision {
    ProviderRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn live_revision(character: char) -> ProviderLiveRevision {
    ProviderLiveRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn ownership_revision(character: char) -> ContextOwnershipRevision {
    ContextOwnershipRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_string(),
    }
}

fn bundle(character: char) -> Arc<ContextBundle> {
    let provider_revision = revision(character);
    Arc::new(ContextBundle {
        context: ContextWorkspace {
            provider_revision: provider_revision.clone(),
            live_revision: live_revision(character),
            ownership_revision: ownership_revision(character),
            active_provider_id: Some("relay-a".to_string()),
            active_provider_name: Some("Relay A".to_string()),
            entries: vec![ContextEntrySummary {
                key: key(ContextKind::Mcp, "alpha"),
                display_name: "alpha".to_string(),
                enabled: true,
                live_state: ContextEntryLiveState::Matching,
            }],
            unmanaged_live_count: 0,
            sync_needed: character != 'b',
        },
        provider: ProviderWorkspace {
            revision: provider_revision,
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: true,
                active_profile_id: None,
                active_profile_kind: None,
            },
            context_options: codex_plus_core::relay_config::CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    })
}

fn load(state: &mut ContextViewState, value: Arc<ContextBundle>) {
    let request_id = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(request_id, Ok(value)));
}

fn preview(value: &ContextBundle) -> Arc<ContextSyncPreview> {
    Arc::new(ContextSyncPreview {
        guard: ContextSyncGuard {
            expected_provider_revision: value.context.provider_revision.clone(),
            expected_live_revision: value.context.live_revision.clone(),
            expected_ownership_revision: value.context.ownership_revision.clone(),
        },
        active_provider_id: value.context.active_provider_id.clone(),
        diff: ContextSyncDiffSummary {
            added: 1,
            updated: 0,
            removed: 1,
            unchanged: 1,
        },
        keys: ContextSyncKeys {
            added: vec![key(ContextKind::Mcp, "alpha")],
            updated: Vec::new(),
            removed: vec![key(ContextKind::Plugin, "browser")],
            unchanged: vec![key(ContextKind::Skill, "writer")],
        },
    })
}

#[test]
fn refresh_accepts_only_latest_request() {
    let mut state = ContextViewState::default();
    let first = state.begin_workspace_refresh();
    let second = state.begin_workspace_refresh();

    assert!(!state.apply_workspace_response(first, Ok(bundle('a'))));
    assert!(state.apply_workspace_response(second, Ok(bundle('b'))));
    assert_eq!(
        state.bundle.as_ref().unwrap().context.provider_revision,
        revision('b')
    );
}

#[test]
fn focus_refresh_preserves_open_editor_draft() {
    let mut state = ContextViewState::default();
    load(&mut state, bundle('a'));
    assert!(state.open_create(ContextKind::Plugin));
    assert!(state.set_editor_id("browser".to_string()));
    assert!(state.set_editor_body(format!("token = \"{SECRET}\"\n")));

    let refresh = state.begin_workspace_refresh();
    assert!(state.apply_workspace_response(refresh, Ok(bundle('b'))));

    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.id, "browser");
    assert!(editor.toml_body.contains(SECRET));
}

#[test]
fn draft_load_accepts_only_matching_request_and_revision() {
    let mut state = ContextViewState::default();
    load(&mut state, bundle('a'));
    let target = key(ContextKind::Mcp, "alpha");
    let (request_id, _) = state.begin_edit(target.clone()).unwrap();
    let stale = Arc::new(ContextEntryDraft {
        provider_revision: revision('b'),
        key: target.clone(),
        toml_body: format!("command = \"{SECRET}\"\n"),
    });
    assert!(!state.apply_draft_response(request_id, Ok(stale)));
    assert!(state.editor.is_none());

    let (request_id, _) = state.begin_edit(target.clone()).unwrap();
    let matching = Arc::new(ContextEntryDraft {
        provider_revision: revision('a'),
        key: target,
        toml_body: format!("command = \"{SECRET}\"\n"),
    });
    assert!(state.apply_draft_response(request_id, Ok(matching)));
    assert!(state.editor.as_ref().unwrap().toml_body.contains(SECRET));
}

#[test]
fn stored_mutation_conflict_preserves_draft_and_last_workspace() {
    let mut state = ContextViewState::default();
    load(&mut state, bundle('a'));
    state.open_create(ContextKind::Skill);
    state.set_editor_id("writer".to_string());
    state.set_editor_body(format!("instructions = \"{SECRET}\"\n"));
    let (request_id, _) = state.begin_save().unwrap();

    assert!(state.apply_stored_mutation_response(
        request_id,
        Err(ContextFailureKind::Service(
            ContextToolsErrorKind::ProviderConflict
        )),
    ));

    assert!(state.editor.as_ref().unwrap().toml_body.contains(SECRET));
    assert_eq!(
        state.bundle.as_ref().unwrap().context.provider_revision,
        revision('a')
    );
}

#[test]
fn delete_confirmation_repeats_exact_key() {
    let mut state = ContextViewState::default();
    load(&mut state, bundle('a'));
    let target = key(ContextKind::Mcp, "alpha");

    assert!(state.request_delete(target.clone()));
    assert_eq!(state.delete_confirmation.as_ref(), Some(&target));
    let (_, request) = state.begin_delete().unwrap();
    assert_eq!(request.key, target);
    assert_eq!(request.confirmed_key, request.key);
}

#[test]
fn sync_preview_is_metadata_only_and_sorted() {
    let mut state = ContextViewState::default();
    let initial = bundle('a');
    load(&mut state, Arc::clone(&initial));
    let (request_id, _) = state.begin_preview().unwrap();

    assert!(state.apply_preview_response(request_id, Ok(preview(&initial))));
    let debug = format!("{:?}", state.sync_preview.as_ref().unwrap());
    assert!(!debug.contains(SECRET));
    assert_eq!(state.sync_preview.as_ref().unwrap().diff.added, 1);
}

#[test]
fn sync_confirmation_rejects_stale_preview() {
    let mut state = ContextViewState::default();
    let initial = bundle('a');
    load(&mut state, Arc::clone(&initial));
    let (request_id, _) = state.begin_preview().unwrap();
    state.apply_preview_response(request_id, Ok(preview(&initial)));

    let refresh = state.begin_workspace_refresh();
    state.apply_workspace_response(refresh, Ok(bundle('b')));

    assert!(state.begin_sync().is_none());
}

#[test]
fn partial_sync_keeps_backup_and_repair_state() {
    let mut state = ContextViewState::default();
    let initial = bundle('a');
    load(&mut state, Arc::clone(&initial));
    let (preview_id, _) = state.begin_preview().unwrap();
    state.apply_preview_response(preview_id, Ok(preview(&initial)));
    let (sync_id, _) = state.begin_sync().unwrap();
    let outcome = Arc::new(ContextSyncOutcome {
        bundle: (*bundle('b')).clone(),
        backup_path: Some("C:/private/backup.toml".to_string()),
        ownership: ContextOwnershipOutcome::PartialFailure,
        diff: ContextSyncDiffSummary {
            added: 1,
            updated: 0,
            removed: 1,
            unchanged: 1,
        },
    });

    assert!(state.apply_sync_response(sync_id, Ok(Arc::clone(&outcome))));
    assert_eq!(
        state.sync_outcome.as_ref().unwrap().ownership,
        ContextOwnershipOutcome::PartialFailure
    );
    assert!(state.sync_outcome.as_ref().unwrap().backup_path.is_some());
}

#[test]
fn successful_bundle_replaces_context_and_provider_revisions_together() {
    let mut app = AppState {
        route: Route::Context,
        ..AppState::default()
    };
    let load_id = app.context.begin_workspace_refresh();
    assert!(app.apply_context_workspace_response(load_id, Ok(bundle('a'))));
    assert_eq!(
        app.provider.baseline.as_ref().unwrap().revision,
        app.context
            .bundle
            .as_ref()
            .unwrap()
            .context
            .provider_revision
    );

    app.context.open_create(ContextKind::Skill);
    app.context.set_editor_id("writer".to_string());
    app.context.set_editor_body("enabled = true\n".to_string());
    let (save_id, _) = app.context.begin_save().unwrap();
    assert!(app.apply_context_stored_mutation_response(save_id, Ok(bundle('b'))));

    assert_eq!(
        app.provider.baseline.as_ref().unwrap().revision,
        app.context
            .bundle
            .as_ref()
            .unwrap()
            .context
            .provider_revision
    );
    assert_eq!(
        app.context
            .bundle
            .as_ref()
            .unwrap()
            .context
            .provider_revision,
        revision('b')
    );
}

#[test]
fn late_context_bundle_never_overwrites_a_dirty_provider_draft() {
    let mut app = AppState::default();
    let load_id = app.context.begin_workspace_refresh();
    assert!(app.apply_context_workspace_response(load_id, Ok(bundle('a'))));
    app.provider.draft_mut().unwrap().default_test_model = "unsaved-provider-draft".to_owned();

    let refresh_id = app.context.begin_workspace_refresh();
    assert!(app.apply_context_workspace_response(refresh_id, Ok(bundle('b'))));

    assert_eq!(
        app.provider.draft().unwrap().default_test_model,
        "unsaved-provider-draft"
    );
    assert_eq!(
        app.context.workspace_error,
        Some(ContextFailureKind::Service(
            ContextToolsErrorKind::ProviderConflict
        ))
    );
    assert_eq!(
        app.context
            .bundle
            .as_ref()
            .unwrap()
            .context
            .provider_revision,
        revision('a')
    );
}

#[test]
fn state_debug_never_exposes_toml_sentinel() {
    let mut state = ContextViewState::default();
    load(&mut state, bundle('a'));
    state.open_create(ContextKind::Mcp);
    state.set_editor_id("alpha".to_string());
    state.set_editor_body(format!("command = \"{SECRET}\"\n"));

    assert!(!format!("{state:?}").contains(SECRET));
}
