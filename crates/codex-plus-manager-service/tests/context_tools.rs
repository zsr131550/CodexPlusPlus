use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use codex_plus_core::context_ownership::{
    ContextOwnershipManifest, OwnedContextEntry, load_context_ownership_at,
    save_context_ownership_at,
};
use codex_plus_core::settings::{
    BackendSettings, RelayContextSelection, RelayProfile, SettingsStore,
};
use codex_plus_manager_service::{
    CompatContextDeleteRequest, ContextEntryKey, ContextEntryLiveState, ContextKind,
    ContextOwnershipOutcome, ContextSyncGuard, ContextSyncScope, ContextToolsEnvironment,
    ContextToolsErrorKind, ContextToolsService, DeleteContextEntry, LoadContextEntryDraft,
    PreviewContextSync, ProviderActivationEnvironment, ProviderEnvironment, SaveContextEntry,
    SaveContextEntryMode, SetContextEntryEnabled, SyncContextToLive, SystemProviderEnvironment,
};
use serde_json::{Value, json};

const SECRET: &str = "context-service-secret-sentinel-7f31";

struct Fixture {
    _temp: tempfile::TempDir,
    settings_path: PathBuf,
    home: PathBuf,
    ownership_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let home = temp.path().join("codex");
        let ownership_path = temp.path().join("context-live-ownership.json");
        fs::create_dir(&home).unwrap();

        let settings = BackendSettings {
            relay_profiles_enabled: true,
            active_relay_id: "relay-a".to_string(),
            relay_profiles: vec![RelayProfile {
                id: "relay-a".to_string(),
                name: "Relay A".to_string(),
                context_selection_initialized: true,
                context_selection: RelayContextSelection {
                    mcp_servers: vec!["alpha".to_string()],
                    skills: vec!["writer".to_string()],
                    plugins: Vec::new(),
                },
                ..RelayProfile::default()
            }],
            relay_context_config_contents: stored_context(),
            relay_test_model: "model-a".to_string(),
            ..BackendSettings::default()
        };
        write_settings(&settings_path, &settings, Some(json!({"nested": true})));
        fs::write(home.join("config.toml"), live_context()).unwrap();
        fs::write(home.join("auth.json"), r#"{"auth":"unchanged"}"#).unwrap();
        save_context_ownership_at(
            &ownership_path,
            &ContextOwnershipManifest {
                version: 1,
                entries: vec![
                    OwnedContextEntry {
                        identity: codex_plus_core::context_ownership::ContextEntryIdentity {
                            kind: "mcp".to_string(),
                            id: "alpha".to_string(),
                        },
                        body_sha256: "a".repeat(64),
                    },
                    OwnedContextEntry {
                        identity: codex_plus_core::context_ownership::ContextEntryIdentity {
                            kind: "plugin".to_string(),
                            id: "browser".to_string(),
                        },
                        body_sha256: "b".repeat(64),
                    },
                ],
            },
        )
        .unwrap();

        Self {
            _temp: temp,
            settings_path,
            home,
            ownership_path,
        }
    }

    fn service(&self) -> ContextToolsService<SystemProviderEnvironment> {
        ContextToolsService::new(
            SystemProviderEnvironment::for_paths(self.settings_path.clone(), self.home.clone())
                .with_context_ownership_path(self.ownership_path.clone()),
        )
    }

    fn store(&self) -> SettingsStore {
        SettingsStore::new(self.settings_path.clone())
    }
}

fn write_settings(path: &Path, settings: &BackendSettings, unknown: Option<Value>) {
    let mut value = serde_json::to_value(settings).unwrap();
    if let Some(unknown) = unknown {
        value
            .as_object_mut()
            .unwrap()
            .insert("customField".to_string(), unknown);
    }
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn stored_context() -> String {
    format!(
        r#"[mcp_servers.alpha]
command = "{SECRET}"
args = ["--token", "{SECRET}"]

[skills.writer]
enabled = true
instructions = "stored"

[plugins.browser]
enabled = false
token = "{SECRET}"
"#
    )
}

fn live_context() -> String {
    format!(
        r#"model = "gpt"

[mcp_servers.alpha]
command = "{SECRET}"
args = ["--token", "{SECRET}"]

[skills.writer]
enabled = true
instructions = "live-different"

[plugins.browser]
enabled = true
token = "old-live"

[mcp_servers.manual]
command = "manual"
"#
    )
}

fn key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_string(),
    }
}

