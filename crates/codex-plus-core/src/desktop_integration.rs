use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::install::{MANAGER_BINARY, MANAGER_NAME, SILENT_BINARY, SILENT_NAME};

pub const WINDOWS_PROTOCOL_COMMAND_SUBKEY: &str =
    r"Software\Classes\codexplusplus\shell\open\command";
pub const WINDOWS_PROTOCOL_SUBKEY: &str = r"Software\Classes\codexplusplus";
pub const MAX_MACOS_INFO_PLIST_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationHealth {
    Current,
    NeedsRepair,
    ReinstallRequired,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationItemKind {
    DesktopManagerShortcut,
    StartMenuLauncherShortcut,
    StartMenuManagerShortcut,
    UrlProtocol,
    MacosBundleRegistration,
}

impl DesktopIntegrationItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DesktopManagerShortcut => "desktop_manager_shortcut",
            Self::StartMenuLauncherShortcut => "start_menu_launcher_shortcut",
            Self::StartMenuManagerShortcut => "start_menu_manager_shortcut",
            Self::UrlProtocol => "url_protocol",
            Self::MacosBundleRegistration => "macos_bundle_registration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationItemState {
    Current,
    NeedsRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopIntegrationItem {
    pub kind: DesktopIntegrationItemKind,
    pub state: DesktopIntegrationItemState,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ShortcutSnapshot {
    pub target: PathBuf,
    pub arguments: String,
}

impl fmt::Debug for ShortcutSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShortcutSnapshot")
            .field("target", &"<redacted>")
            .field("has_arguments", &!self.arguments.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct WindowsDesktopSnapshot {
    pub current_exe: PathBuf,
    pub launcher_is_file: bool,
    pub desktop_dir: Option<PathBuf>,
    pub programs_dir: Option<PathBuf>,
    pub desktop_manager: Option<ShortcutSnapshot>,
    pub start_menu_launcher: Option<ShortcutSnapshot>,
    pub start_menu_manager: Option<ShortcutSnapshot>,
    pub protocol_command: Option<String>,
}

impl fmt::Debug for WindowsDesktopSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsDesktopSnapshot")
            .field("launcher_is_file", &self.launcher_is_file)
            .field("desktop_dir_available", &self.desktop_dir.is_some())
            .field("programs_dir_available", &self.programs_dir.is_some())
            .field("desktop_manager", &self.desktop_manager.is_some())
            .field("start_menu_launcher", &self.start_menu_launcher.is_some())
            .field("start_menu_manager", &self.start_menu_manager.is_some())
            .field("protocol_command", &self.protocol_command.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MacosDesktopSnapshot {
    pub current_exe: PathBuf,
    pub info_plist: Option<Vec<u8>>,
    pub registered: bool,
}

impl fmt::Debug for MacosDesktopSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosDesktopSnapshot")
            .field("info_plist_bytes", &self.info_plist.as_ref().map(Vec::len))
            .field("registered", &self.registered)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum DesktopRepairOperation {
    WriteShortcut {
        kind: DesktopIntegrationItemKind,
        path: PathBuf,
        target: PathBuf,
        arguments: String,
    },
    WriteProtocol {
        subkey: &'static str,
        command: String,
    },
    RegisterMacosBundle {
        bundle_path: PathBuf,
    },
}

impl DesktopRepairOperation {
    pub const fn item_kind(&self) -> DesktopIntegrationItemKind {
        match self {
            Self::WriteShortcut { kind, .. } => *kind,
            Self::WriteProtocol { .. } => DesktopIntegrationItemKind::UrlProtocol,
            Self::RegisterMacosBundle { .. } => DesktopIntegrationItemKind::MacosBundleRegistration,
        }
    }
}

impl fmt::Debug for DesktopRepairOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopRepairOperation")
            .field("item_kind", &self.item_kind())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DesktopRepairPlan {
    pub operations: Vec<DesktopRepairOperation>,
}

impl fmt::Debug for DesktopRepairPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopRepairPlan")
            .field(
                "item_kinds",
                &self
                    .operations
                    .iter()
                    .map(DesktopRepairOperation::item_kind)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopIntegrationAssessment {
    pub health: DesktopIntegrationHealth,
    pub items: Vec<DesktopIntegrationItem>,
    pub plan: Option<DesktopRepairPlan>,
}

pub fn assess_windows_desktop_integration(
    snapshot: &WindowsDesktopSnapshot,
) -> DesktopIntegrationAssessment {
    let Some(install_dir) = stable_windows_install_dir(snapshot) else {
        return unavailable_or_reinstall(snapshot);
    };
    let (Some(desktop_dir), Some(programs_dir)) = (
        snapshot.desktop_dir.as_ref(),
        snapshot.programs_dir.as_ref(),
    ) else {
        return DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::Unavailable,
            items: Vec::new(),
            plan: None,
        };
    };

    let manager_path = snapshot.current_exe.clone();
    let launcher_path = install_dir.join(windows_binary_name(SILENT_BINARY));
    let start_menu_dir = programs_dir.join(SILENT_NAME);
    let expected_protocol = format!("\"{}\" \"%1\"", manager_path.display());

    let expected = [
        (
            DesktopIntegrationItemKind::DesktopManagerShortcut,
            desktop_dir.join(format!("{MANAGER_NAME}.lnk")),
            manager_path.clone(),
            snapshot.desktop_manager.as_ref(),
        ),
        (
            DesktopIntegrationItemKind::StartMenuLauncherShortcut,
            start_menu_dir.join(format!("{SILENT_NAME}.lnk")),
            launcher_path,
            snapshot.start_menu_launcher.as_ref(),
        ),
        (
            DesktopIntegrationItemKind::StartMenuManagerShortcut,
            start_menu_dir.join(format!("{MANAGER_NAME}.lnk")),
            manager_path,
            snapshot.start_menu_manager.as_ref(),
        ),
    ];

    let mut items = Vec::with_capacity(4);
    let mut operations = Vec::new();
    for (kind, path, target, current) in expected {
        let state = if current
            .is_some_and(|shortcut| shortcut.target == target && shortcut.arguments.is_empty())
        {
            DesktopIntegrationItemState::Current
        } else {
            operations.push(DesktopRepairOperation::WriteShortcut {
                kind,
                path,
                target,
                arguments: String::new(),
            });
            DesktopIntegrationItemState::NeedsRepair
        };
        items.push(DesktopIntegrationItem { kind, state });
    }

    let protocol_state = if snapshot.protocol_command.as_deref() == Some(&expected_protocol) {
        DesktopIntegrationItemState::Current
    } else {
        operations.push(DesktopRepairOperation::WriteProtocol {
            subkey: WINDOWS_PROTOCOL_COMMAND_SUBKEY,
            command: expected_protocol,
        });
        DesktopIntegrationItemState::NeedsRepair
    };
    items.push(DesktopIntegrationItem {
        kind: DesktopIntegrationItemKind::UrlProtocol,
        state: protocol_state,
    });

    if operations.is_empty() {
        DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::Current,
            items,
            plan: None,
        }
    } else {
        DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::NeedsRepair,
            items,
            plan: Some(DesktopRepairPlan { operations }),
        }
    }
}

pub fn assess_macos_desktop_integration(
    snapshot: &MacosDesktopSnapshot,
) -> DesktopIntegrationAssessment {
    let Some(bundle_path) = stable_macos_bundle_path(&snapshot.current_exe) else {
        return reinstall_required();
    };
    let Some(info_plist) = snapshot.info_plist.as_deref() else {
        return reinstall_required();
    };
    if info_plist.len() > MAX_MACOS_INFO_PLIST_BYTES
        || parsed_bundle_identifier(info_plist).as_deref()
            != Some(crate::install::MANAGER_BUNDLE_ID)
    {
        return reinstall_required();
    }

    let state = if snapshot.registered {
        DesktopIntegrationItemState::Current
    } else {
        DesktopIntegrationItemState::NeedsRepair
    };
    let items = vec![DesktopIntegrationItem {
        kind: DesktopIntegrationItemKind::MacosBundleRegistration,
        state,
    }];
    if snapshot.registered {
        DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::Current,
            items,
            plan: None,
        }
    } else {
        DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::NeedsRepair,
            items,
            plan: Some(DesktopRepairPlan {
                operations: vec![DesktopRepairOperation::RegisterMacosBundle { bundle_path }],
            }),
        }
    }
}

