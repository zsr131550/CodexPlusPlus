use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use codex_plus_core::settings::BackendSettings;
use codex_plus_core::zed_remote::{
    SshTarget, ZedAvailability, ZedLaunchPlan, ZedOpenStrategy, ZedRemoteError as CoreZedError,
    ZedRemoteProject, ZedRemoteProjectSource, ZedRemoteRegistryStore,
};
use codex_plus_manager_service::{
    ForgetZedRemoteProject, OpenZedRemoteProject, SaveZedPreferences, SystemProviderEnvironment,
    ZedRememberOutcome, ZedRemoteEnvironment, ZedRemoteErrorKind, ZedRemoteProjectSummary,
    ZedRemoteService, ZedRemoteSource, ZedRemoteWorkspace,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct FakeZedEnvironment {
    _root: Arc<tempfile::TempDir>,
    settings: Arc<Mutex<BackendSettings>>,
    global_state: Arc<Mutex<Option<Value>>>,
    request_context: Value,
    registry_path: PathBuf,
    sqlite_paths: Vec<PathBuf>,
    preference_writes: Arc<AtomicUsize>,
    launch_calls: Arc<Mutex<Vec<ZedLaunchPlan>>>,
    launch_failure: Arc<AtomicBool>,
    mutate_registry_after_launch: Arc<AtomicBool>,
}

impl FakeZedEnvironment {
    fn new() -> Self {
        let root = Arc::new(tempfile::tempdir().unwrap());
        let registry_path = root.path().join("recent.json");
        std::fs::write(
            &registry_path,
            serde_json::to_vec(&json!({
                "projects": [recent_project("recent", "/srv/recent", 40)]
            }))
            .unwrap(),
        )
        .unwrap();

        let sqlite_path = root.path().join("state.sqlite");
        let database = rusqlite::Connection::open(&sqlite_path).unwrap();
        database
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL)",
                [],
            )
            .unwrap();
        database
            .execute(
                "INSERT INTO threads (id, cwd) VALUES (?1, ?2)",
                ("thread-sqlite", "/srv/sqlite"),
            )
            .unwrap();
        drop(database);

        Self {
            _root: root,
            settings: Arc::new(Mutex::new(BackendSettings::default())),
            global_state: Arc::new(Mutex::new(Some(json!({
                "selected-remote-host-id": "host-main",
                "codex-managed-remote-connections": [{
                    "hostId": "host-main",
                    "hostname": "alice@host-sentinel.example.test",
                    "sshPort": 2222
                }],
                "remote-projects": [{
                    "id": "remote-main",
                    "hostId": "host-main",
                    "remotePath": "/srv/remote",
                    "label": "remote-sentinel"
                }],
                "project-order": ["remote-main"],
                "thread-workspace-root-hints": {
                    "thread-hint": "/srv/hint"
                }
            })))),
            request_context: json!({
                "hostId": "host-main",
                "workspaceRoot": "/srv/current"
            }),
            registry_path,
            sqlite_paths: vec![sqlite_path],
            preference_writes: Arc::new(AtomicUsize::new(0)),
            launch_calls: Arc::new(Mutex::new(Vec::new())),
            launch_failure: Arc::new(AtomicBool::new(false)),
            mutate_registry_after_launch: Arc::new(AtomicBool::new(false)),
        }
    }

    fn mutate_settings(&self, mutate: impl FnOnce(&mut BackendSettings)) {
        mutate(&mut self.settings.lock().unwrap());
    }

    fn preference_writes(&self) -> usize {
        self.preference_writes.load(Ordering::SeqCst)
    }

    fn replace_recent(&self, project: ZedRemoteProject) {
        std::fs::write(
            &self.registry_path,
            serde_json::to_vec(&json!({ "projects": [project] })).unwrap(),
        )
        .unwrap();
    }

    fn set_launch_failure(&self, enabled: bool) {
        self.launch_failure.store(enabled, Ordering::SeqCst);
    }

    fn set_mutate_registry_after_launch(&self, enabled: bool) {
        self.mutate_registry_after_launch
            .store(enabled, Ordering::SeqCst);
    }

    fn launch_calls(&self) -> Vec<ZedLaunchPlan> {
        self.launch_calls.lock().unwrap().clone()
    }

    fn registry_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.registry_path).unwrap_or_default()
    }
}

