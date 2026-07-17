use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_manager_native::i18n::{Locale, ThemeMode};
use codex_plus_manager_native::state::{OverviewPhase, Route};
use codex_plus_manager_native::views::shell::ShellViewModel;
use codex_plus_manager_service::{
    LocatedResource, OverviewSnapshot, ResourcePresence, ShortcutSnapshot, UpdateCheckState,
};

pub fn snapshot(codex_version: &str) -> Arc<OverviewSnapshot> {
    Arc::new(OverviewSnapshot {
        codex_app: LocatedResource {
            presence: ResourcePresence::Found,
            path: Some(PathBuf::from("C:/Program Files/Codex")),
        },
        codex_version: Some(codex_version.to_owned()),
        silent_shortcut: ShortcutSnapshot {
            installed: true,
            path: Some(PathBuf::from("C:/Users/Test/Desktop/Codex++.lnk")),
        },
        management_shortcut: ShortcutSnapshot {
            installed: true,
            path: Some(PathBuf::from("C:/Users/Test/Desktop/Codex++ Manager.lnk")),
        },
        latest_launch: Some(codex_plus_core::status::LaunchStatus {
            status: "running".to_owned(),
            message: "ready".to_owned(),
            started_at_ms: 42,
            debug_port: Some(9229),
            helper_port: Some(57321),
            codex_app: Some("C:/Program Files/Codex".to_owned()),
        }),
        current_version: "1.2.36".to_owned(),
        update_status: UpdateCheckState::NotChecked,
        settings_path: PathBuf::from("C:/Users/Test/AppData/Roaming/Codex++/settings.json"),
        logs_path: PathBuf::from("C:/Users/Test/AppData/Roaming/Codex++/diagnostic.log"),
    })
}

pub fn model(locale: Locale, theme: ThemeMode) -> ShellViewModel {
    ShellViewModel {
        route: Route::Overview,
        locale,
        theme,
        overview_phase: OverviewPhase::Ready,
        overview_snapshot: Some(snapshot("0.16.0")),
        overview_error: None,
        last_updated: Some("12:34:56 UTC".to_owned()),
        renderer: "WGPU".to_owned(),
    }
}