fn guard(workspace: &codex_plus_manager_service::ContextWorkspace) -> ContextSyncGuard {
    ContextSyncGuard {
        expected_provider_revision: workspace.provider_revision.clone(),
        expected_live_revision: workspace.live_revision.clone(),
        expected_ownership_revision: workspace.ownership_revision.clone(),
    }
}

fn enable_browser(fixture: &Fixture, initialized: bool) {
    let mut settings = fixture.store().load().unwrap();
    settings.relay_context_config_contents = format!(
        r#"[mcp_servers.alpha]
command = "{SECRET}"
args = ["--token", "{SECRET}"]

[skills.writer]
enabled = true
instructions = "stored"

[plugins.browser]
enabled = true
token = "{SECRET}"
"#
    );
    settings.relay_profiles[0].context_selection_initialized = initialized;
    fixture.store().save(&settings).unwrap();
}

#[test]
fn workspace_contains_safe_summaries_and_three_revisions() {
    let fixture = Fixture::new();

    let bundle = fixture.service().load_workspace().unwrap();
    let workspace = bundle.context;

    assert_eq!(workspace.provider_revision.as_str().len(), 64);
    assert_eq!(workspace.live_revision.as_str().len(), 64);
    assert_eq!(workspace.ownership_revision.as_str().len(), 64);
    assert_eq!(workspace.active_provider_id.as_deref(), Some("relay-a"));
    assert_eq!(workspace.active_provider_name.as_deref(), Some("Relay A"));
    assert_eq!(workspace.entries.len(), 3);
    assert_eq!(workspace.unmanaged_live_count, 1);
    assert!(workspace.sync_needed);
    assert_eq!(workspace.entries[0].display_name, "alpha");
    assert_eq!(
        workspace.entries[0].live_state,
        ContextEntryLiveState::Matching
    );
    assert_eq!(
        workspace.entries[1].live_state,
        ContextEntryLiveState::Different
    );
    assert_eq!(
        workspace.entries[2].live_state,
        ContextEntryLiveState::PendingRemoval
    );
}

#[test]
fn workspace_never_exposes_core_first_line_summary() {
    let fixture = Fixture::new();

    let bundle = fixture.service().load_workspace().unwrap();
    let debug = format!("{bundle:?}");

    assert!(!debug.contains(SECRET));
    assert!(!debug.contains("--token"));
    assert!(
        bundle
            .context
            .entries
            .iter()
            .all(|entry| entry.display_name == entry.key.id)
    );
}

#[test]
fn draft_load_is_explicit_and_debug_redacted() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let draft = service
        .load_entry_draft(LoadContextEntryDraft {
            expected_provider_revision: workspace.context.provider_revision,
            key: key(ContextKind::Mcp, "alpha"),
        })
        .unwrap();

    assert!(draft.toml_body.contains(SECRET));
    assert!(!format!("{draft:?}").contains(SECRET));
}

#[test]
fn compatibility_delete_request_debug_redacts_settings() {
    let fixture = Fixture::new();
    let request = CompatContextDeleteRequest {
        settings: fixture.store().load().unwrap(),
        kind: "plugin".to_string(),
        id: "browser".to_string(),
    };

    let debug = format!("{request:?}");

    assert!(debug.contains("CompatContextDeleteRequest"));
    assert!(debug.contains("browser"));
    assert!(!debug.contains(SECRET));
    assert!(!debug.contains("--token"));
}

