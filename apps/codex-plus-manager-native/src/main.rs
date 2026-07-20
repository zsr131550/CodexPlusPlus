#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use codex_plus_core::desktop_integration::{
    DesktopIntegrationItemKind, DesktopRepairOperation, ShortcutSnapshot, WindowsDesktopSnapshot,
};
use codex_plus_core::startup_registration::{
    CANONICAL_RUN_NAME, LEGACY_RUN_NAME, OwnedStringValueSnapshot, StartupRegistrationOperation,
    StartupRegistrationSnapshot, canonical_startup_command,
};
use codex_plus_core::update::current_update_target;
use codex_plus_manager_native::app::{NativeManagerApp, NativeManagerSources};
use codex_plus_manager_native::desktop_host::{
    APP_ID, APP_TITLE, DesktopHostBootstrap, DesktopHostRuntime, NativePersistencePaths,
};
use codex_plus_manager_native::fonts;
use codex_plus_manager_native::i18n::Locale;
use codex_plus_manager_native::path_picker::path_picker_from_environment;
use codex_plus_manager_native::perf::PerfRecorder;
use codex_plus_manager_service::{
    ContextToolsService, DesktopIntegrationEnvironment, DesktopIntegrationEnvironmentError,
    DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationService, DesktopIntegrationSource,
    DesktopStartupArgs, EnhancementSettingsService, MaintenanceService, ManagerSettingsService,
    OverviewService, PluginMarketplaceService, ProviderImportService, ProviderService,
    ProviderSyncService, RelayEnvironmentService, SessionService,
    SystemDesktopIntegrationEnvironment, SystemProviderEnvironment, SystemUpdateEnvironment,
    UpdateDownload, UpdateEnvironment, UpdateEnvironmentError, UpdateEnvironmentErrorKind,
    UpdateService, UpdateSource, UserScriptService, ZedRemoteService,
};
use eframe::egui;

const MEBIBYTE: u64 = 1024 * 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    configure_diagnostic_log_from_env();
    let process_started = Instant::now();
    let persistence_paths = NativePersistencePaths::for_state_override(native_state_override());
    let update_fixture = update_fixture_paths_from_environment()?;
    let desktop_integration_source = desktop_integration_source_from_environment()?;
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
            let update_service = update_source(
                persistence_paths.state_root().join("updates"),
                update_fixture.clone(),
            )?;
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
                    desktop_integration: Arc::clone(&desktop_integration_source),
                    settings: manager_settings_service,
                    update: update_service,
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

const DESKTOP_INTEGRATION_FIXTURE_STATE_ENV: &str =
    "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_FIXTURE_STATE";
const DESKTOP_INTEGRATION_RECORD_PATH_ENV: &str =
    "CODEX_PLUS_NATIVE_DESKTOP_INTEGRATION_RECORD_PATH";
const WINDOWS_NEEDS_REPAIR_LEGACY_FIXTURE: &str = "windows_needs_repair_legacy";

#[derive(Clone)]
struct DesktopIntegrationFixtureConfig {
    record_path: PathBuf,
}

fn desktop_integration_source_from_environment()
-> std::io::Result<Arc<dyn DesktopIntegrationSource>> {
    let fixture = resolve_desktop_integration_fixture(
        fixture_string(DESKTOP_INTEGRATION_FIXTURE_STATE_ENV)?,
        fixture_path(DESKTOP_INTEGRATION_RECORD_PATH_ENV),
    )?;
    if let Some(config) = fixture {
        Ok(Arc::new(DesktopIntegrationService::new(
            FixtureDesktopIntegrationEnvironment::new(config),
        )))
    } else {
        Ok(Arc::new(DesktopIntegrationService::new(
            SystemDesktopIntegrationEnvironment::new(),
        )))
    }
}

fn fixture_string(name: &str) -> std::io::Result<Option<String>> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.into_string().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{name} must contain valid Unicode"),
                )
            })
        })
        .transpose()
}

fn resolve_desktop_integration_fixture(
    state: Option<String>,
    record_path: Option<PathBuf>,
) -> std::io::Result<Option<DesktopIntegrationFixtureConfig>> {
    match (state.as_deref(), record_path) {
        (None, None) => Ok(None),
        (Some(WINDOWS_NEEDS_REPAIR_LEGACY_FIXTURE), Some(record_path)) => {
            Ok(Some(DesktopIntegrationFixtureConfig { record_path }))
        }
        (Some(_), Some(_)) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native desktop-integration fixture state is unsupported",
        )),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native desktop-integration fixture requires both state and record path",
        )),
    }
}

#[derive(Clone)]
struct FixtureDesktopIntegrationEnvironment {
    state: Arc<Mutex<FixtureDesktopIntegrationState>>,
}

