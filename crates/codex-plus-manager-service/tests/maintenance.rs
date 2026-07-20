use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use codex_plus_core::install::{EntryPointState, ShortcutState};
use codex_plus_core::settings::BackendSettings;
use codex_plus_core::status::LaunchStatus;
use codex_plus_manager_service::{
    CodexLaunchPlan, DiagnosticPathPresence, LaunchCodex, LoadMaintenance, MaintenanceEnvironment,
    MaintenanceErrorKind, MaintenanceService, PathKind, PrivatePath, SaveCodexAppPath,
};
use serde_json::json;

const SECRET: &str = "secret-key-sentinel";
const PRIVATE_PATH: &str = "C:/private/path-sentinel";
const PRIVATE_URL: &str = "https://private.invalid/body-sentinel";
const EXISTING_APP_DIR: &str = "C:/apps/codex";

#[derive(Clone)]
struct MaintenanceFixture {
    environment: FakeMaintenanceEnvironment,
}

impl MaintenanceFixture {
    fn with_private_sentinels() -> Self {
        let settings = BackendSettings {
            codex_app_path: PRIVATE_PATH.to_owned(),
            codex_extra_args: vec![format!("--private={SECRET}")],
            codex_app_stepwise_enabled: true,
            codex_app_stepwise_base_url: PRIVATE_URL.to_owned(),
            codex_app_stepwise_api_key: SECRET.to_owned(),
            codex_app_stepwise_model: format!("private-model-{SECRET}"),
            codex_app_image_overlay_enabled: true,
            codex_app_image_overlay_path: PRIVATE_PATH.to_owned(),
            ..BackendSettings::default()
        };

        Self {
            environment: FakeMaintenanceEnvironment::new(settings, fixture_log_bytes()),
        }
    }

    fn with_unknown_settings() -> Self {
        let settings = BackendSettings {
            codex_app_path: "C:/apps/original".to_owned(),
            ..BackendSettings::default()
        };
        let environment = FakeMaintenanceEnvironment::new(settings, fixture_log_bytes());
        environment.set_path_kind(EXISTING_APP_DIR, PathKind::Directory);
        Self { environment }
    }

    fn with_log_failure_and_recording_launcher() -> Self {
        let fixture = Self::with_private_sentinels();
        fixture.environment.state().lock().unwrap().log_failed = true;
        fixture
            .environment
            .set_path_kind(EXISTING_APP_DIR, PathKind::Directory);
        fixture
    }

    fn service(&self) -> MaintenanceService<FakeMaintenanceEnvironment> {
        MaintenanceService::new(self.environment.clone())
    }

    fn private_sentinels(&self) -> [&'static str; 4] {
        [SECRET, "path-sentinel", "private.invalid", "body-sentinel"]
    }

    fn mutate_unrelated_setting(&self) {
        self.environment
            .state()
            .lock()
            .unwrap()
            .settings
            .provider_sync_enabled = true;
    }

    fn replace_app_path_out_of_band(&self) {
        self.environment
            .state()
            .lock()
            .unwrap()
            .settings
            .codex_app_path = "C:/apps/out-of-band".to_owned();
    }

    fn assert_unknown_setting_preserved(&self) {
        assert!(self.environment.state().lock().unwrap().unknown_setting);
    }

    fn assert_launch_count(&self, expected: usize) {
        assert_eq!(
            self.environment.state().lock().unwrap().launches.len(),
            expected
        );
    }
}

#[derive(Clone)]
struct FakeMaintenanceEnvironment {
    state: Arc<Mutex<FakeMaintenanceState>>,
}

struct FakeMaintenanceState {
    settings: BackendSettings,
    log_tail: Vec<u8>,
    path_kinds: HashMap<PathBuf, PathKind>,
    settings_read_failed: bool,
    settings_write_failed: bool,
    entrypoint_failed: bool,
    status_failed: bool,
    log_failed: bool,
    launch_failed: bool,
    unknown_setting: bool,
    successful_writes: usize,
    payloads: Vec<serde_json::Value>,
    launches: Vec<LaunchEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LaunchEvidence {
    debug_port: u16,
    helper_port: u16,
    path_configured: bool,
    argument_count: usize,
}

impl FakeMaintenanceEnvironment {
    fn new(settings: BackendSettings, log_tail: Vec<u8>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeMaintenanceState {
                settings,
                log_tail,
                path_kinds: HashMap::new(),
                settings_read_failed: false,
                settings_write_failed: false,
                entrypoint_failed: false,
                status_failed: false,
                log_failed: false,
                launch_failed: false,
                unknown_setting: true,
                successful_writes: 0,
                payloads: Vec::new(),
                launches: Vec::new(),
            })),
        }
    }

    fn state(&self) -> &Arc<Mutex<FakeMaintenanceState>> {
        &self.state
    }

    fn set_path_kind(&self, path: impl Into<PathBuf>, kind: PathKind) {
        self.state
            .lock()
            .unwrap()
            .path_kinds
            .insert(path.into(), kind);
    }
}

