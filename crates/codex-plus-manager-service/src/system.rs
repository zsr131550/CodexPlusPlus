use std::path::{Path, PathBuf};

use codex_plus_core::install::{self, EntryPointState};
use codex_plus_core::settings::SettingsStore;
use codex_plus_core::status::{LaunchStatus, StatusStore};

use crate::{OverviewEnvironment, OverviewService};

#[derive(Debug, Clone, Copy)]
pub struct SystemOverviewEnvironment;

pub type SystemOverviewSource = OverviewService<SystemOverviewEnvironment>;

impl Default for OverviewService<SystemOverviewEnvironment> {
    fn default() -> Self {
        Self::new(SystemOverviewEnvironment)
    }
}

impl OverviewEnvironment for SystemOverviewEnvironment {
    fn saved_codex_app_path(&self) -> String {
        SettingsStore::default()
            .load()
            .unwrap_or_default()
            .codex_app_path
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        codex_plus_core::app_paths::resolve_codex_app_dir_with_saved(None, Some(saved))
    }

    fn codex_app_version(&self, path: &Path) -> Option<String> {
        codex_plus_core::app_paths::codex_app_version(path)
    }

    fn entrypoints(&self) -> EntryPointState {
        install::inspect_entrypoints()
    }

    fn latest_launch(&self) -> Option<LaunchStatus> {
        StatusStore::default().load_latest().unwrap_or(None)
    }

    fn current_version(&self) -> String {
        codex_plus_core::version::VERSION.to_owned()
    }

    fn settings_path(&self) -> PathBuf {
        codex_plus_core::paths::default_settings_path()
    }

    fn logs_path(&self) -> PathBuf {
        codex_plus_core::paths::default_diagnostic_log_path()
    }
}