#[test]
fn save_entry_preserves_unknown_settings_profiles_and_selections() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let before = fixture.store().load().unwrap();
    let revision = service.load_workspace().unwrap().context.provider_revision;

    service
        .save_entry(SaveContextEntry {
            expected_provider_revision: revision,
            mode: SaveContextEntryMode::Edit,
            key: key(ContextKind::Skill, "writer"),
            toml_body: "enabled = true\ninstructions = \"updated\"\n".to_string(),
        })
        .unwrap();

    let after = fixture.store().load().unwrap();
    let raw: Value = serde_json::from_slice(&fs::read(&fixture.settings_path).unwrap()).unwrap();
    assert_eq!(raw["customField"], json!({"nested": true}));
    assert_eq!(after.relay_profiles, before.relay_profiles);
    assert_eq!(after.active_relay_id, before.active_relay_id);
    assert_eq!(after.relay_test_model, before.relay_test_model);
    assert!(after.relay_context_config_contents.contains("updated"));
}

#[test]
fn toggle_changes_only_relay_context_config_contents() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let before = fixture.store().load().unwrap();
    let revision = service.load_workspace().unwrap().context.provider_revision;

    service
        .set_entry_enabled(SetContextEntryEnabled {
            expected_provider_revision: revision,
            key: key(ContextKind::Skill, "writer"),
            enabled: false,
        })
        .unwrap();

    let after = fixture.store().load().unwrap();
    let mut expected = before;
    expected.relay_context_config_contents = after.relay_context_config_contents.clone();
    assert_eq!(after, expected);
    assert!(
        after
            .relay_context_config_contents
            .contains("enabled = false")
    );
}

#[test]
fn delete_requires_exact_existing_kind_and_id() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();
    let bytes_before = fs::read(&fixture.settings_path).unwrap();

    let error = service
        .delete_entry(DeleteContextEntry {
            expected_provider_revision: workspace.context.provider_revision.clone(),
            key: key(ContextKind::Plugin, "browser"),
            confirmed_key: key(ContextKind::Plugin, "other"),
        })
        .unwrap_err();
    assert_eq!(error.kind(), ContextToolsErrorKind::ConfirmationMismatch);
    assert_eq!(fs::read(&fixture.settings_path).unwrap(), bytes_before);

    let bundle = service
        .delete_entry(DeleteContextEntry {
            expected_provider_revision: workspace.context.provider_revision,
            key: key(ContextKind::Plugin, "browser"),
            confirmed_key: key(ContextKind::Plugin, "browser"),
        })
        .unwrap();
    assert!(
        !bundle
            .context
            .entries
            .iter()
            .any(|entry| entry.key.id == "browser")
    );
}

#[test]
fn stale_provider_revision_rejects_every_stored_mutation() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let stale = service.load_workspace().unwrap().context.provider_revision;
    fixture
        .store()
        .update(json!({"relayTestModel": "concurrent"}))
        .unwrap();
    let bytes_after_concurrent_write = fs::read(&fixture.settings_path).unwrap();

    let errors = [
        service
            .save_entry(SaveContextEntry {
                expected_provider_revision: stale.clone(),
                mode: SaveContextEntryMode::Edit,
                key: key(ContextKind::Mcp, "alpha"),
                toml_body: "command = \"changed\"\n".to_string(),
            })
            .unwrap_err(),
        service
            .set_entry_enabled(SetContextEntryEnabled {
                expected_provider_revision: stale.clone(),
                key: key(ContextKind::Mcp, "alpha"),
                enabled: false,
            })
            .unwrap_err(),
        service
            .delete_entry(DeleteContextEntry {
                expected_provider_revision: stale,
                key: key(ContextKind::Mcp, "alpha"),
                confirmed_key: key(ContextKind::Mcp, "alpha"),
            })
            .unwrap_err(),
    ];

    assert!(
        errors
            .iter()
            .all(|error| error.kind() == ContextToolsErrorKind::ProviderConflict)
    );
    assert_eq!(
        fs::read(&fixture.settings_path).unwrap(),
        bytes_after_concurrent_write
    );
}