impl MaintenanceEnvironment for FakeMaintenanceEnvironment {
    fn load_maintenance_settings(&self) -> anyhow::Result<BackendSettings> {
        let state = self.state.lock().unwrap();
        if state.settings_read_failed {
            anyhow::bail!("private settings failure {SECRET}")
        }
        Ok(state.settings.clone())
    }

    fn update_maintenance_settings_if<F>(
        &self,
        payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        let mut state = self.state.lock().unwrap();
        if state.settings_write_failed {
            anyhow::bail!("private write failure {SECRET}")
        }
        state.payloads.push(payload.clone());
        if !predicate(&state.settings) {
            return Ok(None);
        }
        if let Some(path) = payload
            .get("codexAppPath")
            .and_then(serde_json::Value::as_str)
        {
            state.settings.codex_app_path = path.to_owned();
        }
        state.successful_writes += 1;
        Ok(Some(state.settings.clone()))
    }

    fn inspect_path(&self, path: &Path) -> anyhow::Result<PathKind> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .path_kinds
            .get(path)
            .copied()
            .unwrap_or(PathKind::Missing))
    }

    fn resolve_codex_app(&self, saved: &str) -> Option<PathBuf> {
        (!saved.is_empty()).then(|| PathBuf::from(saved))
    }

    fn codex_app_version(&self, _path: &Path) -> Option<String> {
        Some(format!("private-version-{SECRET}"))
    }

    fn inspect_entrypoints(&self) -> anyhow::Result<EntryPointState> {
        if self.state.lock().unwrap().entrypoint_failed {
            anyhow::bail!("private entrypoint failure {PRIVATE_PATH}")
        }
        Ok(EntryPointState {
            silent_shortcut: ShortcutState {
                installed: true,
                path: Some(PRIVATE_PATH.to_owned()),
            },
            management_shortcut: ShortcutState {
                installed: false,
                path: Some(PRIVATE_PATH.to_owned()),
            },
        })
    }

    fn load_latest_launch(&self) -> anyhow::Result<Option<LaunchStatus>> {
        if self.state.lock().unwrap().status_failed {
            anyhow::bail!("private status failure {PRIVATE_URL}")
        }
        Ok(Some(LaunchStatus {
            status: "running".to_owned(),
            message: PRIVATE_URL.to_owned(),
            started_at_ms: 123,
            debug_port: Some(9229),
            helper_port: Some(57321),
            codex_app: Some(PRIVATE_PATH.to_owned()),
        }))
    }

    fn read_diagnostic_tail(&self, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        let state = self.state.lock().unwrap();
        if state.log_failed {
            anyhow::bail!("private log failure {PRIVATE_PATH}")
        }
        let start = state.log_tail.len().saturating_sub(max_bytes);
        Ok(state.log_tail[start..].to_vec())
    }

    fn diagnostic_path_presence(&self) -> DiagnosticPathPresence {
        DiagnosticPathPresence {
            settings: true,
            logs: true,
            latest_status: true,
        }
    }

    fn launch_codex(&self, plan: &CodexLaunchPlan) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.launches.push(LaunchEvidence {
            debug_port: plan.debug_port(),
            helper_port: plan.helper_port(),
            path_configured: plan.path_configured(),
            argument_count: plan.argument_count(),
        });
        if state.launch_failed {
            anyhow::bail!("private launch failure {PRIVATE_PATH}")
        }
        Ok(())
    }
}

#[test]
fn safe_log_document_is_bounded_allowlisted_and_never_falls_back_to_raw_text() {
    let fixture = MaintenanceFixture::with_private_sentinels();
    let document = fixture.service().load_logs(240).unwrap();

    assert_eq!(document.effective_lines(), 200);
    assert!(document.text().contains("manager.launch_requested"));
    assert!(document.text().contains("renderer.event"));
    for forbidden in fixture.private_sentinels() {
        assert!(!document.text().contains(forbidden));
        assert!(!format!("{document:?}").contains(forbidden));
    }
    assert!(document.dropped_lines() >= 3);
    assert!(!document.text().contains("arbitrary.renderer.secret"));
}

