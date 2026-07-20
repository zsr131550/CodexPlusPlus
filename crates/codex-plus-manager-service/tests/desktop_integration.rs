use std::sync::{Arc, Mutex};

use codex_plus_core::desktop_integration::{
    DesktopIntegrationHealth, DesktopIntegrationItemKind, DesktopRepairOperation, ShortcutSnapshot,
    WindowsDesktopSnapshot,
};
use codex_plus_core::startup_registration::{
    CANONICAL_RUN_NAME, LEGACY_RUN_NAME, OwnedStringValueSnapshot, StartAtSignInHealth,
    StartupRegistrationOperation, StartupRegistrationSnapshot, canonical_startup_command,
};
use codex_plus_manager_service::{
    DesktopIntegrationEnvironment, DesktopIntegrationEnvironmentError,
    DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationErrorKind, DesktopIntegrationPlatform,
    DesktopIntegrationService, DesktopIntegrationSource, MigrateStartAtSignIn,
    RepairDesktopIntegration, SetStartAtSignIn,
};

fn manager_path() -> std::path::PathBuf {
    std::path::PathBuf::from(r"C:\Program Files\CodexPlusPlus\codex-plus-plus-manager.exe")
}

fn launcher_path() -> std::path::PathBuf {
    std::path::PathBuf::from(r"C:\Program Files\CodexPlusPlus\codex-plus-plus.exe")
}

fn legacy_run_value() -> String {
    format!("\"{}\" --debug-port 9229", launcher_path().display())
}

fn current_shortcut(target: std::path::PathBuf) -> ShortcutSnapshot {
    ShortcutSnapshot {
        target,
        arguments: String::new(),
    }
}

fn windows_environment(
    needs_repair: bool,
    legacy_enabled: bool,
) -> DesktopIntegrationEnvironmentSnapshot {
    let repair = WindowsDesktopSnapshot {
        current_exe: manager_path(),
        launcher_is_file: true,
        desktop_dir: Some(std::path::PathBuf::from(r"C:\Users\fixture\Desktop")),
        programs_dir: Some(std::path::PathBuf::from(
            r"C:\Users\fixture\AppData\Roaming\Microsoft\Windows\Start Menu\Programs",
        )),
        desktop_manager: (!needs_repair).then(|| current_shortcut(manager_path())),
        start_menu_launcher: (!needs_repair).then(|| current_shortcut(launcher_path())),
        start_menu_manager: (!needs_repair).then(|| current_shortcut(manager_path())),
        protocol_command: (!needs_repair)
            .then(|| format!("\"{}\" \"%1\"", manager_path().display())),
    };
    let sign_in = StartupRegistrationSnapshot {
        launcher_path: launcher_path(),
        launcher_is_file: true,
        canonical_run: OwnedStringValueSnapshot::Absent,
        legacy_run: if legacy_enabled {
            OwnedStringValueSnapshot::String(legacy_run_value())
        } else {
            OwnedStringValueSnapshot::Absent
        },
        legacy_shortcut: None,
    };
    DesktopIntegrationEnvironmentSnapshot::Windows {
        repair: Box::new(repair),
        sign_in,
    }
}

#[derive(Clone)]
struct RecordingEnvironment {
    state: Arc<Mutex<DesktopIntegrationEnvironmentSnapshot>>,
    effects: Arc<Mutex<Vec<String>>>,
}

impl RecordingEnvironment {
    fn new(state: DesktopIntegrationEnvironmentSnapshot) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            effects: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn effects(&self) -> Vec<String> {
        self.effects.lock().unwrap().clone()
    }

    fn replace(&self, state: DesktopIntegrationEnvironmentSnapshot) {
        *self.state.lock().unwrap() = state;
    }
}

