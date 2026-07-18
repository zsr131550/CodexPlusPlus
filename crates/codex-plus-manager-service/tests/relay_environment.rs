use codex_plus_manager_service::{
    RelayEnvironmentService, RelayEnvironmentSource, RemoveEnvironmentConflicts,
    SystemProviderEnvironment,
};
use std::sync::{Mutex, OnceLock};

const NAME: &str = "OPENAI_CODEX_PLUS_NATIVE_TEST_SENTINEL";
const SECRET: &str = "environment-secret-sentinel";

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn environment_workspace_contains_metadata_without_values() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let settings = temp.path().join("settings.json");
    let codex_home = temp.path().join("codex");
    std::fs::create_dir(&codex_home).unwrap();
    std::fs::write(codex_home.join(".env"), format!("{NAME}={SECRET}\n")).unwrap();
    unsafe { std::env::set_var(NAME, SECRET) };
    let service = RelayEnvironmentService::new(SystemProviderEnvironment::for_manager_paths(
        settings,
        codex_home,
        temp.path().join("cc-switch.db"),
        temp.path().join("pending.json"),
        temp.path().join("backups"),
        true,
    ));

    let workspace = service.inspect().unwrap();
    let rendered = format!("{workspace:?}");

    unsafe { std::env::remove_var(NAME) };
    assert!(workspace.report.codex_env_file.exists);
    assert!(workspace.conflicts.iter().any(|item| item.name == NAME));
    assert!(!rendered.contains(SECRET));
}

#[test]
fn cleanup_removes_only_the_selected_process_name_and_reports_backup() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let service = RelayEnvironmentService::new(SystemProviderEnvironment::for_manager_paths(
        temp.path().join("settings.json"),
        temp.path().join("codex"),
        temp.path().join("cc-switch.db"),
        temp.path().join("pending.json"),
        temp.path().join("backups"),
        true,
    ));
    unsafe { std::env::set_var(NAME, SECRET) };
    let workspace = service.inspect().unwrap();

    let outcome = service
        .remove_conflicts(RemoveEnvironmentConflicts {
            expected_revision: workspace.revision,
            names: vec![NAME.to_string(), "CODEX_HOME".to_string()],
        })
        .unwrap();

    unsafe { std::env::remove_var(NAME) };
    assert!(std::env::var_os(NAME).is_none());
    assert_eq!(outcome.removed.len(), 1);
    assert!(outcome.backup_path.is_some());
    assert!(outcome.remaining.iter().all(|item| item.name != NAME));
    assert!(!format!("{outcome:?}").contains(SECRET));
}