#[test]
fn safe_log_format_has_stable_field_order_and_newlines() {
    let environment = FakeMaintenanceEnvironment::new(
        BackendSettings::default(),
        format!(
            "{}\n",
            json!({
                "timestamp_ms": 1,
                "pid": 999,
                "event": "manager.launch_requested",
                "detail": {
                    "helper_port": 57321,
                    "debug_port": 9229,
                    "path": PRIVATE_PATH,
                }
            })
        )
        .into_bytes(),
    );

    let document = MaintenanceService::new(environment).load_logs(20).unwrap();

    assert_eq!(
        document.text(),
        "1 info manager.launch_requested debug_port=9229 helper_port=57321\n"
    );
    assert_eq!(document.parsed_lines(), 1);
    assert_eq!(document.dropped_lines(), 0);
}

#[test]
fn diagnostic_report_contains_only_the_typed_allowlist() {
    let fixture = MaintenanceFixture::with_private_sentinels();
    let report = fixture.service().build_diagnostics().unwrap();
    let value: serde_json::Value = serde_json::from_str(report.text()).unwrap();

    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert!(value["configured"]["stepwiseApiKey"].is_boolean());
    assert!(value["counts"]["extraArgs"].is_number());
    assert!(value["paths"]["settingsPresent"].is_boolean());
    assert_eq!(report.text().as_ptr(), report.text().as_ptr());
    for forbidden in fixture.private_sentinels() {
        assert!(!report.text().contains(forbidden));
        assert!(!format!("{report:?}").contains(forbidden));
    }
}

#[test]
fn app_path_save_preserves_unknown_and_unrelated_fields_and_rejects_same_scope_conflict() {
    let fixture = MaintenanceFixture::with_unknown_settings();
    let service = fixture.service();
    let workspace = service
        .load_workspace(LoadMaintenance { log_lines: 20 })
        .unwrap();
    let revision = workspace.app_path.unwrap().revision;
    fixture.mutate_unrelated_setting();

    let saved = service
        .save_app_path(SaveCodexAppPath {
            expected_revision: revision,
            path: PrivatePath::new(EXISTING_APP_DIR),
            confirmed_clear: false,
        })
        .unwrap();
    assert!(saved.app_path.unwrap().configured);
    fixture.assert_unknown_setting_preserved();
    let state = fixture.environment.state().lock().unwrap();
    assert_eq!(state.successful_writes, 1);
    assert_eq!(
        state.payloads.last(),
        Some(&json!({ "codexAppPath": EXISTING_APP_DIR }))
    );
    drop(state);

    assert_eq!(
        service
            .save_app_path(SaveCodexAppPath {
                expected_revision: revision,
                path: PrivatePath::new(EXISTING_APP_DIR),
                confirmed_clear: false,
            })
            .unwrap_err()
            .kind(),
        MaintenanceErrorKind::InvalidRevision
    );

    let stale = service
        .load_workspace(LoadMaintenance { log_lines: 20 })
        .unwrap();
    fixture.replace_app_path_out_of_band();
    let error = service
        .save_app_path(SaveCodexAppPath {
            expected_revision: stale.app_path.unwrap().revision,
            path: PrivatePath::new(EXISTING_APP_DIR),
            confirmed_clear: false,
        })
        .unwrap_err();
    assert_eq!(error.kind(), MaintenanceErrorKind::SettingsConflict);
    assert!(error.refreshed_workspace().is_some());
    assert_eq!(
        fixture
            .environment
            .state()
            .lock()
            .unwrap()
            .successful_writes,
        1
    );
}