struct FixtureDesktopIntegrationState {
    snapshot: DesktopIntegrationEnvironmentSnapshot,
    record_path: PathBuf,
}

impl FixtureDesktopIntegrationEnvironment {
    fn new(config: DesktopIntegrationFixtureConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(FixtureDesktopIntegrationState {
                snapshot: windows_needs_repair_legacy_snapshot(),
                record_path: config.record_path,
            })),
        }
    }
}

impl DesktopIntegrationEnvironment for FixtureDesktopIntegrationEnvironment {
    fn inspect_desktop_integration(
        &self,
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError> {
        self.state
            .lock()
            .map(|state| state.snapshot.clone())
            .map_err(|_| DesktopIntegrationEnvironmentError::InspectFailed)
    }

    fn apply_desktop_repair_operation(
        &self,
        operation: &DesktopRepairOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)?;
        let record_path = state.record_path.clone();
        let DesktopIntegrationEnvironmentSnapshot::Windows { repair, .. } = &mut state.snapshot
        else {
            return Err(DesktopIntegrationEnvironmentError::EffectFailed);
        };
        match operation {
            DesktopRepairOperation::WriteShortcut {
                kind,
                target,
                arguments,
                ..
            } => {
                let slot = match kind {
                    DesktopIntegrationItemKind::DesktopManagerShortcut => {
                        &mut repair.desktop_manager
                    }
                    DesktopIntegrationItemKind::StartMenuLauncherShortcut => {
                        &mut repair.start_menu_launcher
                    }
                    DesktopIntegrationItemKind::StartMenuManagerShortcut => {
                        &mut repair.start_menu_manager
                    }
                    _ => return Err(DesktopIntegrationEnvironmentError::EffectFailed),
                };
                record_desktop_integration_operation(
                    &record_path,
                    &format!("repair:{}", kind.as_str()),
                )?;
                *slot = Some(ShortcutSnapshot {
                    target: target.clone(),
                    arguments: arguments.clone(),
                });
            }
            DesktopRepairOperation::WriteProtocol { command, .. } => {
                record_desktop_integration_operation(&record_path, "repair:url_protocol")?;
                repair.protocol_command = Some(command.clone());
            }
            DesktopRepairOperation::RegisterMacosBundle { .. } => {
                return Err(DesktopIntegrationEnvironmentError::EffectFailed);
            }
        }
        Ok(())
    }

    fn apply_startup_registration_operation(
        &self,
        operation: &StartupRegistrationOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)?;
        let record_path = state.record_path.clone();
        let DesktopIntegrationEnvironmentSnapshot::Windows { sign_in, .. } = &mut state.snapshot
        else {
            return Err(DesktopIntegrationEnvironmentError::EffectFailed);
        };
        match operation {
            StartupRegistrationOperation::SetRunValue { name, value }
                if *name == CANONICAL_RUN_NAME =>
            {
                record_desktop_integration_operation(&record_path, "startup:set_canonical")?;
                sign_in.canonical_run = OwnedStringValueSnapshot::String(value.clone());
            }
            StartupRegistrationOperation::DeleteRunValue { name }
                if *name == CANONICAL_RUN_NAME =>
            {
                record_desktop_integration_operation(&record_path, "startup:delete_canonical")?;
                sign_in.canonical_run = OwnedStringValueSnapshot::Absent;
            }
            StartupRegistrationOperation::DeleteRunValue { name } if *name == LEGACY_RUN_NAME => {
                record_desktop_integration_operation(&record_path, "startup:delete_legacy_run")?;
                sign_in.legacy_run = OwnedStringValueSnapshot::Absent;
            }
            StartupRegistrationOperation::DeleteLegacyStartupShortcut => {
                record_desktop_integration_operation(
                    &record_path,
                    "startup:delete_legacy_shortcut",
                )?;
                sign_in.legacy_shortcut = None;
            }
            _ => return Err(DesktopIntegrationEnvironmentError::EffectFailed),
        }
        Ok(())
    }
}

fn windows_needs_repair_legacy_snapshot() -> DesktopIntegrationEnvironmentSnapshot {
    let manager_path = PathBuf::from("/fixture/codex-plus-plus-manager.exe");
    let launcher_path = PathBuf::from("/fixture/codex-plus-plus.exe");
    DesktopIntegrationEnvironmentSnapshot::Windows {
        repair: Box::new(WindowsDesktopSnapshot {
            current_exe: manager_path,
            launcher_is_file: true,
            desktop_dir: Some(PathBuf::from("/fixture/Desktop")),
            programs_dir: Some(PathBuf::from("/fixture/Programs")),
            desktop_manager: None,
            start_menu_launcher: None,
            start_menu_manager: None,
            protocol_command: None,
        }),
        sign_in: StartupRegistrationSnapshot {
            launcher_is_file: true,
            canonical_run: OwnedStringValueSnapshot::Absent,
            legacy_run: OwnedStringValueSnapshot::String(format!(
                "{} --debug-port 9229",
                canonical_startup_command(&launcher_path)
            )),
            legacy_shortcut: None,
            launcher_path,
        },
    }
}