impl ZedRemoteEnvironment for FakeZedEnvironment {
    fn load_zed_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.settings.lock().unwrap().clone())
    }

    fn update_zed_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        let mut settings = self.settings.lock().unwrap();
        if !predicate(&settings) {
            return Ok(None);
        }
        if let Some(strategy) = payload.get("zedRemoteOpenStrategy") {
            settings.zed_remote_open_strategy = serde_json::from_value(strategy.clone())?;
        }
        if let Some(enabled) = payload
            .get("zedRemoteProjectRegistryEnabled")
            .and_then(Value::as_bool)
        {
            settings.zed_remote_project_registry_enabled = enabled;
        }
        self.preference_writes.fetch_add(1, Ordering::SeqCst);
        Ok(Some(settings.clone()))
    }

    fn load_zed_global_state(&self) -> Result<Option<Value>, CoreZedError> {
        Ok(self.global_state.lock().unwrap().clone())
    }

    fn zed_request_context(&self) -> Value {
        self.request_context.clone()
    }

    fn zed_registry_store(&self) -> ZedRemoteRegistryStore {
        ZedRemoteRegistryStore::new(self.registry_path.clone())
    }

    fn zed_sqlite_paths(&self) -> Vec<PathBuf> {
        self.sqlite_paths.clone()
    }

    fn zed_availability(&self) -> ZedAvailability {
        ZedAvailability {
            platform_supported: true,
            cli_found: true,
            app_found: false,
        }
    }

    fn launch_zed_remote(&self, plan: &ZedLaunchPlan) -> Result<(), CoreZedError> {
        self.launch_calls.lock().unwrap().push(plan.clone());
        if self.launch_failure.load(Ordering::SeqCst) {
            return Err(CoreZedError::Launch(std::io::Error::other(
                "fake launch failure",
            )));
        }
        if self.mutate_registry_after_launch.load(Ordering::SeqCst) {
            let store = self.zed_registry_store();
            let snapshot = store.inspect().unwrap();
            let _ = store.remember_if_revision(
                &snapshot.revision,
                recent_project("post-launch", "/srv/post-launch", 100),
            );
        }
        Ok(())
    }
}

#[test]
fn workspace_contains_all_sources_in_priority_order_and_safe_availability() {
    let environment = FakeZedEnvironment::new();
    let workspace = ZedRemoteService::new(environment).load_workspace().unwrap();

    assert_eq!(
        workspace
            .projects
            .iter()
            .map(|project| project.source)
            .collect::<Vec<_>>(),
        vec![
            ZedRemoteProjectSource::CurrentThread,
            ZedRemoteProjectSource::CodexRemoteProject,
            ZedRemoteProjectSource::ThreadWorkspaceHint,
            ZedRemoteProjectSource::SqliteThreadCwd,
            ZedRemoteProjectSource::Recent,
        ]
    );
    assert!(workspace.availability.platform_supported);
    assert!(workspace.availability.cli_found);
    assert!(!workspace.availability.app_found);
}

#[test]
fn project_revision_ignores_presentation_fields_but_detects_launch_identity() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let first = service.load_workspace().unwrap();
    let first_recent = first
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let first_revision = first_recent.revision.clone();

    let mut presentation_only = recent_project("recent", "/srv/recent", 99);
    presentation_only.label = "changed-label-sentinel".to_string();
    presentation_only.is_current = true;
    environment.replace_recent(presentation_only);
    let presentation_revision = service
        .load_workspace()
        .unwrap()
        .projects
        .into_iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap()
        .revision;
    assert_eq!(presentation_revision, first_revision);

    environment.replace_recent(recent_project("recent", "/srv/changed", 99));
    let changed_revision = service
        .load_workspace()
        .unwrap()
        .projects
        .into_iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap()
        .revision;
    assert_ne!(changed_revision, first_revision);
}

#[test]
fn unrelated_settings_change_does_not_stale_zed_preferences() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let before = service.load_workspace().unwrap();
    environment.mutate_settings(|settings| settings.codex_app_path = "unrelated".to_string());

    let after = service
        .save_preferences(SaveZedPreferences {
            expected_revision: before.settings_revision,
            default_strategy: ZedOpenStrategy::ReuseWindow,
            registry_enabled: false,
        })
        .unwrap();

    assert_eq!(after.default_strategy, ZedOpenStrategy::ReuseWindow);
    assert!(!after.registry_enabled);
    assert_eq!(environment.preference_writes(), 1);
}