#[test]
fn app_path_validation_does_not_consume_revision_and_clear_requires_confirmation() {
    let fixture = MaintenanceFixture::with_unknown_settings();
    let service = fixture.service();
    let workspace = service
        .load_workspace(LoadMaintenance { log_lines: 20 })
        .unwrap();
    let revision = workspace.app_path.unwrap().revision;

    let unconfirmed = service
        .save_app_path(SaveCodexAppPath {
            expected_revision: revision,
            path: PrivatePath::new("  "),
            confirmed_clear: false,
        })
        .unwrap_err();
    assert_eq!(unconfirmed.kind(), MaintenanceErrorKind::InvalidPath);
    assert_eq!(
        fixture
            .environment
            .state()
            .lock()
            .unwrap()
            .successful_writes,
        0
    );

    let cleared = service
        .save_app_path(SaveCodexAppPath {
            expected_revision: revision,
            path: PrivatePath::new(""),
            confirmed_clear: true,
        })
        .unwrap();
    assert!(!cleared.app_path.unwrap().configured);

    let workspace = service
        .load_workspace(LoadMaintenance { log_lines: 20 })
        .unwrap();
    let missing = service
        .save_app_path(SaveCodexAppPath {
            expected_revision: workspace.app_path.unwrap().revision,
            path: PrivatePath::new("C:/missing/app"),
            confirmed_clear: false,
        })
        .unwrap_err();
    assert_eq!(missing.kind(), MaintenanceErrorKind::InvalidPath);
    assert_eq!(
        fixture
            .environment
            .state()
            .lock()
            .unwrap()
            .successful_writes,
        1
    );
}

#[test]
fn partial_workspace_keeps_available_sections_and_launch_is_recorded_once_without_path() {
    let fixture = MaintenanceFixture::with_log_failure_and_recording_launcher();
    let service = fixture.service();
    let workspace = service
        .load_workspace(LoadMaintenance { log_lines: 20 })
        .unwrap();
    assert!(workspace.logs.is_unavailable());
    assert!(workspace.entrypoints.is_available());
    assert!(workspace.latest_launch.is_available());
    assert!(!format!("{workspace:?}").contains(PRIVATE_PATH));
    assert!(!format!("{workspace:?}").contains(PRIVATE_URL));

    let outcome = service
        .launch(LaunchCodex::native(
            PrivatePath::new(EXISTING_APP_DIR),
            9229,
            57321,
        ))
        .unwrap();
    assert_eq!(outcome.debug_port, 9229);
    assert_eq!(outcome.helper_port, 57321);
    assert!(outcome.accepted);
    fixture.assert_launch_count(1);
    assert_eq!(
        fixture.environment.state().lock().unwrap().launches[0],
        LaunchEvidence {
            debug_port: 9229,
            helper_port: 57321,
            path_configured: true,
            argument_count: 6,
        }
    );
}

#[test]
fn native_launch_rejects_invalid_inputs_and_maps_one_executor_failure() {
    let fixture = MaintenanceFixture::with_log_failure_and_recording_launcher();
    let service = fixture.service();

    let invalid_port = service
        .launch(LaunchCodex::native(PrivatePath::new(""), 0, 57321))
        .unwrap_err();
    assert_eq!(invalid_port.kind(), MaintenanceErrorKind::InvalidPort);
    let invalid_path = service
        .launch(LaunchCodex::native(
            PrivatePath::new("C:/missing/app"),
            9229,
            57321,
        ))
        .unwrap_err();
    assert_eq!(invalid_path.kind(), MaintenanceErrorKind::InvalidPath);
    fixture.assert_launch_count(0);

    fixture.environment.state().lock().unwrap().launch_failed = true;
    let failed = service
        .launch(LaunchCodex::native(
            PrivatePath::new(EXISTING_APP_DIR),
            9229,
            57321,
        ))
        .unwrap_err();
    assert_eq!(failed.kind(), MaintenanceErrorKind::LaunchFailed);
    assert!(!format!("{failed:?}").contains(PRIVATE_PATH));
    fixture.assert_launch_count(1);
}

fn fixture_log_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [
        json!({
            "timestamp_ms": 10,
            "pid": 321,
            "event": "manager.launch_requested",
            "detail": {
                "debug_port": 9229,
                "helper_port": 57321,
                "api_key": SECRET,
                "path": PRIVATE_PATH,
                "url": PRIVATE_URL,
                "response_body": PRIVATE_URL,
            }
        }),
        json!({
            "timestamp_ms": 11,
            "event": format!("unknown.{SECRET}"),
            "detail": { "message": PRIVATE_URL }
        }),
        json!({
            "timestamp_ms": 12,
            "event": "renderer.arbitrary.renderer.secret",
            "detail": { "payload": SECRET, "path": PRIVATE_PATH }
        }),
    ] {
        bytes.extend_from_slice(value.to_string().as_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"{not-json}\n");
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    bytes.extend_from_slice(&vec![b'x'; 16 * 1024 + 1]);
    bytes.push(b'\n');
    bytes
}
