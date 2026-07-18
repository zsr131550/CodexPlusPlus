use std::path::{Path, PathBuf};

use codex_plus_core::provider_import::{
    ProviderImportRequest, load_pending_provider_import_at, save_pending_provider_import_at,
};
use codex_plus_core::settings::{BackendSettings, SettingsStore};
use codex_plus_manager_service::{
    ConfirmPendingImport, ImportCcsProviders, ProviderImportErrorKind, ProviderImportService,
    ProviderImportSource, SystemProviderEnvironment,
};
use rusqlite::{Connection, params};

const SECRET: &str = "provider-import-secret-sentinel";

struct Fixture {
    _temp: tempfile::TempDir,
    settings_path: PathBuf,
    codex_home: PathBuf,
    ccs_db: PathBuf,
    pending_path: PathBuf,
    backup_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let codex_home = temp.path().join("codex");
        let ccs_db = temp.path().join("cc-switch.db");
        let pending_path = temp.path().join("pending-provider-import.json");
        let backup_dir = temp.path().join("backups");
        std::fs::create_dir(&codex_home).unwrap();
        SettingsStore::new(settings_path.clone())
            .save(&BackendSettings::default())
            .unwrap();
        create_ccs_db(&ccs_db);
        Self {
            _temp: temp,
            settings_path,
            codex_home,
            ccs_db,
            pending_path,
            backup_dir,
        }
    }

    fn service(&self) -> ProviderImportService<SystemProviderEnvironment> {
        ProviderImportService::new(SystemProviderEnvironment::for_manager_paths(
            self.settings_path.clone(),
            self.codex_home.clone(),
            self.ccs_db.clone(),
            self.pending_path.clone(),
            self.backup_dir.clone(),
            true,
        ))
    }
}

fn create_ccs_db(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                created_at INTEGER,
                sort_index INTEGER,
                PRIMARY KEY (id, app_type)
            );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO providers
             (id, app_type, name, settings_config, created_at, sort_index)
             VALUES (?1, 'codex', ?2, ?3, 1, 1)",
            params![
                "fixture",
                "Fixture Relay",
                serde_json::json!({
                    "base_url": "https://fixture.example/v1",
                    "apiKey": SECRET,
                    "api_format": "responses"
                })
                .to_string()
            ],
        )
        .unwrap();
}

#[test]
fn discovers_safe_ccs_summaries_and_imports_a_fresh_workspace() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let discovery = service.discover_ccs().unwrap();
    let rendered = format!("{discovery:?}");
    assert_eq!(discovery.importable_count, 1);
    assert_eq!(discovery.duplicate_count, 0);
    assert_eq!(discovery.providers[0].name, "Fixture Relay");
    assert!(!rendered.contains(SECRET));

    let outcome = service
        .import_ccs(ImportCcsProviders {
            source_revision: discovery.source_revision,
            provider_revision: discovery.provider_revision,
        })
        .unwrap();

    assert_eq!(outcome.imported, 1);
    assert_eq!(outcome.workspace.document.profiles.len(), 2);
    assert!(!format!("{outcome:?}").contains(SECRET));
}

#[test]
fn stale_provider_revision_rejects_ccs_import_without_writing() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let discovery = service.discover_ccs().unwrap();
    let mut settings = SettingsStore::new(fixture.settings_path.clone())
        .load()
        .unwrap();
    settings.relay_test_model = "changed".to_string();
    SettingsStore::new(fixture.settings_path.clone())
        .save(&settings)
        .unwrap();

    let error = service
        .import_ccs(ImportCcsProviders {
            source_revision: discovery.source_revision,
            provider_revision: discovery.provider_revision,
        })
        .unwrap_err();

    assert_eq!(error.kind(), ProviderImportErrorKind::ProviderConflict);
    assert_eq!(
        SettingsStore::new(fixture.settings_path)
            .load()
            .unwrap()
            .relay_profiles
            .len(),
        1
    );
}

#[test]
fn pending_conflict_keeps_the_reviewed_request() {
    let fixture = Fixture::new();
    let service = fixture.service();
    save_pending_provider_import_at(
        &fixture.pending_path,
        &ProviderImportRequest {
            name: "Pending".to_string(),
            base_url: "https://pending.example/v1".to_string(),
            api_key: SECRET.to_string(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: String::new(),
            auth_contents: String::new(),
        },
    )
    .unwrap();
    let pending = service.load_pending().unwrap().pending.unwrap();
    let provider_revision = service.discover_ccs().unwrap().provider_revision;

    let error = service
        .confirm_pending(ConfirmPendingImport {
            pending_revision: "0".repeat(64),
            provider_revision,
        })
        .unwrap_err();

    assert_eq!(error.kind(), ProviderImportErrorKind::PendingConflict);
    assert_eq!(
        load_pending_provider_import_at(&fixture.pending_path)
            .unwrap()
            .unwrap()
            .name,
        "Pending"
    );
    assert!(!format!("{pending:?}").contains(SECRET));
}
