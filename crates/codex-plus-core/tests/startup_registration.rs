use std::path::PathBuf;

use codex_plus_core::startup_registration::{
    CANONICAL_RUN_NAME, LEGACY_RUN_NAME, OwnedStringValueSnapshot, StartAtSignInHealth,
    StartupRegistrationOperation, StartupRegistrationSnapshot, build_migrate_start_at_sign_in_plan,
    build_package_uninstall_plan, build_package_upgrade_plan, build_set_start_at_sign_in_plan,
    canonical_startup_command, inspect_start_at_sign_in,
};

fn launcher_path() -> PathBuf {
    PathBuf::from(r"C:\Program Files\CodexPlusPlus\codex-plus-plus.exe")
}

fn old_run_value(port: u16) -> String {
    format!("\"{}\" --debug-port {port}", launcher_path().display())
}

fn old_shortcut(arguments: &str) -> codex_plus_core::desktop_integration::ShortcutSnapshot {
    codex_plus_core::desktop_integration::ShortcutSnapshot {
        target: launcher_path(),
        arguments: arguments.to_string(),
    }
}

fn disabled_snapshot() -> StartupRegistrationSnapshot {
    StartupRegistrationSnapshot {
        launcher_path: launcher_path(),
        launcher_is_file: true,
        canonical_run: OwnedStringValueSnapshot::Absent,
        legacy_run: OwnedStringValueSnapshot::Absent,
        legacy_shortcut: None,
    }
}

#[test]
fn startup_inspection_truth_table_separates_effective_behavior_from_health() {
    let exact = canonical_startup_command(&launcher_path());
    let cases = [
        (
            OwnedStringValueSnapshot::String(exact.clone()),
            OwnedStringValueSnapshot::Absent,
            None,
            StartAtSignInHealth::Enabled,
            true,
        ),
        (
            OwnedStringValueSnapshot::Absent,
            OwnedStringValueSnapshot::Absent,
            None,
            StartAtSignInHealth::Disabled,
            false,
        ),
        (
            OwnedStringValueSnapshot::Absent,
            OwnedStringValueSnapshot::String(old_run_value(9229)),
            None,
            StartAtSignInHealth::NeedsMigration,
            true,
        ),
        (
            OwnedStringValueSnapshot::Absent,
            OwnedStringValueSnapshot::Absent,
            Some(old_shortcut("--debug-port 9229")),
            StartAtSignInHealth::NeedsMigration,
            true,
        ),
        (
            OwnedStringValueSnapshot::Absent,
            OwnedStringValueSnapshot::String(old_run_value(9229)),
            Some(old_shortcut("--debug-port 9229")),
            StartAtSignInHealth::NeedsMigration,
            true,
        ),
        (
            OwnedStringValueSnapshot::String("unexpected".to_string()),
            OwnedStringValueSnapshot::Absent,
            None,
            StartAtSignInHealth::NeedsRepair,
            false,
        ),
        (
            OwnedStringValueSnapshot::String(exact),
            OwnedStringValueSnapshot::String(old_run_value(9229)),
            None,
            StartAtSignInHealth::NeedsRepair,
            true,
        ),
    ];

    for (canonical_run, legacy_run, legacy_shortcut, health, effective_enabled) in cases {
        let mut snapshot = disabled_snapshot();
        snapshot.canonical_run = canonical_run;
        snapshot.legacy_run = legacy_run;
        snapshot.legacy_shortcut = legacy_shortcut;

        let inspection = inspect_start_at_sign_in(&snapshot);
        assert_eq!(inspection.health, health);
        assert_eq!(inspection.effective_enabled, effective_enabled);
    }
}

#[test]
fn startup_inspection_rejects_invalid_owned_values_and_unstable_layouts() {
    let invalid_legacy = [
        OwnedStringValueSnapshot::UnsupportedType,
        OwnedStringValueSnapshot::String("cmd.exe /c launcher.exe".to_string()),
        OwnedStringValueSnapshot::String(format!(
            "\"{}\" --helper-port 57321",
            launcher_path().display()
        )),
    ];
    for legacy_run in invalid_legacy {
        let mut snapshot = disabled_snapshot();
        snapshot.legacy_run = legacy_run;
        let inspection = inspect_start_at_sign_in(&snapshot);
        assert_eq!(inspection.health, StartAtSignInHealth::NeedsRepair);
        assert!(!inspection.effective_enabled);
    }

    let mut development = disabled_snapshot();
    development.launcher_path = PathBuf::from(r"C:\dev\codex-plus-launcher.exe");
    assert_eq!(
        inspect_start_at_sign_in(&development).health,
        StartAtSignInHealth::Unavailable
    );

    let mut missing = disabled_snapshot();
    missing.launcher_is_file = false;
    assert_eq!(
        inspect_start_at_sign_in(&missing).health,
        StartAtSignInHealth::Unavailable
    );
}