fn record_desktop_integration_operation(
    path: &Path,
    operation: &str,
) -> Result<(), DesktopIntegrationEnvironmentError> {
    let mut record = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)?;
    writeln!(record, "{operation}").map_err(|_| DesktopIntegrationEnvironmentError::EffectFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_plus_core::desktop_integration::DesktopIntegrationHealth;
    use codex_plus_core::startup_registration::StartAtSignInHealth;
    use codex_plus_manager_service::{
        MigrateStartAtSignIn, RepairDesktopIntegration, SetStartAtSignIn,
    };

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

    #[test]
    fn update_fixture_requires_all_paths_and_never_partially_falls_back() {
        assert!(
            resolve_update_fixture_paths(None, None, None, None)
                .unwrap()
                .is_none()
        );
        let paths = resolve_update_fixture_paths(
            Some(PathBuf::from("metadata.json")),
            Some(PathBuf::from("asset.exe")),
            Some(PathBuf::from("launch.record")),
            Some(PathBuf::from("check.record")),
        )
        .unwrap()
        .unwrap();
        assert_eq!(paths.asset, PathBuf::from("asset.exe"));
        assert!(
            resolve_update_fixture_paths(Some(PathBuf::from("metadata.json")), None, None, None,)
                .is_err()
        );
    }

    #[test]
    fn desktop_integration_fixture_requires_all_configuration() {
        assert!(
            resolve_desktop_integration_fixture(None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            resolve_desktop_integration_fixture(
                Some(WINDOWS_NEEDS_REPAIR_LEGACY_FIXTURE.to_owned()),
                None,
            )
            .is_err()
        );
        assert!(
            resolve_desktop_integration_fixture(
                None,
                Some(PathBuf::from("desktop-integration.record")),
            )
            .is_err()
        );
        assert!(
            resolve_desktop_integration_fixture(
                Some("unknown".to_owned()),
                Some(PathBuf::from("desktop-integration.record")),
            )
            .is_err()
        );
    }

    #[test]
    fn desktop_integration_fixture_records_the_complete_bounded_workflow() {
        let temp = tempfile::tempdir().unwrap();
        let record_path = temp.path().join("desktop-integration.record");
        let config = resolve_desktop_integration_fixture(
            Some(WINDOWS_NEEDS_REPAIR_LEGACY_FIXTURE.to_owned()),
            Some(record_path.clone()),
        )
        .unwrap()
        .unwrap();
        let source =
            DesktopIntegrationService::new(FixtureDesktopIntegrationEnvironment::new(config));

        let initial = source.inspect().unwrap();
        assert_eq!(initial.repair_health, DesktopIntegrationHealth::NeedsRepair);
        assert_eq!(
            initial.sign_in.unwrap().health,
            StartAtSignInHealth::NeedsMigration
        );
        assert!(!record_path.exists());

        let repaired = source
            .repair(RepairDesktopIntegration {
                expected_revision: initial.revision,
                confirmed: true,
            })
            .unwrap();
        assert_eq!(repaired.applied_operation_count, 4);
        assert_eq!(
            repaired.workspace.repair_health,
            DesktopIntegrationHealth::Current
        );

        let migrated = source
            .migrate_sign_in(MigrateStartAtSignIn {
                expected_revision: repaired.workspace.revision,
            })
            .unwrap();
        assert_eq!(migrated.applied_operation_count, 2);
        assert_eq!(
            migrated.workspace.sign_in.unwrap().health,
            StartAtSignInHealth::Enabled
        );

        let disabled = source
            .set_start_at_sign_in(SetStartAtSignIn {
                expected_revision: migrated.workspace.revision,
                enabled: false,
            })
            .unwrap();
        assert_eq!(disabled.applied_operation_count, 1);
        assert_eq!(
            disabled.workspace.sign_in.unwrap().health,
            StartAtSignInHealth::Disabled
        );

        let enabled = source
            .set_start_at_sign_in(SetStartAtSignIn {
                expected_revision: disabled.workspace.revision,
                enabled: true,
            })
            .unwrap();
        assert_eq!(enabled.applied_operation_count, 1);
        assert_eq!(
            enabled.workspace.sign_in.unwrap().health,
            StartAtSignInHealth::Enabled
        );

        assert_eq!(
            std::fs::read_to_string(record_path).unwrap(),
            concat!(
                "repair:desktop_manager_shortcut\n",
                "repair:start_menu_launcher_shortcut\n",
                "repair:start_menu_manager_shortcut\n",
                "repair:url_protocol\n",
                "startup:set_canonical\n",
                "startup:delete_legacy_run\n",
                "startup:delete_canonical\n",
                "startup:set_canonical\n",
            )
        );
    }
}

#[derive(Clone)]
struct UpdateFixturePaths {
    metadata: PathBuf,
    asset: PathBuf,
    launch_record: PathBuf,
    check_record: PathBuf,
}

fn update_fixture_paths_from_environment() -> std::io::Result<Option<UpdateFixturePaths>> {
    resolve_update_fixture_paths(
        fixture_path("CODEX_PLUS_NATIVE_UPDATE_METADATA_PATH"),
        fixture_path("CODEX_PLUS_NATIVE_UPDATE_ASSET_PATH"),
        fixture_path("CODEX_PLUS_NATIVE_UPDATE_LAUNCH_RECORD_PATH"),
        fixture_path("CODEX_PLUS_NATIVE_UPDATE_CHECK_RECORD_PATH"),
    )
}

fn fixture_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn resolve_update_fixture_paths(
    metadata: Option<PathBuf>,
    asset: Option<PathBuf>,
    launch_record: Option<PathBuf>,
    check_record: Option<PathBuf>,
) -> std::io::Result<Option<UpdateFixturePaths>> {
    match (metadata, asset, launch_record, check_record) {
        (None, None, None, None) => Ok(None),
        (Some(metadata), Some(asset), Some(launch_record), Some(check_record)) => {
            Ok(Some(UpdateFixturePaths {
                metadata,
                asset,
                launch_record,
                check_record,
            }))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native update fixture requires metadata, asset, launch, and check paths",
        )),
    }
}

