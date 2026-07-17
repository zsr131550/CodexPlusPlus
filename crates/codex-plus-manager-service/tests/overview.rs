use std::path::{Path, PathBuf};

use codex_plus_core::install::{EntryPointState, ShortcutState};
use codex_plus_core::status::LaunchStatus;
use codex_plus_manager_service::{
    OverviewEnvironment, OverviewService, OverviewSource, ResourcePresence, UpdateCheckState,
};

#[derive(Clone)]
struct FakeEnvironment {
    resolved_codex_app: Option<PathBuf>,
}

impl FakeEnvironment {
    fn with_codex_app(path: impl Into<PathBuf>) -> Self {
        Self {
            resolved_codex_app: Some(path.into()),
        }
    }

    fn without_codex_app() -> Self {
        Self {
            resolved_codex_app: None,
        }
    }
}

impl OverviewEnvironment for FakeEnvironment {
    fn saved_codex_app_path(&self) -> String {
        "C:/saved/Codex".to_owned()
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        assert_eq!(saved, "C:/saved/Codex");
        self.resolved_codex_app.clone()
    }

    fn codex_app_version(&self, path: &Path) -> Option<String> {
        assert_eq!(path, Path::new("C:/resolved/Codex"));
        Some("0.16.0".to_owned())
    }

    fn entrypoints(&self) -> EntryPointState {
        EntryPointState {
            silent_shortcut: ShortcutState {
                installed: true,
                path: Some("C:/Desktop/Codex++.lnk".to_owned()),
            },
            management_shortcut: ShortcutState {
                installed: false,
                path: Some("C:/Desktop/Codex++ Manager.lnk".to_owned()),
            },
        }
    }

    fn latest_launch(&self) -> Option<LaunchStatus> {
        Some(LaunchStatus {
            status: "running".to_owned(),
            message: "ready".to_owned(),
            started_at_ms: 42,
            debug_port: Some(9229),
            helper_port: Some(57321),
            codex_app: Some("C:/resolved/Codex".to_owned()),
        })
    }

    fn current_version(&self) -> String {
        "1.2.36".to_owned()
    }

    fn settings_path(&self) -> PathBuf {
        PathBuf::from("C:/state/settings.json")
    }

    fn logs_path(&self) -> PathBuf {
        PathBuf::from("C:/state/diagnostic.log")
    }
}

#[test]
fn overview_service_composes_typed_snapshot() {
    let snapshot = OverviewService::new(FakeEnvironment::with_codex_app("C:/resolved/Codex"))
        .load_overview()
        .expect("load fake overview");

    assert_eq!(snapshot.codex_app.presence, ResourcePresence::Found);
    assert_eq!(
        snapshot.codex_app.path,
        Some(PathBuf::from("C:/resolved/Codex"))
    );
    assert_eq!(snapshot.codex_version.as_deref(), Some("0.16.0"));
    assert!(snapshot.silent_shortcut.installed);
    assert!(!snapshot.management_shortcut.installed);
    assert_eq!(snapshot.latest_launch.unwrap().debug_port, Some(9229));
    assert_eq!(snapshot.current_version, "1.2.36");
    assert_eq!(snapshot.update_status, UpdateCheckState::NotChecked);
    assert_eq!(
        snapshot.settings_path,
        PathBuf::from("C:/state/settings.json")
    );
    assert_eq!(snapshot.logs_path, PathBuf::from("C:/state/diagnostic.log"));
}

#[test]
fn overview_service_marks_missing_codex_app_without_version_probe() {
    let snapshot = OverviewService::new(FakeEnvironment::without_codex_app())
        .load_overview()
        .expect("load fake overview");

    assert_eq!(snapshot.codex_app.presence, ResourcePresence::Missing);
    assert_eq!(snapshot.codex_app.path, None);
    assert_eq!(snapshot.codex_version, None);
}