#[test]
fn stored_mutations_never_write_live_config_or_manifest() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let live_before = fs::read(fixture.home.join("config.toml")).unwrap();
    let auth_before = fs::read(fixture.home.join("auth.json")).unwrap();
    let ownership_before = fs::read(&fixture.ownership_path).unwrap();

    let first = service.load_workspace().unwrap();
    let second = service
        .save_entry(SaveContextEntry {
            expected_provider_revision: first.context.provider_revision,
            mode: SaveContextEntryMode::Create,
            key: key(ContextKind::Mcp, "new-entry"),
            toml_body: "command = \"new\"\n".to_string(),
        })
        .unwrap();
    let third = service
        .set_entry_enabled(SetContextEntryEnabled {
            expected_provider_revision: second.context.provider_revision,
            key: key(ContextKind::Mcp, "new-entry"),
            enabled: false,
        })
        .unwrap();
    service
        .delete_entry(DeleteContextEntry {
            expected_provider_revision: third.context.provider_revision,
            key: key(ContextKind::Mcp, "new-entry"),
            confirmed_key: key(ContextKind::Mcp, "new-entry"),
        })
        .unwrap();

    assert_eq!(
        fs::read(fixture.home.join("config.toml")).unwrap(),
        live_before
    );
    assert_eq!(
        fs::read(fixture.home.join("auth.json")).unwrap(),
        auth_before
    );
    assert_eq!(fs::read(&fixture.ownership_path).unwrap(), ownership_before);
}

