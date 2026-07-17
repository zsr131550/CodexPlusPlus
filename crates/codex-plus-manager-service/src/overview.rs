use std::path::{Path, PathBuf};

use codex_plus_core::install::EntryPointState;
use codex_plus_core::status::LaunchStatus;

use crate::OverviewError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePresence {
    Found,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedResource {
    pub presence: ResourcePresence,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSnapshot {
    pub installed: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckState {
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewSnapshot {
    pub codex_app: LocatedResource,
    pub codex_version: Option<String>,
    pub silent_shortcut: ShortcutSnapshot,
    pub management_shortcut: ShortcutSnapshot,
    pub latest_launch: Option<LaunchStatus>,
    pub current_version: String,
    pub update_status: UpdateCheckState,
    pub settings_path: PathBuf,
    pub logs_path: PathBuf,
}

pub trait OverviewSource: Send + Sync + 'static {
    fn load_overview(&self) -> Result<OverviewSnapshot, OverviewError>;
}

pub trait OverviewEnvironment: Send + Sync + 'static {
    fn saved_codex_app_path(&self) -> String;
    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf>;
    fn codex_app_version(&self, path: &Path) -> Option<String>;
    fn entrypoints(&self) -> EntryPointState;
    fn latest_launch(&self) -> Option<LaunchStatus>;
    fn current_version(&self) -> String;
    fn settings_path(&self) -> PathBuf;
    fn logs_path(&self) -> PathBuf;
}

pub struct OverviewService<E> {
    environment: E,
}

impl<E> OverviewService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: OverviewEnvironment> OverviewSource for OverviewService<E> {
    fn load_overview(&self) -> Result<OverviewSnapshot, OverviewError> {
        let saved = self.environment.saved_codex_app_path();
        let codex_path = self.environment.resolve_codex_app(&saved);
        let codex_version = codex_path
            .as_deref()
            .and_then(|path| self.environment.codex_app_version(path));
        let entrypoints = self.environment.entrypoints();

        Ok(OverviewSnapshot {
            codex_app: LocatedResource {
                presence: if codex_path.is_some() {
                    ResourcePresence::Found
                } else {
                    ResourcePresence::Missing
                },
                path: codex_path,
            },
            codex_version,
            silent_shortcut: ShortcutSnapshot {
                installed: entrypoints.silent_shortcut.installed,
                path: entrypoints.silent_shortcut.path.map(PathBuf::from),
            },
            management_shortcut: ShortcutSnapshot {
                installed: entrypoints.management_shortcut.installed,
                path: entrypoints.management_shortcut.path.map(PathBuf::from),
            },
            latest_launch: self.environment.latest_launch(),
            current_version: self.environment.current_version(),
            update_status: UpdateCheckState::NotChecked,
            settings_path: self.environment.settings_path(),
            logs_path: self.environment.logs_path(),
        })
    }
}
