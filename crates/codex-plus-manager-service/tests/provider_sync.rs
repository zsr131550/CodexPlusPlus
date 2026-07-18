use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use codex_plus_core::settings::BackendSettings;
use codex_plus_data::{
    ProviderSyncResult, ProviderSyncStatus, ProviderSyncTargetList, ProviderSyncTargetOption,
    ProviderSyncTargetSource,
};
use codex_plus_manager_service::{
    ProviderSyncEnvironment, ProviderSyncError, ProviderSyncErrorKind, ProviderSyncRevision,
    ProviderSyncService, ProviderSyncSource, SetProviderAutoRepair,
};

#[derive(Clone)]
struct FakeProviderSyncEnvironment {
    settings: Arc<Mutex<BackendSettings>>,
    targets: ProviderSyncTargetList,
    reject_preference_save: Arc<AtomicBool>,
    preference_save_calls: Arc<AtomicUsize>,
}

impl FakeProviderSyncEnvironment {
    fn new(settings: BackendSettings) -> Self {
        Self {
            settings: Arc::new(Mutex::new(settings)),
            targets: ProviderSyncTargetList {
                current_provider: "openai".to_owned(),
                targets: vec![ProviderSyncTargetOption {
                    id: "from-config".to_owned(),
                    sources: vec![ProviderSyncTargetSource::Config],
                    is_current_provider: false,
                    is_manual: false,
                    is_saved: false,
                }],
            },
            reject_preference_save: Arc::new(AtomicBool::new(false)),
            preference_save_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn mutate_settings(&self, mutate: impl FnOnce(&mut BackendSettings)) {
        mutate(&mut self.settings.lock().unwrap());
    }

    fn preference_save_calls(&self) -> usize {
        self.preference_save_calls.load(Ordering::SeqCst)
    }
}

impl ProviderSyncEnvironment for FakeProviderSyncEnvironment {
    fn load_provider_sync_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.settings.lock().unwrap().clone())
    }

    fn load_provider_sync_targets(&self) -> ProviderSyncTargetList {
        self.targets.clone()
    }

    fn run_provider_sync(&self, target: &str) -> ProviderSyncResult {
        ProviderSyncResult {
            status: ProviderSyncStatus::Synced,
            message: "synced".to_owned(),
            target_provider: target.to_owned(),
            backup_dir: None,
            changed_session_files: 0,
            skipped_locked_rollout_files: Vec::new(),
            sqlite_rows_updated: 0,
            sqlite_provider_rows_updated: 0,
            sqlite_user_event_rows_updated: 0,
            sqlite_cwd_rows_updated: 0,
            updated_workspace_roots: 0,
            encrypted_content_warning: None,
        }
    }

    fn save_provider_sync_enabled(
        &self,
        _expected: &ProviderSyncRevision,
        enabled: bool,
    ) -> Result<(), ProviderSyncError> {
        self.preference_save_calls.fetch_add(1, Ordering::SeqCst);
        if self.reject_preference_save.load(Ordering::SeqCst) {
            return Err(ProviderSyncError::new(
                ProviderSyncErrorKind::SettingsConflict,
            ));
        }
        self.settings.lock().unwrap().provider_sync_enabled = enabled;
        Ok(())
    }

    fn save_provider_sync_target(&self, target: &str) -> Result<(), ProviderSyncError> {
        self.settings
            .lock()
            .unwrap()
            .provider_sync_last_selected_provider = target.to_owned();
        Ok(())
    }
}

fn settings() -> BackendSettings {
    BackendSettings {
        provider_sync_enabled: true,
        provider_sync_manual_providers: vec!["manual".to_owned()],
        provider_sync_saved_providers: vec!["saved".to_owned()],
        provider_sync_last_selected_provider: "saved".to_owned(),
        ..BackendSettings::default()
    }
}

#[test]
fn provider_sync_workspace_preserves_manual_targets_and_settings_revision() {
    let environment = FakeProviderSyncEnvironment::new(settings());
    let service = ProviderSyncService::new(environment.clone());

    let workspace = service.load_provider_sync_workspace().unwrap();

    assert!(workspace.auto_repair);
    assert_eq!(workspace.selected_target, "saved");
    let manual = workspace
        .targets
        .targets
        .iter()
        .find(|target| target.id == "manual")
        .unwrap();
    assert!(manual.is_manual);
    assert!(manual.sources.contains(&ProviderSyncTargetSource::Manual));
    let saved = workspace
        .targets
        .targets
        .iter()
        .find(|target| target.id == "saved")
        .unwrap();
    assert!(saved.is_saved);
    let revision = workspace.revision;
    environment.mutate_settings(|settings| settings.codex_app_path = "unrelated".to_owned());

    assert_eq!(
        service.load_provider_sync_workspace().unwrap().revision,
        revision
    );
}

#[test]
fn provider_sync_preference_rejects_a_changed_settings_revision() {
    let environment = FakeProviderSyncEnvironment::new(settings());
    let service = ProviderSyncService::new(environment.clone());
    let workspace = service.load_provider_sync_workspace().unwrap();
    environment.mutate_settings(|settings| settings.provider_sync_enabled = false);

    let error = service
        .set_provider_auto_repair(SetProviderAutoRepair {
            expected_revision: workspace.revision,
            enabled: false,
        })
        .unwrap_err();

    assert_eq!(error.kind(), ProviderSyncErrorKind::SettingsConflict);
    assert_eq!(environment.preference_save_calls(), 0);
}