fn stable_windows_install_dir(snapshot: &WindowsDesktopSnapshot) -> Option<&Path> {
    let expected_name = windows_binary_name(MANAGER_BINARY);
    if snapshot.current_exe.file_name()?.to_str()? != expected_name || !snapshot.launcher_is_file {
        return None;
    }
    snapshot.current_exe.parent()
}

fn stable_macos_bundle_path(current_exe: &Path) -> Option<PathBuf> {
    if current_exe.file_name()?.to_str()? != MANAGER_BINARY {
        return None;
    }
    let macos_dir = current_exe.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let bundle_path = contents_dir.parent()?;
    if bundle_path.extension()?.to_str()? != "app" {
        return None;
    }
    Some(bundle_path.to_path_buf())
}

fn parsed_bundle_identifier(info_plist: &[u8]) -> Option<String> {
    let value = plist::Value::from_reader(Cursor::new(info_plist)).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(ToOwned::to_owned)
}

fn reinstall_required() -> DesktopIntegrationAssessment {
    DesktopIntegrationAssessment {
        health: DesktopIntegrationHealth::ReinstallRequired,
        items: Vec::new(),
        plan: None,
    }
}

fn unavailable_or_reinstall(snapshot: &WindowsDesktopSnapshot) -> DesktopIntegrationAssessment {
    let health = if snapshot.current_exe.parent().is_some() {
        DesktopIntegrationHealth::ReinstallRequired
    } else {
        DesktopIntegrationHealth::Unavailable
    };
    DesktopIntegrationAssessment {
        health,
        items: Vec::new(),
        plan: None,
    }
}