#[test]
fn native_preview_uses_active_provider_effective_selection() {
    let fixture = Fixture::new();
    enable_browser(&fixture, true);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let preview = service
        .preview_context_sync(PreviewContextSync {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    assert_eq!(preview.active_provider_id.as_deref(), Some("relay-a"));
    assert!(
        preview
            .keys
            .removed
            .contains(&key(ContextKind::Plugin, "browser"))
    );
    assert!(
        !preview
            .keys
            .updated
            .contains(&key(ContextKind::Plugin, "browser"))
    );
    assert_eq!(preview.diff.removed, 1);
}

#[test]
fn workspace_status_uses_active_provider_effective_selection() {
    let fixture = Fixture::new();
    enable_browser(&fixture, false);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();
    service
        .sync_context_to_live(SyncContextToLive {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    let mut settings = fixture.store().load().unwrap();
    settings.relay_profiles[0].context_selection_initialized = true;
    fixture.store().save(&settings).unwrap();

    let workspace = service.load_workspace().unwrap().context;
    let browser = workspace
        .entries
        .iter()
        .find(|entry| entry.key == key(ContextKind::Plugin, "browser"))
        .unwrap();
    assert!(workspace.sync_needed);
    assert_eq!(browser.live_state, ContextEntryLiveState::PendingRemoval);
}

#[test]
fn disabled_provider_is_not_exposed_as_a_native_sync_target() {
    let fixture = Fixture::new();
    let mut settings = fixture.store().load().unwrap();
    settings.relay_profiles_enabled = false;
    fixture.store().save(&settings).unwrap();

    let workspace = fixture.service().load_workspace().unwrap().context;

    assert_eq!(workspace.active_provider_id, None);
    assert_eq!(workspace.active_provider_name, None);
}

#[test]
fn uninitialized_selection_previews_all_enabled_entries() {
    let fixture = Fixture::new();
    enable_browser(&fixture, false);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let preview = service
        .preview_context_sync(PreviewContextSync {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    assert!(
        preview
            .keys
            .updated
            .contains(&key(ContextKind::Plugin, "browser"))
    );
    assert!(
        !preview
            .keys
            .removed
            .contains(&key(ContextKind::Plugin, "browser"))
    );
}

#[test]
fn missing_active_provider_blocks_native_preview() {
    let fixture = Fixture::new();
    let mut settings = fixture.store().load().unwrap();
    settings.active_relay_id = "missing".to_string();
    fixture.store().save(&settings).unwrap();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let error = service
        .preview_context_sync(PreviewContextSync {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap_err();

    assert_eq!(error.kind(), ContextToolsErrorKind::ActiveProviderMissing);
}

#[test]
fn legacy_preview_uses_all_enabled_global_entries() {
    let fixture = Fixture::new();
    enable_browser(&fixture, true);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let preview = service
        .preview_context_sync(PreviewContextSync {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::AllEnabledGlobal,
        })
        .unwrap();

    assert_eq!(preview.active_provider_id, None);
    assert!(
        preview
            .keys
            .updated
            .contains(&key(ContextKind::Plugin, "browser"))
    );
    assert!(
        !preview
            .keys
            .removed
            .contains(&key(ContextKind::Plugin, "browser"))
    );
}

#[test]
fn provider_live_and_ownership_conflicts_write_nothing() {
    for conflict in ["provider", "live", "ownership"] {
        let fixture = Fixture::new();
        let service = fixture.service();
        let workspace = service.load_workspace().unwrap();
        match conflict {
            "provider" => {
                fixture
                    .store()
                    .update(json!({"relayTestModel": "concurrent"}))
                    .unwrap();
            }
            "live" => {
                fs::write(
                    fixture.home.join("config.toml"),
                    format!("{}\n# concurrent\n", live_context()),
                )
                .unwrap();
            }
            "ownership" => {
                save_context_ownership_at(
                    &fixture.ownership_path,
                    &ContextOwnershipManifest::default(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let live_before = fs::read(fixture.home.join("config.toml")).unwrap();
        let ownership_before = fs::read(&fixture.ownership_path).unwrap();

        let error = service
            .sync_context_to_live(SyncContextToLive {
                guard: guard(&workspace.context),
                scope: ContextSyncScope::ActiveProvider,
            })
            .unwrap_err();

        let expected = match conflict {
            "provider" => ContextToolsErrorKind::ProviderConflict,
            "live" => ContextToolsErrorKind::LiveConflict,
            "ownership" => ContextToolsErrorKind::OwnershipConflict,
            _ => unreachable!(),
        };
        assert_eq!(error.kind(), expected);
        assert_eq!(
            fs::read(fixture.home.join("config.toml")).unwrap(),
            live_before
        );
        assert_eq!(fs::read(&fixture.ownership_path).unwrap(), ownership_before);
    }
}

#[test]
fn successful_sync_writes_backup_then_manifest_and_returns_fresh_bundle() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();
    let settings_before = fs::read(&fixture.settings_path).unwrap();

    let outcome = service
        .sync_context_to_live(SyncContextToLive {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    assert_eq!(outcome.ownership, ContextOwnershipOutcome::Updated);
    let backup = outcome.backup_path.as_ref().unwrap();
    assert!(Path::new(backup).exists());
    assert_ne!(
        outcome.bundle.context.live_revision,
        workspace.context.live_revision
    );
    assert_ne!(
        outcome.bundle.context.ownership_revision,
        workspace.context.ownership_revision
    );
    assert_eq!(fs::read(&fixture.settings_path).unwrap(), settings_before);
    let live = fs::read_to_string(fixture.home.join("config.toml")).unwrap();
    assert!(live.contains("instructions = \"stored\""));
    assert!(!live.contains("[plugins.browser]"));
}

#[test]
fn native_sync_can_clear_the_last_owned_live_entry() {
    let fixture = Fixture::new();
    let mut settings = fixture.store().load().unwrap();
    settings.relay_context_config_contents.clear();
    fixture.store().save(&settings).unwrap();
    fs::write(
        fixture.home.join("config.toml"),
        "[mcp_servers.alpha]\ncommand = \"owned\"\n",
    )
    .unwrap();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let outcome = service
        .sync_context_to_live(SyncContextToLive {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    assert!(
        fs::read_to_string(fixture.home.join("config.toml"))
            .unwrap()
            .trim()
            .is_empty()
    );
    assert!(outcome.backup_path.is_some());
    assert!(
        load_context_ownership_at(&fixture.ownership_path)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn legacy_sync_can_clear_the_last_owned_live_entry() {
    let fixture = Fixture::new();
    let mut settings = fixture.store().load().unwrap();
    settings.relay_context_config_contents.clear();
    fixture.store().save(&settings).unwrap();
    fs::write(
        fixture.home.join("config.toml"),
        "[mcp_servers.alpha]\ncommand = \"owned\"\n",
    )
    .unwrap();

    let entries = fixture.service().sync_all_global_compat(&settings).unwrap();

    assert!(entries.mcp_servers.is_empty());
    assert!(
        fs::read_to_string(fixture.home.join("config.toml"))
            .unwrap()
            .trim()
            .is_empty()
    );
    assert!(
        load_context_ownership_at(&fixture.ownership_path)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[derive(Clone)]
struct FailingOwnershipEnvironment {
    inner: SystemProviderEnvironment,
}

impl ProviderEnvironment for FailingOwnershipEnvironment {
    fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        self.inner.load_settings()
    }

    fn update_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        self.inner.update_settings_if(payload, predicate)
    }
}

impl ProviderActivationEnvironment for FailingOwnershipEnvironment {
    fn settings_store(&self) -> &SettingsStore {
        self.inner.settings_store()
    }

    fn codex_home(&self) -> &Path {
        self.inner.codex_home()
    }
}

impl ContextToolsEnvironment for FailingOwnershipEnvironment {
    fn load_context_ownership(&self) -> anyhow::Result<ContextOwnershipManifest> {
        self.inner.load_context_ownership()
    }

    fn save_context_ownership(&self, _manifest: &ContextOwnershipManifest) -> anyhow::Result<()> {
        anyhow::bail!("injected ownership save failure")
    }
}

#[test]
fn manifest_save_failure_reports_partial_success_and_preserves_old_manifest() {
    let fixture = Fixture::new();
    let environment =
        SystemProviderEnvironment::for_paths(fixture.settings_path.clone(), fixture.home.clone())
            .with_context_ownership_path(fixture.ownership_path.clone());
    let service = ContextToolsService::new(FailingOwnershipEnvironment { inner: environment });
    let workspace = service.load_workspace().unwrap();
    let ownership_before = fs::read(&fixture.ownership_path).unwrap();

    let outcome = service
        .sync_context_to_live(SyncContextToLive {
            guard: guard(&workspace.context),
            scope: ContextSyncScope::ActiveProvider,
        })
        .unwrap();

    assert_eq!(outcome.ownership, ContextOwnershipOutcome::PartialFailure);
    assert!(
        outcome
            .backup_path
            .as_ref()
            .is_some_and(|path| Path::new(path).exists())
    );
    assert_ne!(
        outcome.bundle.context.live_revision,
        workspace.context.live_revision
    );
    assert_eq!(
        outcome.bundle.context.ownership_revision,
        workspace.context.ownership_revision
    );
    assert_eq!(fs::read(&fixture.ownership_path).unwrap(), ownership_before);
    assert!(!format!("{outcome:?}").contains(SECRET));
}

#[test]
fn two_service_handles_serialize_live_sync() {
    let fixture = Fixture::new();
    let first = fixture.service();
    let second = fixture.service();
    let workspace = first.load_workspace().unwrap();
    let request = SyncContextToLive {
        guard: guard(&workspace.context),
        scope: ContextSyncScope::ActiveProvider,
    };
    let barrier = Arc::new(Barrier::new(3));
    let first_barrier = Arc::clone(&barrier);
    let first_request = request.clone();
    let first_thread = std::thread::spawn(move || {
        first_barrier.wait();
        first.sync_context_to_live(first_request)
    });
    let second_barrier = Arc::clone(&barrier);
    let second_thread = std::thread::spawn(move || {
        second_barrier.wait();
        second.sync_context_to_live(request)
    });
    barrier.wait();

    let results = [first_thread.join().unwrap(), second_thread.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .all(|error| {
                matches!(
                    error.kind(),
                    ContextToolsErrorKind::LiveConflict | ContextToolsErrorKind::OwnershipConflict
                )
            })
    );
}