impl DesktopIntegrationEnvironment for RecordingEnvironment {
    fn inspect_desktop_integration(
        &self,
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError> {
        Ok(self.state.lock().unwrap().clone())
    }

    fn apply_desktop_repair_operation(
        &self,
        operation: &DesktopRepairOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        self.effects
            .lock()
            .unwrap()
            .push(operation.item_kind().as_str().to_string());
        let mut state = self.state.lock().unwrap();
        match (&mut *state, operation) {
            (
                DesktopIntegrationEnvironmentSnapshot::Windows { repair, .. },
                DesktopRepairOperation::WriteShortcut {
                    kind,
                    target,
                    arguments,
                    ..
                },
            ) => {
                let value = Some(ShortcutSnapshot {
                    target: target.clone(),
                    arguments: arguments.clone(),
                });
                match kind {
                    DesktopIntegrationItemKind::DesktopManagerShortcut => {
                        repair.desktop_manager = value
                    }
                    DesktopIntegrationItemKind::StartMenuLauncherShortcut => {
                        repair.start_menu_launcher = value
                    }
                    DesktopIntegrationItemKind::StartMenuManagerShortcut => {
                        repair.start_menu_manager = value
                    }
                    _ => return Err(DesktopIntegrationEnvironmentError::EffectFailed),
                }
            }
            (
                DesktopIntegrationEnvironmentSnapshot::Windows { repair, .. },
                DesktopRepairOperation::WriteProtocol { command, .. },
            ) => repair.protocol_command = Some(command.clone()),
            (
                DesktopIntegrationEnvironmentSnapshot::Macos { repair },
                DesktopRepairOperation::RegisterMacosBundle { .. },
            ) => repair.registered = true,
            _ => return Err(DesktopIntegrationEnvironmentError::EffectFailed),
        }
        Ok(())
    }

    fn apply_startup_registration_operation(
        &self,
        operation: &StartupRegistrationOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        self.effects.lock().unwrap().push(match operation {
            StartupRegistrationOperation::SetRunValue { name, .. } => format!("set:{name}"),
            StartupRegistrationOperation::DeleteRunValue { name } => format!("delete:{name}"),
            StartupRegistrationOperation::DeleteLegacyStartupShortcut => {
                "delete:legacy_shortcut".to_string()
            }
        });
        let mut state = self.state.lock().unwrap();
        let DesktopIntegrationEnvironmentSnapshot::Windows { sign_in, .. } = &mut *state else {
            return Err(DesktopIntegrationEnvironmentError::EffectFailed);
        };
        match operation {
            StartupRegistrationOperation::SetRunValue { name, value }
                if *name == CANONICAL_RUN_NAME =>
            {
                sign_in.canonical_run = OwnedStringValueSnapshot::String(value.clone());
            }
            StartupRegistrationOperation::DeleteRunValue { name }
                if *name == CANONICAL_RUN_NAME =>
            {
                sign_in.canonical_run = OwnedStringValueSnapshot::Absent;
            }
            StartupRegistrationOperation::DeleteRunValue { name } if *name == LEGACY_RUN_NAME => {
                sign_in.legacy_run = OwnedStringValueSnapshot::Absent;
            }
            StartupRegistrationOperation::DeleteLegacyStartupShortcut => {
                sign_in.legacy_shortcut = None;
            }
            _ => return Err(DesktopIntegrationEnvironmentError::EffectFailed),
        }
        Ok(())
    }
}

#[test]
fn inspection_composes_safe_workspace_with_opaque_revision_and_zero_effects() {
    let environment = RecordingEnvironment::new(windows_environment(true, true));
    let service = DesktopIntegrationService::new(environment.clone());

    let workspace = service.inspect().unwrap();

    assert_eq!(workspace.platform, DesktopIntegrationPlatform::Windows);
    assert_eq!(
        workspace.repair_health,
        DesktopIntegrationHealth::NeedsRepair
    );
    assert_eq!(workspace.repair_items.len(), 4);
    let sign_in = workspace.sign_in.expect("Windows sign-in status");
    assert_eq!(sign_in.health, StartAtSignInHealth::NeedsMigration);
    assert!(sign_in.effective_enabled);
    assert!(environment.effects().is_empty());
    let debug = format!("{workspace:?}");
    assert!(!debug.contains("Program Files"));
    assert!(!debug.contains(&legacy_run_value()));
    assert!(format!("{:?}", workspace.revision).contains("opaque"));
}

#[test]
fn repair_confirmation_conflict_and_replay_are_rejected_before_effects() {
    let environment = RecordingEnvironment::new(windows_environment(true, false));
    let service = DesktopIntegrationService::new(environment.clone());

    let unconfirmed = service.inspect().unwrap();
    let error = service
        .repair(RepairDesktopIntegration {
            expected_revision: unconfirmed.revision,
            confirmed: false,
        })
        .unwrap_err();
    assert_eq!(
        error.kind(),
        DesktopIntegrationErrorKind::ConfirmationRequired
    );
    assert!(environment.effects().is_empty());
    assert_eq!(
        service
            .repair(RepairDesktopIntegration {
                expected_revision: unconfirmed.revision,
                confirmed: true,
            })
            .unwrap_err()
            .kind(),
        DesktopIntegrationErrorKind::InvalidRevision
    );

    let stale = service.inspect().unwrap();
    environment.replace(windows_environment(true, true));
    let conflict = service
        .repair(RepairDesktopIntegration {
            expected_revision: stale.revision,
            confirmed: true,
        })
        .unwrap_err();
    assert_eq!(conflict.kind(), DesktopIntegrationErrorKind::Conflict);
    assert!(conflict.refreshed_workspace().is_some());
    assert!(environment.effects().is_empty());
    assert_eq!(
        service
            .repair(RepairDesktopIntegration {
                expected_revision: stale.revision,
                confirmed: true,
            })
            .unwrap_err()
            .kind(),
        DesktopIntegrationErrorKind::InvalidRevision
    );

    let current = service.inspect().unwrap();
    let mutation = service
        .repair(RepairDesktopIntegration {
            expected_revision: current.revision,
            confirmed: true,
        })
        .unwrap();
    assert_eq!(
        mutation.workspace.repair_health,
        DesktopIntegrationHealth::Current
    );
    assert_eq!(mutation.applied_operation_count, 4);
    assert_ne!(mutation.workspace.revision, current.revision);
}

#[test]
fn sign_in_migrate_disable_and_enable_are_truthful_and_ordered() {
    let environment = RecordingEnvironment::new(windows_environment(false, true));
    let source: Arc<dyn DesktopIntegrationSource> =
        Arc::new(DesktopIntegrationService::new(environment.clone()));

    let legacy = source.inspect().unwrap();
    assert!(legacy.sign_in.as_ref().unwrap().effective_enabled);
    assert_eq!(
        legacy.sign_in.as_ref().unwrap().health,
        StartAtSignInHealth::NeedsMigration
    );
    let migrated = source
        .migrate_sign_in(MigrateStartAtSignIn {
            expected_revision: legacy.revision,
        })
        .unwrap();
    assert_eq!(
        migrated.workspace.sign_in.as_ref().unwrap().health,
        StartAtSignInHealth::Enabled
    );
    assert_eq!(
        environment.effects(),
        vec![
            format!("set:{CANONICAL_RUN_NAME}"),
            format!("delete:{LEGACY_RUN_NAME}"),
        ]
    );

    let not_migratable = source
        .migrate_sign_in(MigrateStartAtSignIn {
            expected_revision: migrated.workspace.revision,
        })
        .unwrap_err();
    assert_eq!(
        not_migratable.kind(),
        DesktopIntegrationErrorKind::MigrationUnavailable
    );
    assert_eq!(
        source
            .set_start_at_sign_in(SetStartAtSignIn {
                expected_revision: migrated.workspace.revision,
                enabled: false,
            })
            .unwrap_err()
            .kind(),
        DesktopIntegrationErrorKind::InvalidRevision
    );

    let enabled = source.inspect().unwrap();
    let disabled = source
        .set_start_at_sign_in(SetStartAtSignIn {
            expected_revision: enabled.revision,
            enabled: false,
        })
        .unwrap();
    assert_eq!(
        disabled.workspace.sign_in.as_ref().unwrap().health,
        StartAtSignInHealth::Disabled
    );
    let reenabled = source
        .set_start_at_sign_in(SetStartAtSignIn {
            expected_revision: disabled.workspace.revision,
            enabled: true,
        })
        .unwrap();
    assert_eq!(
        reenabled.workspace.sign_in.as_ref().unwrap().health,
        StartAtSignInHealth::Enabled
    );
    assert_eq!(
        reenabled
            .workspace
            .sign_in
            .as_ref()
            .map(|status| status.effective_enabled),
        Some(true)
    );

    let exact = canonical_startup_command(&launcher_path());
    assert!(!exact.contains("--debug-port"));
    assert!(
        environment
            .effects()
            .iter()
            .all(|effect| !effect.contains("process") && !effect.contains("install"))
    );
}

#[test]
fn unsupported_platform_is_safe_and_has_no_mutation_surface() {
    let environment = RecordingEnvironment::new(DesktopIntegrationEnvironmentSnapshot::Unsupported);
    let service = DesktopIntegrationService::new(environment.clone());

    let workspace = service.inspect().unwrap();

    assert_eq!(workspace.platform, DesktopIntegrationPlatform::Unsupported);
    assert_eq!(
        workspace.repair_health,
        DesktopIntegrationHealth::Unavailable
    );
    assert!(workspace.sign_in.is_none());
    assert!(environment.effects().is_empty());
}