fn update_source(
    update_root: PathBuf,
    fixture: Option<UpdateFixturePaths>,
) -> Result<Arc<dyn UpdateSource>, UpdateEnvironmentError> {
    if let Some(paths) = fixture {
        Ok(Arc::new(UpdateService::new(FixtureUpdateEnvironment {
            paths,
        })))
    } else {
        Ok(Arc::new(UpdateService::new(SystemUpdateEnvironment::new(
            update_root,
        )?)))
    }
}

struct FixtureUpdateEnvironment {
    paths: UpdateFixturePaths,
}

impl UpdateEnvironment for FixtureUpdateEnvironment {
    type Artifact = Vec<u8>;

    fn current_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    fn target(&self) -> codex_plus_core::update::UpdateTarget {
        current_update_target()
    }

    fn fetch_release_metadata(
        &self,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        let mut check_record = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.check_record)
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        check_record
            .write_all(b"check\n")
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        let file = File::open(&self.paths.metadata)
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Transport, error))?;
        let mut bytes = Vec::new();
        file.take(maximum_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Transport, error))?;
        Ok(bytes)
    }

    fn open_asset_download(&self, url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        let file = File::open(&self.paths.asset)
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Transport, error))?;
        let length = file
            .metadata()
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Transport, error))?
            .len();
        Ok(UpdateDownload::new(
            url.to_owned(),
            Some(length),
            FixtureAssetChunks { file },
        ))
    }

    fn create_update_artifact(
        &self,
        _safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        Ok(Vec::new())
    }

    fn publish_update_artifact(
        &self,
        _artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        Ok(())
    }

    fn cleanup_update_artifact(&self, artifact: &mut Self::Artifact) {
        artifact.clear();
    }

    fn launch_update_artifact(
        &self,
        artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        let mut record = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.paths.launch_record)
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Launcher, error))?;
        writeln!(record, "launched_bytes={}", artifact.len())
            .map_err(|error| fixture_update_error(UpdateEnvironmentErrorKind::Launcher, error))
    }
}

struct FixtureAssetChunks {
    file: File,
}

impl Iterator for FixtureAssetChunks {
    type Item = Result<Vec<u8>, UpdateEnvironmentError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut bytes = vec![0_u8; 8 * 1024];
        match self.file.read(&mut bytes) {
            Ok(0) => None,
            Ok(length) => {
                bytes.truncate(length);
                Some(Ok(bytes))
            }
            Err(error) => Some(Err(fixture_update_error(
                UpdateEnvironmentErrorKind::Transport,
                error,
            ))),
        }
    }
}

fn fixture_update_error(
    kind: UpdateEnvironmentErrorKind,
    error: impl std::fmt::Display,
) -> UpdateEnvironmentError {
    UpdateEnvironmentError::new(kind, error.to_string())
}