#[test]
fn either_zed_setting_change_rejects_preference_save_without_a_write() {
    let strategy_environment = FakeZedEnvironment::new();
    let strategy_service = ZedRemoteService::new(strategy_environment.clone());
    let strategy_before = strategy_service.load_workspace().unwrap();
    strategy_environment.mutate_settings(|settings| {
        settings.zed_remote_open_strategy = ZedOpenStrategy::NewWindow;
    });
    let strategy_error = strategy_service
        .save_preferences(SaveZedPreferences {
            expected_revision: strategy_before.settings_revision,
            default_strategy: ZedOpenStrategy::ReuseWindow,
            registry_enabled: false,
        })
        .unwrap_err();
    assert_eq!(strategy_error.kind(), ZedRemoteErrorKind::SettingsConflict);
    assert_eq!(strategy_environment.preference_writes(), 0);

    let registry_environment = FakeZedEnvironment::new();
    let registry_service = ZedRemoteService::new(registry_environment.clone());
    let registry_before = registry_service.load_workspace().unwrap();
    registry_environment.mutate_settings(|settings| {
        settings.zed_remote_project_registry_enabled = false;
    });
    let registry_error = registry_service
        .save_preferences(SaveZedPreferences {
            expected_revision: registry_before.settings_revision,
            default_strategy: ZedOpenStrategy::ReuseWindow,
            registry_enabled: true,
        })
        .unwrap_err();
    assert_eq!(registry_error.kind(), ZedRemoteErrorKind::SettingsConflict);
    assert_eq!(registry_environment.preference_writes(), 0);
}

#[test]
fn open_requires_exact_confirmations_and_rejects_stale_project_before_launch() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let workspace = service.load_workspace().unwrap();
    let project = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();

    let mut request = open_request(project, &workspace, false, ZedOpenStrategy::Default);
    request.confirmed_project_id.push_str("-mismatch");
    assert_eq!(
        service.open_project(request).unwrap_err().kind(),
        ZedRemoteErrorKind::InvalidProject
    );
    assert!(environment.launch_calls().is_empty());

    let mut request = open_request(project, &workspace, false, ZedOpenStrategy::Default);
    request.confirmed_strategy = ZedOpenStrategy::NewWindow;
    assert_eq!(
        service.open_project(request).unwrap_err().kind(),
        ZedRemoteErrorKind::InvalidProject
    );
    assert!(environment.launch_calls().is_empty());

    let mut request = open_request(project, &workspace, false, ZedOpenStrategy::Default);
    request.confirmed_remember = true;
    assert_eq!(
        service.open_project(request).unwrap_err().kind(),
        ZedRemoteErrorKind::InvalidProject
    );
    assert!(environment.launch_calls().is_empty());

    let mut changed = recent_project("recent", "/srv/changed-before-launch", 50);
    changed.id = project.id.clone();
    environment.replace_recent(changed);
    let request = open_request(project, &workspace, false, ZedOpenStrategy::Default);
    assert_eq!(
        service.open_project(request).unwrap_err().kind(),
        ZedRemoteErrorKind::ProjectConflict
    );
    assert!(environment.launch_calls().is_empty());
}

#[test]
fn open_captures_exact_strategy_args_and_remember_false_writes_nothing() {
    for (strategy, expected_args) in [
        (ZedOpenStrategy::AddToFocusedWorkspace, "-a"),
        (ZedOpenStrategy::ReuseWindow, "-r"),
        (ZedOpenStrategy::NewWindow, "-n"),
        (ZedOpenStrategy::Default, ""),
    ] {
        let environment = FakeZedEnvironment::new();
        let service = ZedRemoteService::new(environment.clone());
        let workspace = service.load_workspace().unwrap();
        let project = workspace
            .projects
            .iter()
            .find(|project| project.source == ZedRemoteProjectSource::Recent)
            .unwrap();
        let before = environment.registry_bytes();

        let outcome = service
            .open_project(open_request(project, &workspace, false, strategy))
            .unwrap();
        let calls = environment.launch_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].strategy(), strategy);
        assert_eq!(calls[0].url(), project.url);
        assert_eq!(
            calls[0].args(),
            if expected_args.is_empty() {
                vec![project.url.clone()]
            } else {
                vec![expected_args.to_string(), project.url.clone()]
            }
        );
        assert_eq!(outcome.remember, ZedRememberOutcome::NotRequested);
        assert_eq!(environment.registry_bytes(), before);
    }
}

