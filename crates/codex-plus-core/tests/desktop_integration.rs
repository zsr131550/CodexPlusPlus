use std::path::{Path, PathBuf};

use codex_plus_core::desktop_integration::{
    DesktopIntegrationHealth, DesktopIntegrationItemKind, DesktopIntegrationItemState,
    DesktopRepairOperation, MAX_MACOS_INFO_PLIST_BYTES, MacosDesktopSnapshot, ShortcutSnapshot,
    WindowsDesktopSnapshot, assess_macos_desktop_integration, assess_windows_desktop_integration,
};
use codex_plus_core::install::{
    MANAGER_BINARY, MANAGER_BUNDLE_ID, MANAGER_NAME, SILENT_BINARY, SILENT_NAME,
};

fn exe_name(binary: &str) -> String {
    format!("{binary}.exe")
}

fn installed_root() -> PathBuf {
    PathBuf::from(r"C:\Program Files\CodexPlusPlus")
}

fn manager_path() -> PathBuf {
    installed_root().join(exe_name(MANAGER_BINARY))
}

fn launcher_path() -> PathBuf {
    installed_root().join(exe_name(SILENT_BINARY))
}

fn desktop_dir() -> PathBuf {
    PathBuf::from(r"C:\Users\fixture\Desktop")
}

fn programs_dir() -> PathBuf {
    PathBuf::from(r"C:\Users\fixture\AppData\Roaming\Microsoft\Windows\Start Menu\Programs")
}

fn shortcut(target: impl Into<PathBuf>) -> ShortcutSnapshot {
    ShortcutSnapshot {
        target: target.into(),
        arguments: String::new(),
    }
}

fn expected_protocol_command() -> String {
    format!("\"{}\" \"%1\"", manager_path().display())
}

fn current_windows_snapshot() -> WindowsDesktopSnapshot {
    WindowsDesktopSnapshot {
        current_exe: manager_path(),
        launcher_is_file: true,
        desktop_dir: Some(desktop_dir()),
        programs_dir: Some(programs_dir()),
        desktop_manager: Some(shortcut(manager_path())),
        start_menu_launcher: Some(shortcut(launcher_path())),
        start_menu_manager: Some(shortcut(manager_path())),
        protocol_command: Some(expected_protocol_command()),
    }
}

#[test]
fn windows_current_stable_layout_has_an_empty_idempotent_plan() {
    let assessment = assess_windows_desktop_integration(&current_windows_snapshot());

    assert_eq!(assessment.health, DesktopIntegrationHealth::Current);
    assert!(assessment.plan.is_none());
    assert!(
        assessment
            .items
            .iter()
            .all(|item| item.state == DesktopIntegrationItemState::Current)
    );
}

#[test]
fn windows_rejects_development_filename_and_missing_sibling_launcher() {
    let mut development = current_windows_snapshot();
    development.current_exe = installed_root().join("codex-plus-manager-native.exe");
    let development = assess_windows_desktop_integration(&development);
    assert_eq!(
        development.health,
        DesktopIntegrationHealth::ReinstallRequired
    );
    assert!(development.plan.is_none());

    let mut missing_launcher = current_windows_snapshot();
    missing_launcher.launcher_is_file = false;
    let missing_launcher = assess_windows_desktop_integration(&missing_launcher);
    assert_eq!(
        missing_launcher.health,
        DesktopIntegrationHealth::ReinstallRequired
    );
    assert!(missing_launcher.plan.is_none());
}

