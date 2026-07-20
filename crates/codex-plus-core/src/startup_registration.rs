use std::fmt;
use std::path::{Path, PathBuf};

use crate::desktop_integration::ShortcutSnapshot;
use crate::install::SILENT_BINARY;

pub const STARTUP_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const CANONICAL_RUN_NAME: &str = "CodexPlusPlus";
pub const LEGACY_RUN_NAME: &str = "CodexPlusPlusWatcher";
pub const LEGACY_STARTUP_SHORTCUT_NAME: &str = "CodexPlusPlusWatcher.lnk";

#[derive(Clone, PartialEq, Eq)]
pub enum OwnedStringValueSnapshot {
    Absent,
    String(String),
    UnsupportedType,
}

impl OwnedStringValueSnapshot {
    const fn is_present(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

impl fmt::Debug for OwnedStringValueSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Absent => "Absent",
            Self::String(_) => "String(<redacted>)",
            Self::UnsupportedType => "UnsupportedType",
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StartupRegistrationSnapshot {
    pub launcher_path: PathBuf,
    pub launcher_is_file: bool,
    pub canonical_run: OwnedStringValueSnapshot,
    pub legacy_run: OwnedStringValueSnapshot,
    pub legacy_shortcut: Option<ShortcutSnapshot>,
}

impl fmt::Debug for StartupRegistrationSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StartupRegistrationSnapshot")
            .field("launcher_is_file", &self.launcher_is_file)
            .field("canonical_run", &self.canonical_run)
            .field("legacy_run", &self.legacy_run)
            .field("legacy_shortcut", &self.legacy_shortcut.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAtSignInHealth {
    Enabled,
    Disabled,
    NeedsMigration,
    NeedsRepair,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartAtSignInInspection {
    pub health: StartAtSignInHealth,
    pub effective_enabled: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum StartupRegistrationOperation {
    SetRunValue { name: &'static str, value: String },
    DeleteRunValue { name: &'static str },
    DeleteLegacyStartupShortcut,
}

impl fmt::Debug for StartupRegistrationOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetRunValue { name, .. } => formatter
                .debug_struct("SetRunValue")
                .field("name", name)
                .field("value", &"<redacted>")
                .finish(),
            Self::DeleteRunValue { name } => formatter
                .debug_struct("DeleteRunValue")
                .field("name", name)
                .finish(),
            Self::DeleteLegacyStartupShortcut => formatter.write_str("DeleteLegacyStartupShortcut"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StartupRegistrationPlan {
    pub operations: Vec<StartupRegistrationOperation>,
}

impl fmt::Debug for StartupRegistrationPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StartupRegistrationPlan")
            .field("operation_count", &self.operations.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAtSignInPlanError {
    NotMigratable,
}

pub fn canonical_startup_command(launcher_path: &Path) -> String {
    format!("\"{}\"", launcher_path.display())
}

pub fn inspect_start_at_sign_in(snapshot: &StartupRegistrationSnapshot) -> StartAtSignInInspection {
    if !stable_launcher_available(snapshot) {
        return StartAtSignInInspection {
            health: StartAtSignInHealth::Unavailable,
            effective_enabled: false,
        };
    }

    let canonical_exact = canonical_is_exact(snapshot);
    let legacy_run_present = snapshot.legacy_run.is_present();
    let legacy_shortcut_present = snapshot.legacy_shortcut.is_some();
    let legacy_run_valid = legacy_run_is_valid(snapshot);
    let legacy_shortcut_valid = legacy_shortcut_is_valid(snapshot);
    let effective_enabled = canonical_exact || legacy_run_valid || legacy_shortcut_valid;
    let canonical_present = snapshot.canonical_run.is_present();
    let any_present = canonical_present || legacy_run_present || legacy_shortcut_present;
    let legacy_present = legacy_run_present || legacy_shortcut_present;
    let legacy_invalid = (legacy_run_present && !legacy_run_valid)
        || (legacy_shortcut_present && !legacy_shortcut_valid);

    let health = if canonical_exact && !legacy_present {
        StartAtSignInHealth::Enabled
    } else if !any_present {
        StartAtSignInHealth::Disabled
    } else if !canonical_present && legacy_present && !legacy_invalid {
        StartAtSignInHealth::NeedsMigration
    } else {
        StartAtSignInHealth::NeedsRepair
    };
    StartAtSignInInspection {
        health,
        effective_enabled,
    }
}

pub fn build_set_start_at_sign_in_plan(
    snapshot: &StartupRegistrationSnapshot,
    enabled: bool,
) -> StartupRegistrationPlan {
    if enabled {
        build_enable_plan(snapshot)
    } else {
        build_cleanup_plan(snapshot)
    }
}

pub fn build_migrate_start_at_sign_in_plan(
    snapshot: &StartupRegistrationSnapshot,
) -> Result<StartupRegistrationPlan, StartAtSignInPlanError> {
    if inspect_start_at_sign_in(snapshot).health != StartAtSignInHealth::NeedsMigration {
        return Err(StartAtSignInPlanError::NotMigratable);
    }
    Ok(build_enable_plan(snapshot))
}

pub fn build_package_upgrade_plan(
    snapshot: &StartupRegistrationSnapshot,
) -> StartupRegistrationPlan {
    if inspect_start_at_sign_in(snapshot).effective_enabled {
        build_enable_plan(snapshot)
    } else {
        build_cleanup_plan(snapshot)
    }
}

pub fn build_package_uninstall_plan(
    snapshot: &StartupRegistrationSnapshot,
) -> StartupRegistrationPlan {
    build_cleanup_plan(snapshot)
}

fn build_enable_plan(snapshot: &StartupRegistrationSnapshot) -> StartupRegistrationPlan {
    if !stable_launcher_available(snapshot) {
        return StartupRegistrationPlan {
            operations: Vec::new(),
        };
    }
    let mut operations = Vec::with_capacity(3);
    if !canonical_is_exact(snapshot) {
        operations.push(StartupRegistrationOperation::SetRunValue {
            name: CANONICAL_RUN_NAME,
            value: canonical_startup_command(&snapshot.launcher_path),
        });
    }
    if snapshot.legacy_run.is_present() {
        operations.push(StartupRegistrationOperation::DeleteRunValue {
            name: LEGACY_RUN_NAME,
        });
    }
    if snapshot.legacy_shortcut.is_some() {
        operations.push(StartupRegistrationOperation::DeleteLegacyStartupShortcut);
    }
    StartupRegistrationPlan { operations }
}

fn build_cleanup_plan(snapshot: &StartupRegistrationSnapshot) -> StartupRegistrationPlan {
    let mut operations = Vec::with_capacity(3);
    if snapshot.canonical_run.is_present() {
        operations.push(StartupRegistrationOperation::DeleteRunValue {
            name: CANONICAL_RUN_NAME,
        });
    }
    if snapshot.legacy_run.is_present() {
        operations.push(StartupRegistrationOperation::DeleteRunValue {
            name: LEGACY_RUN_NAME,
        });
    }
    if snapshot.legacy_shortcut.is_some() {
        operations.push(StartupRegistrationOperation::DeleteLegacyStartupShortcut);
    }
    StartupRegistrationPlan { operations }
}

fn stable_launcher_available(snapshot: &StartupRegistrationSnapshot) -> bool {
    snapshot.launcher_is_file
        && snapshot
            .launcher_path
            .file_name()
            .and_then(|name| name.to_str())
            == Some(&format!("{SILENT_BINARY}.exe"))
}

fn canonical_is_exact(snapshot: &StartupRegistrationSnapshot) -> bool {
    matches!(
        &snapshot.canonical_run,
        OwnedStringValueSnapshot::String(value)
            if value == &canonical_startup_command(&snapshot.launcher_path)
    )
}

fn legacy_run_is_valid(snapshot: &StartupRegistrationSnapshot) -> bool {
    let OwnedStringValueSnapshot::String(value) = &snapshot.legacy_run else {
        return false;
    };
    let prefix = format!("{} ", canonical_startup_command(&snapshot.launcher_path));
    value
        .strip_prefix(&prefix)
        .is_some_and(valid_legacy_debug_port_arguments)
}

fn legacy_shortcut_is_valid(snapshot: &StartupRegistrationSnapshot) -> bool {
    snapshot.legacy_shortcut.as_ref().is_some_and(|shortcut| {
        shortcut.target == snapshot.launcher_path
            && valid_legacy_debug_port_arguments(&shortcut.arguments)
    })
}

fn valid_legacy_debug_port_arguments(arguments: &str) -> bool {
    let mut parts = arguments.split_ascii_whitespace();
    parts.next() == Some("--debug-port")
        && parts
            .next()
            .and_then(|port| port.parse::<u16>().ok())
            .is_some()
        && parts.next().is_none()
}

#[cfg(windows)]
pub fn inspect_system_startup_registration(
    launcher_path: PathBuf,
) -> anyhow::Result<StartupRegistrationSnapshot> {
    let values = crate::windows_integration::read_current_user_string_values(STARTUP_RUN_KEY)?;
    let canonical_run = owned_registry_value(&values, CANONICAL_RUN_NAME);
    let legacy_run = owned_registry_value(&values, LEGACY_RUN_NAME);
    let legacy_shortcut = crate::windows_integration::startup_dir()
        .map(|dir| dir.join(LEGACY_STARTUP_SHORTCUT_NAME))
        .and_then(read_owned_legacy_shortcut);
    Ok(StartupRegistrationSnapshot {
        launcher_is_file: launcher_path.is_file(),
        launcher_path,
        canonical_run,
        legacy_run,
        legacy_shortcut,
    })
}

#[cfg(windows)]
pub fn apply_system_startup_registration_operation(
    operation: &StartupRegistrationOperation,
) -> anyhow::Result<()> {
    match operation {
        StartupRegistrationOperation::SetRunValue { name, value }
            if *name == CANONICAL_RUN_NAME =>
        {
            crate::windows_integration::set_current_user_string_value(STARTUP_RUN_KEY, name, value)
        }
        StartupRegistrationOperation::DeleteRunValue { name }
            if *name == CANONICAL_RUN_NAME || *name == LEGACY_RUN_NAME =>
        {
            crate::windows_integration::delete_current_user_value(STARTUP_RUN_KEY, name)
        }
        StartupRegistrationOperation::DeleteLegacyStartupShortcut => {
            let Some(path) = crate::windows_integration::startup_dir()
                .map(|dir| dir.join(LEGACY_STARTUP_SHORTCUT_NAME))
            else {
                return Ok(());
            };
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }
        _ => anyhow::bail!("operation is not an owned startup registration operation"),
    }
}

#[cfg(windows)]
fn owned_registry_value(
    values: &[(String, Option<String>)],
    name: &str,
) -> OwnedStringValueSnapshot {
    match values
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
    {
        Some((_, Some(value))) => OwnedStringValueSnapshot::String(value.clone()),
        Some((_, None)) => OwnedStringValueSnapshot::UnsupportedType,
        None => OwnedStringValueSnapshot::Absent,
    }
}

#[cfg(windows)]
fn read_owned_legacy_shortcut(path: PathBuf) -> Option<ShortcutSnapshot> {
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