#[test]
fn open_rejects_stale_registry_before_launch_and_launcher_failure_does_not_write() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let workspace = service.load_workspace().unwrap();
    let project = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let request = open_request(project, &workspace, true, ZedOpenStrategy::Default);
    let store = environment.zed_registry_store();
    let current = store.inspect().unwrap();
    store
        .remember_if_revision(&current.revision, recent_project("other", "/srv/other", 90))
        .unwrap();
    let before = environment.registry_bytes();
    assert_eq!(
        service.open_project(request).unwrap_err().kind(),
        ZedRemoteErrorKind::RegistryConflict
    );
    assert!(environment.launch_calls().is_empty());
    assert_eq!(environment.registry_bytes(), before);

    let failing_environment = FakeZedEnvironment::new();
    failing_environment.set_launch_failure(true);
    let failing_service = ZedRemoteService::new(failing_environment.clone());
    let failing_workspace = failing_service.load_workspace().unwrap();
    let failing_project = failing_workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let before = failing_environment.registry_bytes();
    assert_eq!(
        failing_service
            .open_project(open_request(
                failing_project,
                &failing_workspace,
                true,
                ZedOpenStrategy::Default,
            ))
            .unwrap_err()
            .kind(),
        ZedRemoteErrorKind::LaunchFailed
    );
    assert_eq!(failing_environment.launch_calls().len(), 1);
    assert_eq!(failing_environment.registry_bytes(), before);
}

#[test]
fn remember_success_and_post_launch_failure_are_typed_outcomes() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let workspace = service.load_workspace().unwrap();
    let project = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let outcome = service
        .open_project(open_request(
            project,
            &workspace,
            true,
            ZedOpenStrategy::Default,
        ))
        .unwrap();
    assert_eq!(outcome.remember, ZedRememberOutcome::Remembered);
    assert_eq!(environment.launch_calls().len(), 1);

    let conflict_environment = FakeZedEnvironment::new();
    conflict_environment.set_mutate_registry_after_launch(true);
    let conflict_service = ZedRemoteService::new(conflict_environment.clone());
    let conflict_workspace = conflict_service.load_workspace().unwrap();
    let conflict_project = conflict_workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let conflict_outcome = conflict_service
        .open_project(open_request(
            conflict_project,
            &conflict_workspace,
            true,
            ZedOpenStrategy::Default,
        ))
        .unwrap();
    assert_eq!(
        conflict_outcome.remember,
        ZedRememberOutcome::Failed(ZedRemoteErrorKind::RegistryConflict)
    );
    assert_eq!(conflict_environment.launch_calls().len(), 1);
}

#[test]
fn forget_requires_exact_id_and_revision() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment.clone());
    let workspace = service.load_workspace().unwrap();
    let project = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let mut mismatch = forget_request(project, &workspace);
    mismatch.confirmed_project_id.push_str("-mismatch");
    assert_eq!(
        service.forget_project(mismatch).unwrap_err().kind(),
        ZedRemoteErrorKind::InvalidProject
    );

    let forgotten = service
        .forget_project(forget_request(project, &workspace))
        .unwrap();
    assert!(!forgotten.projects.iter().any(|item| item.id == project.id));

    let stale_environment = FakeZedEnvironment::new();
    let stale_service = ZedRemoteService::new(stale_environment.clone());
    let stale_workspace = stale_service.load_workspace().unwrap();
    let stale_project = stale_workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let stale_store = stale_environment.zed_registry_store();
    let stale_current = stale_store.inspect().unwrap();
    stale_store
        .remember_if_revision(
            &stale_current.revision,
            recent_project("other", "/srv/other", 90),
        )
        .unwrap();
    let before = stale_environment.registry_bytes();
    assert_eq!(
        stale_service
            .forget_project(forget_request(stale_project, &stale_workspace))
            .unwrap_err()
            .kind(),
        ZedRemoteErrorKind::RegistryConflict
    );
    assert_eq!(stale_environment.registry_bytes(), before);
}

#[test]
fn system_preferences_preserve_unknown_settings_fields() {
    let temp = tempfile::tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        serde_json::to_vec(&json!({
            "futureField": {"keep": true},
            "zedRemoteOpenStrategy": "addToFocusedWorkspace",
            "zedRemoteProjectRegistryEnabled": true
        }))
        .unwrap(),
    )
    .unwrap();
    let environment =
        SystemProviderEnvironment::for_paths(settings_path.clone(), temp.path().join("codex"))
            .with_zed_remote_paths(
                temp.path().join("global-state.json"),
                temp.path().join("recent.json"),
                vec![temp.path().join("state.sqlite")],
            );
    let service = ZedRemoteService::new(environment);
    let before = service.load_workspace().unwrap();

    service
        .save_preferences(SaveZedPreferences {
            expected_revision: before.settings_revision,
            default_strategy: ZedOpenStrategy::NewWindow,
            registry_enabled: false,
        })
        .unwrap();

    let raw: Value = serde_json::from_slice(&std::fs::read(settings_path).unwrap()).unwrap();
    assert_eq!(raw["futureField"], json!({"keep": true}));
    assert_eq!(raw["zedRemoteOpenStrategy"], "newWindow");
    assert_eq!(raw["zedRemoteProjectRegistryEnabled"], false);
}