fn windows_binary_name(binary: &str) -> String {
    format!("{binary}.exe")
}

#[cfg(windows)]
pub fn inspect_system_windows_desktop(
    current_exe: PathBuf,
) -> anyhow::Result<WindowsDesktopSnapshot> {
    let desktop_dir = crate::windows_integration::desktop_dir();
    let programs_dir = crate::windows_integration::programs_dir();
    let install_dir = current_exe.parent().map(Path::to_path_buf);
    let launcher_path = install_dir
        .as_ref()
        .map(|dir| dir.join(windows_binary_name(SILENT_BINARY)));
    let start_menu_dir = programs_dir.as_ref().map(|dir| dir.join(SILENT_NAME));

    let desktop_manager = desktop_dir
        .as_ref()
        .map(|dir| dir.join(format!("{MANAGER_NAME}.lnk")))
        .and_then(read_owned_shortcut);
    let start_menu_launcher = start_menu_dir
        .as_ref()
        .map(|dir| dir.join(format!("{SILENT_NAME}.lnk")))
        .and_then(read_owned_shortcut);
    let start_menu_manager = start_menu_dir
        .as_ref()
        .map(|dir| dir.join(format!("{MANAGER_NAME}.lnk")))
        .and_then(read_owned_shortcut);
    let protocol_command = inspect_protocol_command()?;

    Ok(WindowsDesktopSnapshot {
        current_exe,
        launcher_is_file: launcher_path.as_ref().is_some_and(|path| path.is_file()),
        desktop_dir,
        programs_dir,
        desktop_manager,
        start_menu_launcher,
        start_menu_manager,
        protocol_command,
    })
}

#[cfg(windows)]
pub fn apply_system_windows_repair_operation(
    operation: &DesktopRepairOperation,
) -> anyhow::Result<()> {
    match operation {
        DesktopRepairOperation::WriteShortcut {
            path,
            target,
            arguments,
            ..
        } => {
            crate::windows_integration::create_shortcut(&crate::windows_integration::ShortcutSpec {
                path: path.clone(),
                target: target.clone(),
                arguments: arguments.clone(),
                working_directory: target.parent().map(Path::to_path_buf),
                description: "Codex++ desktop integration".to_string(),
                icon: Some(target.clone()),
                show_minimized: false,
            })
        }
        DesktopRepairOperation::WriteProtocol { subkey, command }
            if *subkey == WINDOWS_PROTOCOL_COMMAND_SUBKEY =>
        {
            crate::windows_integration::set_current_user_string_value(
                WINDOWS_PROTOCOL_SUBKEY,
                "",
                "URL:Codex++ Import Protocol",
            )?;
            crate::windows_integration::set_current_user_string_value(
                WINDOWS_PROTOCOL_SUBKEY,
                "URL Protocol",
                "",
            )?;
            crate::windows_integration::set_current_user_string_value(subkey, "", command)
        }
        _ => anyhow::bail!("operation is not a Windows desktop repair operation"),
    }
}

