#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use codex_plus_manager_native::app::{NativeManagerApp, NativeManagerSources};
use codex_plus_manager_native::desktop_host::{
    APP_ID, APP_TITLE, DesktopHostBootstrap, DesktopHostRuntime, NativePersistencePaths,
};
use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::Locale;
use codex_plus_manager_native::path_picker::path_picker_from_environment;
use codex_plus_manager_native::perf::PerfRecorder;
use codex_plus_manager_service::{
    ContextToolsService, DesktopStartupArgs, EnhancementSettingsService, MaintenanceService,
    ManagerSettingsService, OverviewService, PluginMarketplaceService, ProviderImportService,
    ProviderService, ProviderSyncService, RelayEnvironmentService, SessionService,
    SystemProviderEnvironment, UserScriptService, ZedRemoteService,
};
use eframe::egui;

const MEBIBYTE: u64 = 1024 * 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_diagnostic_log_from_env();
    let process_started = Instant::now();
    let persistence_paths = NativePersistencePaths::for_state_override(native_state_override());
    if let Err(error) = persistence_paths.migrate_legacy_if_needed() {
        let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
            "native_manager.preference_migration_failed",
            serde_json::json!({ "kind": format!("{error:?}") }),
        );
    }
    let environment = SystemProviderEnvironment::for_native_process();
    let startup = DesktopStartupArgs::new(std::env::args_os());
    let startup_plan = startup.prepare(&environment);
    for issue in startup_plan.issues() {
        let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
            "native_manager.startup_argument_ignored",
            serde_json::json!({
                "kind": format!("{:?}", issue.kind()),
                "argument_index": issue.argument_index(),
            }),
        );
    }
    let instance = codex_plus_core::manager_instance::acquire_manager_instance(
        codex_plus_core::manager_instance::ManagerInstanceConfig::for_state_dir(
            persistence_paths.state_root(),
        ),
    )
    .inspect_err(|error| {
        log_desktop_host_failure("native_manager.instance_acquire_failed", error);
    })?;
    let desktop_host = match instance {
        codex_plus_core::manager_instance::ManagerInstance::Secondary(client) => {
            for action in startup_plan.actions().iter().copied() {
                client.send(action).inspect_err(|error| {
                    log_desktop_host_failure("native_manager.activation_send_failed", error);
                })?;
            }
            return Ok(());
        }
        codex_plus_core::manager_instance::ManagerInstance::Primary(owner) => {
            DesktopHostBootstrap::new(owner, startup_plan.into_actions())
        }
    };
    let perf = PerfRecorder::from_env(process_started);
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
        wgpu_options: memory_efficient_wgpu_configuration(),
        persist_window: true,
        persistence_path: Some(persistence_paths.canonical().to_path_buf()),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(move |creation| {
            let provider_service = Arc::new(ProviderService::new(environment.clone()));
            let import_service = Arc::new(ProviderImportService::new(environment.clone()));
            let context_service = Arc::new(ContextToolsService::new(environment.clone()));
            let enhancement_service =
                Arc::new(EnhancementSettingsService::new(environment.clone()));
            let marketplace_service = Arc::new(PluginMarketplaceService::new(environment.clone()));
            let session_service = Arc::new(SessionService::new(environment.clone()));
            let provider_sync_service = Arc::new(ProviderSyncService::new(environment.clone()));
            let user_script_service = Arc::new(UserScriptService::new(environment.clone()));
            let zed_remote_service = Arc::new(ZedRemoteService::new(environment.clone()));
            let maintenance_service = Arc::new(MaintenanceService::new(environment.clone()));
            let manager_settings_service =
                Arc::new(ManagerSettingsService::new(environment.clone()));
            let overview_service = Arc::new(OverviewService::new(environment.clone()));
            let environment_service = Arc::new(RelayEnvironmentService::new(environment));
            let desktop_host =
                DesktopHostRuntime::start(desktop_host, creation.egui_ctx.clone(), Locale::ZhCn)
                    .inspect_err(|error| {
                        log_desktop_host_failure("native_manager.desktop_host_start_failed", error);
                    })?;
            Ok(Box::new(NativeManagerApp::new_with_desktop_host(
                creation,
                cjk_font,
                NativeManagerSources {
                    overview: overview_service,
                    provider: provider_service.clone(),
                    activation: provider_service,
                    provider_import: import_service,
                    environment: environment_service,
                    context: context_service,
                    enhancements: enhancement_service,
                    marketplace: marketplace_service,
                    sessions: session_service,
                    provider_sync: provider_sync_service,
                    user_scripts: user_script_service,
                    zed_remote: zed_remote_service,
                    maintenance: maintenance_service,
                    settings: manager_settings_service,
                    path_picker: path_picker_from_environment(),
                },
                perf,
                Some(desktop_host),
            )))
        }),
    )?;
    Ok(())
}

fn configure_diagnostic_log_from_env() {
    let Some(path) = std::env::var_os("CODEX_PLUS_NATIVE_DIAGNOSTIC_LOG_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return;
    };
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(path));
}

fn memory_efficient_wgpu_configuration() -> eframe::WgpuConfiguration {
    memory_efficient_wgpu_configuration_for(eframe::wgpu::Backends::from_env())
}

fn memory_efficient_wgpu_configuration_for(
    backend_override: Option<eframe::wgpu::Backends>,
) -> eframe::WgpuConfiguration {
    let mut configuration = eframe::WgpuConfiguration {
        surface: eframe::SurfaceConfig::LOW_LATENCY,
        ..Default::default()
    };
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut configuration.wgpu_setup else {
        unreachable!("default WGPU configuration must create a device")
    };
    setup.instance_descriptor.backends =
        backend_override.unwrap_or(platform_default_wgpu_backends());
    let default_device_descriptor = Arc::clone(&setup.device_descriptor);
    setup.device_descriptor = Arc::new(move |adapter| {
        let mut descriptor = default_device_descriptor(adapter);
        descriptor.memory_hints = eframe::wgpu::MemoryHints::Manual {
            suballocated_device_memory_block_size: 4 * MEBIBYTE..8 * MEBIBYTE,
        };
        descriptor
    });
    configuration
}

#[cfg(windows)]
fn platform_default_wgpu_backends() -> eframe::wgpu::Backends {
    eframe::wgpu::Backends::GL
}

#[cfg(not(windows))]
fn platform_default_wgpu_backends() -> eframe::wgpu::Backends {
    eframe::wgpu::Backends::PRIMARY | eframe::wgpu::Backends::GL
}

fn native_state_override() -> Option<PathBuf> {
    std::env::var_os("CODEX_PLUS_NATIVE_STATE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn log_desktop_host_failure(event: &str, error: &dyn std::fmt::Debug) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        event,
        serde_json::json!({ "kind": format!("{error:?}") }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgpu_configuration_uses_a_low_memory_windows_backend() {
        let configuration = memory_efficient_wgpu_configuration_for(None);
        assert_eq!(configuration.surface, eframe::SurfaceConfig::LOW_LATENCY);

        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = configuration.wgpu_setup else {
            panic!("configuration should create a WGPU device")
        };
        #[cfg(windows)]
        assert_eq!(
            setup.instance_descriptor.backends,
            eframe::wgpu::Backends::GL
        );
    }

    #[test]
    fn wgpu_backend_environment_override_is_preserved() {
        let configuration =
            memory_efficient_wgpu_configuration_for(Some(eframe::wgpu::Backends::DX12));
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = configuration.wgpu_setup else {
            panic!("configuration should create a WGPU device")
        };
        assert_eq!(
            setup.instance_descriptor.backends,
            eframe::wgpu::Backends::DX12
        );
    }
}