#[test]
fn canonical_startup_value_is_only_the_quoted_stable_launcher_path() {
    let value = canonical_startup_command(&launcher_path());

    assert_eq!(value, format!("\"{}\"", launcher_path().display()));
    assert!(!value.contains("--debug-port"));
    assert!(!value.contains("--helper-port"));
    assert!(!value.to_ascii_lowercase().contains("cmd"));
}

#[test]
fn enable_reconciles_canonical_then_removes_known_legacy_entries() {
    let mut snapshot = disabled_snapshot();
    snapshot.canonical_run = OwnedStringValueSnapshot::String("unexpected".to_string());
    snapshot.legacy_run = OwnedStringValueSnapshot::String(old_run_value(9333));
    snapshot.legacy_shortcut = Some(old_shortcut("--debug-port 9333"));

    let plan = build_set_start_at_sign_in_plan(&snapshot, true);

    assert_eq!(
        plan.operations,
        vec![
            StartupRegistrationOperation::SetRunValue {
                name: CANONICAL_RUN_NAME,
                value: canonical_startup_command(&launcher_path()),
            },
            StartupRegistrationOperation::DeleteRunValue {
                name: LEGACY_RUN_NAME,
            },
            StartupRegistrationOperation::DeleteLegacyStartupShortcut,
        ]
    );
}

#[test]
fn migrate_is_explicit_preserves_enabled_intent_and_is_idempotent() {
    let mut legacy = disabled_snapshot();
    legacy.legacy_shortcut = Some(old_shortcut("--debug-port 9229"));

    let plan = build_migrate_start_at_sign_in_plan(&legacy).expect("migrate plan");
    assert_eq!(
        plan.operations,
        vec![
            StartupRegistrationOperation::SetRunValue {
                name: CANONICAL_RUN_NAME,
                value: canonical_startup_command(&launcher_path()),
            },
            StartupRegistrationOperation::DeleteLegacyStartupShortcut,
        ]
    );

    let mut enabled = disabled_snapshot();
    enabled.canonical_run =
        OwnedStringValueSnapshot::String(canonical_startup_command(&launcher_path()));
    assert!(build_migrate_start_at_sign_in_plan(&enabled).is_err());
    assert!(
        build_set_start_at_sign_in_plan(&enabled, true)
            .operations
            .is_empty()
    );
}

#[test]
fn disable_and_package_uninstall_remove_only_owned_registrations() {
    let mut snapshot = disabled_snapshot();
    snapshot.canonical_run =
        OwnedStringValueSnapshot::String(canonical_startup_command(&launcher_path()));
    snapshot.legacy_run = OwnedStringValueSnapshot::String(old_run_value(9229));
    snapshot.legacy_shortcut = Some(old_shortcut("--debug-port 9229"));

    let expected = vec![
        StartupRegistrationOperation::DeleteRunValue {
            name: CANONICAL_RUN_NAME,
        },
        StartupRegistrationOperation::DeleteRunValue {
            name: LEGACY_RUN_NAME,
        },
        StartupRegistrationOperation::DeleteLegacyStartupShortcut,
    ];
    assert_eq!(
        build_set_start_at_sign_in_plan(&snapshot, false).operations,
        expected
    );
    assert_eq!(build_package_uninstall_plan(&snapshot).operations, expected);
    assert!(
        build_package_uninstall_plan(&disabled_snapshot())
            .operations
            .is_empty()
    );
}

#[test]
fn package_upgrade_preserves_only_valid_enabled_intent() {
    let mut valid_legacy = disabled_snapshot();
    valid_legacy.legacy_run = OwnedStringValueSnapshot::String(old_run_value(9229));
    let migrated = build_package_upgrade_plan(&valid_legacy);
    assert!(matches!(
        migrated.operations.first(),
        Some(StartupRegistrationOperation::SetRunValue { name, .. }) if *name == CANONICAL_RUN_NAME
    ));

    let mut invalid_owned = disabled_snapshot();
    invalid_owned.legacy_run = OwnedStringValueSnapshot::String("unexpected".to_string());
    assert_eq!(
        build_package_upgrade_plan(&invalid_owned).operations,
        vec![StartupRegistrationOperation::DeleteRunValue {
            name: LEGACY_RUN_NAME,
        }]
    );
}

#[test]
fn inspection_and_plans_leave_watcher_disabled_unowned() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join("watcher.disabled");
    std::fs::write(&sentinel, b"preserve").unwrap();

    let snapshot = disabled_snapshot();
    let _ = inspect_start_at_sign_in(&snapshot);
    let _ = build_set_start_at_sign_in_plan(&snapshot, true);
    let _ = build_package_upgrade_plan(&snapshot);
    let _ = build_package_uninstall_plan(&snapshot);

    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserve");
    let debug = format!("{snapshot:?}");
    assert!(!debug.contains("Program Files"));
    assert!(!debug.contains("watcher.disabled"));
}
