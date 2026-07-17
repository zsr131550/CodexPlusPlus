#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use codex_plus_manager_native::app::NativeManagerApp;
use codex_plus_manager_native::fonts;
use codex_plus_manager_service::SystemOverviewSource;
use eframe::egui;

const APP_ID: &str = "com.codexplusplus.manager.native";
const APP_TITLE: &str = "Codex++ Native Manager";

fn main() -> eframe::Result {
    let cjk_font = match fonts::load_cjk_font() {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                "native_manager.cjk_font_unavailable",
                serde_json::json!({
                    "error": error.to_string(),
                    "attempted": error.attempted(),
                }),
            );
            None
        }
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_title(APP_TITLE)
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([960.0, 720.0]),
        renderer: eframe::Renderer::Wgpu,
        persist_window: true,
        persistence_path: persistence_path_from_env(),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(move |creation| {
            Ok(Box::new(NativeManagerApp::new(
                creation,
                cjk_font,
                Arc::new(SystemOverviewSource::default()),
            )))
        }),
    )
}

fn persistence_path_from_env() -> Option<PathBuf> {
    std::env::var_os("CODEX_PLUS_NATIVE_STATE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|directory| directory.join("app.ron"))
}