#[test]
fn windows_repair_plan_contains_only_missing_or_mismatched_allowlisted_items() {
    let mut snapshot = current_windows_snapshot();
    snapshot.desktop_manager = None;
    snapshot.start_menu_launcher = Some(shortcut(manager_path()));
    snapshot.start_menu_manager = None;
    snapshot.protocol_command = Some("unexpected".to_string());

    let assessment = assess_windows_desktop_integration(&snapshot);

    assert_eq!(assessment.health, DesktopIntegrationHealth::NeedsRepair);
    assert_eq!(
        assessment
            .items
            .iter()
            .filter(|item| item.state == DesktopIntegrationItemState::NeedsRepair)
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            DesktopIntegrationItemKind::DesktopManagerShortcut,
            DesktopIntegrationItemKind::StartMenuLauncherShortcut,
            DesktopIntegrationItemKind::StartMenuManagerShortcut,
            DesktopIntegrationItemKind::UrlProtocol,
        ]
    );

    let plan = assessment.plan.as_ref().expect("repair plan");
    let operations = &plan.operations;
    assert_eq!(operations.len(), 4);
    assert_eq!(
        operations
            .iter()
            .map(DesktopRepairOperation::item_kind)
            .collect::<Vec<_>>(),
        vec![
            DesktopIntegrationItemKind::DesktopManagerShortcut,
            DesktopIntegrationItemKind::StartMenuLauncherShortcut,
            DesktopIntegrationItemKind::StartMenuManagerShortcut,
            DesktopIntegrationItemKind::UrlProtocol,
        ]
    );

    match &operations[0] {
        DesktopRepairOperation::WriteShortcut {
            path,
            target,
            arguments,
            ..
        } => {
            assert_eq!(path, &desktop_dir().join(format!("{MANAGER_NAME}.lnk")));
            assert_eq!(target, &manager_path());
            assert!(arguments.is_empty());
        }
        operation => panic!(
            "unexpected operation kind: {}",
            operation.item_kind().as_str()
        ),
    }
    match &operations[1] {
        DesktopRepairOperation::WriteShortcut {
            path,
            target,
            arguments,
            ..
        } => {
            assert_eq!(
                path,
                &programs_dir()
                    .join(SILENT_NAME)
                    .join(format!("{SILENT_NAME}.lnk"))
            );
            assert_eq!(target, &launcher_path());
            assert!(arguments.is_empty());
        }
        operation => panic!(
            "unexpected operation kind: {}",
            operation.item_kind().as_str()
        ),
    }
    match &operations[2] {
        DesktopRepairOperation::WriteShortcut {
            path,
            target,
            arguments,
            ..
        } => {
            assert_eq!(
                path,
                &programs_dir()
                    .join(SILENT_NAME)
                    .join(format!("{MANAGER_NAME}.lnk"))
            );
            assert_eq!(target, &manager_path());
            assert!(arguments.is_empty());
        }
        operation => panic!(
            "unexpected operation kind: {}",
            operation.item_kind().as_str()
        ),
    }
    match &operations[3] {
        DesktopRepairOperation::WriteProtocol { subkey, command } => {
            assert_eq!(
                *subkey,
                r"Software\Classes\codexplusplus\shell\open\command"
            );
            assert_eq!(command, &expected_protocol_command());
        }
        operation => panic!(
            "unexpected operation kind: {}",
            operation.item_kind().as_str()
        ),
    }

    let debug = format!("{plan:?}");
    assert!(!debug.contains(installed_root().to_string_lossy().as_ref()));
    for forbidden in ["uninstall", "remove_data", "process", "launcher_desktop"] {
        assert!(!debug.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn windows_missing_known_folders_is_unavailable_without_a_plan() {
    for clear in ["desktop", "programs"] {
        let mut snapshot = current_windows_snapshot();
        match clear {
            "desktop" => snapshot.desktop_dir = None,
            "programs" => snapshot.programs_dir = None,
            _ => unreachable!(),
        }

        let assessment = assess_windows_desktop_integration(&snapshot);
        assert_eq!(assessment.health, DesktopIntegrationHealth::Unavailable);
        assert!(assessment.plan.is_none());
    }
}

#[test]
fn windows_shortcut_target_comparison_is_exact() {
    let mut snapshot = current_windows_snapshot();
    snapshot.desktop_manager = Some(shortcut(Path::new(r"C:\Other\codex-plus-plus-manager.exe")));

    let assessment = assess_windows_desktop_integration(&snapshot);

    assert_eq!(assessment.health, DesktopIntegrationHealth::NeedsRepair);
    assert_eq!(assessment.plan.expect("repair plan").operations.len(), 1);
}

fn macos_bundle_path() -> PathBuf {
    PathBuf::from("/Applications/CodexPlusPlusManager.app")
}

fn macos_executable_path() -> PathBuf {
    macos_bundle_path()
        .join("Contents")
        .join("MacOS")
        .join(MANAGER_BINARY)
}

fn plist(bundle_id: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>{bundle_id}</string>
</dict>
</plist>"#
    )
    .into_bytes()
}

fn current_macos_snapshot() -> MacosDesktopSnapshot {
    MacosDesktopSnapshot {
        current_exe: macos_executable_path(),
        info_plist: Some(plist(MANAGER_BUNDLE_ID)),
        registered: true,
    }
}

#[test]
fn macos_current_registration_has_no_plan() {
    let assessment = assess_macos_desktop_integration(&current_macos_snapshot());

    assert_eq!(assessment.health, DesktopIntegrationHealth::Current);
    assert!(assessment.plan.is_none());
    assert_eq!(
        assessment.items,
        vec![
            codex_plus_core::desktop_integration::DesktopIntegrationItem {
                kind: DesktopIntegrationItemKind::MacosBundleRegistration,
                state: DesktopIntegrationItemState::Current,
            }
        ]
    );
}

#[test]
fn macos_missing_registration_plans_only_the_exact_existing_bundle() {
    let mut snapshot = current_macos_snapshot();
    snapshot.registered = false;

    let assessment = assess_macos_desktop_integration(&snapshot);

    assert_eq!(assessment.health, DesktopIntegrationHealth::NeedsRepair);
    let plan = assessment.plan.expect("registration plan");
    assert_eq!(plan.operations.len(), 1);
    match &plan.operations[0] {
        DesktopRepairOperation::RegisterMacosBundle { bundle_path } => {
            assert_eq!(bundle_path, &macos_bundle_path());
        }
        operation => panic!(
            "unexpected operation kind: {}",
            operation.item_kind().as_str()
        ),
    }
    let debug = format!("{plan:?}");
    assert!(!debug.contains("/Applications"));
    for forbidden in ["create", "copy", "replace", "delete"] {
        assert!(!debug.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn macos_rejects_missing_malformed_oversized_and_wrong_id_plists() {
    let cases = [
        None,
        Some(b"not a plist".to_vec()),
        Some(vec![b'x'; MAX_MACOS_INFO_PLIST_BYTES + 1]),
        Some(plist("com.example.unowned")),
    ];

    for info_plist in cases {
        let mut snapshot = current_macos_snapshot();
        snapshot.info_plist = info_plist;
        let assessment = assess_macos_desktop_integration(&snapshot);
        assert_eq!(
            assessment.health,
            DesktopIntegrationHealth::ReinstallRequired
        );
        assert!(assessment.plan.is_none());
    }
}

#[test]
fn macos_rejects_executables_outside_the_stable_bundle_layout() {
    let invalid_paths = [
        PathBuf::from(format!("/usr/local/bin/{MANAGER_BINARY}")),
        macos_bundle_path().join(MANAGER_BINARY),
        macos_bundle_path()
            .join("Contents")
            .join("MacOS")
            .join("codex-plus-manager-native"),
    ];

    for current_exe in invalid_paths {
        let mut snapshot = current_macos_snapshot();
        snapshot.current_exe = current_exe;
        let assessment = assess_macos_desktop_integration(&snapshot);
        assert_eq!(
            assessment.health,
            DesktopIntegrationHealth::ReinstallRequired
        );
        assert!(assessment.plan.is_none());
    }
}