#[cfg(windows)]
fn read_owned_shortcut(path: PathBuf) -> Option<ShortcutSnapshot> {
    if !path.exists() {
        return None;
    }
    match crate::windows_integration::read_shortcut(&path) {
        Ok(Some(shortcut)) => Some(shortcut),
        _ => Some(ShortcutSnapshot {
            target: PathBuf::new(),
            arguments: String::new(),
        }),
    }
}

#[cfg(windows)]
fn inspect_protocol_command() -> anyhow::Result<Option<String>> {
    let root =
        crate::windows_integration::read_current_user_string_values(WINDOWS_PROTOCOL_SUBKEY)?;
    let label_current = registry_string(&root, "") == Some("URL:Codex++ Import Protocol");
    let marker_current = registry_string(&root, "URL Protocol") == Some("");
    if !label_current || !marker_current {
        return Ok(None);
    }
    let command = crate::windows_integration::read_current_user_string_values(
        WINDOWS_PROTOCOL_COMMAND_SUBKEY,
    )?;
    Ok(registry_string(&command, "").map(ToOwned::to_owned))
}

#[cfg(windows)]
fn registry_string<'a>(values: &'a [(String, Option<String>)], name: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_deref())
}

#[cfg(target_os = "macos")]
pub fn inspect_system_macos_desktop(current_exe: PathBuf) -> anyhow::Result<MacosDesktopSnapshot> {
    use std::io::Read;

    let bundle_path = stable_macos_bundle_path(&current_exe);
    let info_plist = bundle_path.as_ref().and_then(|bundle_path| {
        let path = bundle_path.join("Contents").join("Info.plist");
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take((MAX_MACOS_INFO_PLIST_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(bytes)
    });
    let registered = bundle_path
        .as_deref()
        .is_some_and(|bundle_path| macos_bundle_is_registered(bundle_path).unwrap_or(false));
    Ok(MacosDesktopSnapshot {
        current_exe,
        info_plist,
        registered,
    })
}

#[cfg(target_os = "macos")]
pub fn apply_system_macos_repair_operation(
    operation: &DesktopRepairOperation,
) -> anyhow::Result<()> {
    let DesktopRepairOperation::RegisterMacosBundle { bundle_path } = operation else {
        anyhow::bail!("operation is not a macOS desktop repair operation");
    };
    let status = std::process::Command::new(macos_lsregister_path())
        .arg("-f")
        .arg(bundle_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("LaunchServices registration failed");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_bundle_is_registered(bundle_path: &Path) -> anyhow::Result<bool> {
    use std::io::Read;
    use std::process::Stdio;

    const MAX_DUMP_BYTES: usize = 8 * 1024 * 1024;
    let mut child = std::process::Command::new(macos_lsregister_path())
        .arg("-dump")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("LaunchServices output is unavailable"))?
        .take((MAX_DUMP_BYTES + 1) as u64)
        .read_to_end(&mut output)?;
    if output.len() > MAX_DUMP_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!("LaunchServices output exceeded the inspection limit");
    }
    if !child.wait()?.success() {
        anyhow::bail!("LaunchServices inspection failed");
    }
    let output = String::from_utf8_lossy(&output);
    let path = bundle_path.to_string_lossy();
    Ok(output.match_indices(path.as_ref()).any(|(index, _)| {
        let start = index.saturating_sub(4_096);
        let end = (index + path.len() + 4_096).min(output.len());
        output
            .get(start..end)
            .is_some_and(|record| record.contains(crate::install::MANAGER_BUNDLE_ID))
    }))
}

#[cfg(target_os = "macos")]
fn macos_lsregister_path() -> &'static str {
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
}