#[test]
fn system_recording_launcher_writes_only_safe_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    std::fs::write(&settings_path, b"{}").unwrap();
    let registry_path = temp.path().join("recent.json");
    let project = recent_project("record-sentinel", "/srv/record-sentinel", 10);
    std::fs::write(
        &registry_path,
        serde_json::to_vec(&json!({ "projects": [project] })).unwrap(),
    )
    .unwrap();
    let launch_record_path = temp.path().join("launch-record.json");
    let environment =
        SystemProviderEnvironment::for_paths(settings_path, temp.path().join("codex"))
            .with_zed_remote_paths(
                temp.path().join("global-state.json"),
                registry_path,
                vec![temp.path().join("state.sqlite")],
            )
            .with_zed_launch_record_path(&launch_record_path);
    let service = ZedRemoteService::new(environment);
    let workspace = service.load_workspace().unwrap();
    let selected = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();

    service
        .open_project(open_request(
            selected,
            &workspace,
            false,
            ZedOpenStrategy::NewWindow,
        ))
        .unwrap();

    let raw = std::fs::read_to_string(launch_record_path).unwrap();
    assert!(raw.contains("newWindow"));
    assert!(raw.contains("argumentCount"));
    assert!(!raw.contains("ssh://"));
    assert!(!raw.contains("record-sentinel"));
}

#[test]
fn workspace_debug_omits_project_metadata() {
    let workspace = ZedRemoteService::new(FakeZedEnvironment::new())
        .load_workspace()
        .unwrap();
    let debug = format!("{workspace:?}");

    for sentinel in [
        "host-sentinel",
        "remote-sentinel",
        "/srv/",
        "ssh://",
        "alice@",
    ] {
        assert!(!debug.contains(sentinel), "leaked sentinel: {sentinel}");
    }
}

#[test]
fn open_outcome_and_errors_debug_without_operational_metadata() {
    let environment = FakeZedEnvironment::new();
    let service = ZedRemoteService::new(environment);
    let workspace = service.load_workspace().unwrap();
    let project = workspace
        .projects
        .iter()
        .find(|project| project.source == ZedRemoteProjectSource::Recent)
        .unwrap();
    let outcome = service
        .open_project(open_request(
            project,
            &workspace,
            false,
            ZedOpenStrategy::NewWindow,
        ))
        .unwrap();
    let debug = format!("{outcome:?}");
    assert!(!debug.contains("ssh://"));
    assert!(!debug.contains("host-sentinel"));
    assert!(!debug.contains("/srv/"));

    let error = ZedRemoteErrorKind::InvalidProject;
    assert!(!format!("{error:?}").contains("sentinel"));
}

fn recent_project(suffix: &str, path: &str, last_opened_at_ms: i64) -> ZedRemoteProject {
    ZedRemoteProject {
        id: format!("zed-remote-project:{suffix}"),
        label: format!("project-{suffix}"),
        host_id: "host-main".to_string(),
        ssh: SshTarget {
            user: "alice".to_string(),
            host: "host-sentinel.example.test".to_string(),
            port: Some(2222),
        },
        path: path.to_string(),
        url: format!("ssh://alice@host-sentinel.example.test:2222{path}"),
        source: ZedRemoteProjectSource::Recent,
        last_opened_at_ms: Some(last_opened_at_ms),
        is_current: false,
    }
}

fn open_request(
    project: &ZedRemoteProjectSummary,
    workspace: &ZedRemoteWorkspace,
    remember: bool,
    strategy: ZedOpenStrategy,
) -> OpenZedRemoteProject {
    OpenZedRemoteProject {
        project_id: project.id.clone(),
        confirmed_project_id: project.id.clone(),
        expected_project_revision: project.revision.clone(),
        expected_registry_revision: workspace.registry_revision.clone(),
        strategy,
        confirmed_strategy: strategy,
        remember,
        confirmed_remember: remember,
    }
}

fn forget_request(
    project: &ZedRemoteProjectSummary,
    workspace: &ZedRemoteWorkspace,
) -> ForgetZedRemoteProject {
    ForgetZedRemoteProject {
        expected_registry_revision: workspace.registry_revision.clone(),
        project_id: project.id.clone(),
        confirmed_project_id: project.id.clone(),
    }
}
