use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::runtime::DispatchError;
use codex_plus_manager_native::runtime::settings::{SettingsDispatcher, SettingsResponse};
use codex_plus_manager_service::{
    ManagerSettingsError, ManagerSettingsErrorKind, ManagerSettingsSource,
    ManagerSettingsWorkspace, ResetExtraArgs, ResetImageOverlaySettings, ResetStepwiseSettings,
    SafeSettingsGroup, SaveExtraArgs, SaveImageOverlaySettings, SaveStepwiseSettings,
    StepwiseSecretChange, StepwiseSettingsInput, StepwiseTestOutcome, TestStepwiseSettings,
};

mod common;

use common::manager_settings_workspace;

struct BlockingSettingsSource {
    calls: Arc<Mutex<Vec<&'static str>>>,
    workspaces: Mutex<VecDeque<Arc<ManagerSettingsWorkspace>>>,
    first_started: mpsc::Sender<()>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl BlockingSettingsSource {
    fn workspace(&self, call: &'static str) -> ManagerSettingsWorkspace {
        self.calls.lock().unwrap().push(call);
        (*self.workspaces.lock().unwrap().pop_front().unwrap()).clone()
    }
}

impl ManagerSettingsSource for BlockingSettingsSource {
    fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        let first = self.calls.lock().unwrap().is_empty();
        if first {
            self.first_started.send(()).unwrap();
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(self.workspace("load"))
    }

    fn save_stepwise(
        &self,
        _request: SaveStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("save_stepwise"))
    }

    fn reset_stepwise(
        &self,
        _request: ResetStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("reset_stepwise"))
    }

    fn test_stepwise(
        &self,
        _request: TestStepwiseSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        self.calls.lock().unwrap().push("test_stepwise");
        Ok(StepwiseTestOutcome { item_count: 3 })
    }

    fn save_image_overlay(
        &self,
        _request: SaveImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("save_image"))
    }

    fn reset_image_overlay(
        &self,
        _request: ResetImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("reset_image"))
    }

    fn save_extra_args(
        &self,
        _request: SaveExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("save_args"))
    }

    fn reset_extra_args(
        &self,
        _request: ResetExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Ok(self.workspace("reset_args"))
    }

    fn test_compatibility_settings(
        &self,
        _settings: codex_plus_core::settings::BackendSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        panic!("not used")
    }
}

#[test]
fn settings_runtime_coalesces_only_adjacent_loads_and_keeps_commands_fifo() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(BlockingSettingsSource {
        calls: Arc::clone(&calls),
        workspaces: Mutex::new((1..=6).map(manager_settings_workspace).collect()),
        first_started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = SettingsDispatcher::spawn(source, Arc::new(|| {}));
    let workspace = manager_settings_workspace(20);

    dispatcher.request_load(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_load(2).unwrap();
    dispatcher.request_load(3).unwrap();
    dispatcher
        .request_test_stepwise(4, test_request(&workspace))
        .unwrap();
    dispatcher
        .request_save_stepwise(5, save_stepwise_request(&workspace))
        .unwrap();
    dispatcher
        .request_reset_image(
            6,
            ResetImageOverlaySettings {
                expected_revision: workspace.image_overlay.revision,
                confirmed_group: SafeSettingsGroup::ImageOverlay,
            },
        )
        .unwrap();
    dispatcher
        .request_save_args(
            7,
            SaveExtraArgs {
                expected_revision: workspace.extra_args.revision,
                settings: workspace.extra_args.settings.clone(),
            },
        )
        .unwrap();
    dispatcher.request_load(8).unwrap();
    dispatcher.request_load(9).unwrap();
    release_tx.send(()).unwrap();

    let responses = (0..7).map(|_| receive(&dispatcher)).collect::<Vec<_>>();
    assert_eq!(
        responses
            .iter()
            .map(SettingsResponse::request_id)
            .collect::<Vec<_>>(),
        [1, 3, 4, 5, 6, 7, 9]
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [
            "load",
            "load",
            "test_stepwise",
            "save_stepwise",
            "reset_image",
            "save_args",
            "load",
        ]
    );
    assert!(matches!(
        responses[2],
        SettingsResponse::StepwiseTested { request_id: 4, .. }
    ));
    assert!(matches!(
        responses[4],
        SettingsResponse::ImageReset { request_id: 6, .. }
    ));
}

struct FailingSettingsSource;

impl ManagerSettingsSource for FailingSettingsSource {
    fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        Err(ManagerSettingsError::new(
            ManagerSettingsErrorKind::SettingsReadFailed,
            None,
        ))
    }

    fn save_stepwise(
        &self,
        _request: SaveStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn reset_stepwise(
        &self,
        _request: ResetStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn test_stepwise(
        &self,
        _request: TestStepwiseSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        panic!("not used")
    }
    fn save_image_overlay(
        &self,
        _request: SaveImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn reset_image_overlay(
        &self,
        _request: ResetImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn save_extra_args(
        &self,
        _request: SaveExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn reset_extra_args(
        &self,
        _request: ResetExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        panic!("not used")
    }
    fn test_compatibility_settings(
        &self,
        _settings: codex_plus_core::settings::BackendSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        panic!("not used")
    }
}

struct DiagnosticLogGuard;

impl Drop for DiagnosticLogGuard {
    fn drop(&mut self) {
        codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    }
}

#[test]
fn settings_runtime_logs_only_safe_failure_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let _guard = DiagnosticLogGuard;
    let dispatcher = SettingsDispatcher::spawn(Arc::new(FailingSettingsSource), Arc::new(|| {}));

    dispatcher.request_load(41).unwrap();
    assert!(matches!(
        receive(&dispatcher),
        SettingsResponse::Loaded { .. }
    ));

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(log.contains("native.settings.load"));
    assert!(log.contains("SettingsReadFailed"));
    assert!(log.contains("request_id"));
    assert!(!log.contains("private-key-sentinel"));
    assert!(!log.contains("C:/private"));
}

#[test]
fn settings_runtime_disconnect_maps_to_worker_stopped() {
    struct PanicSource;
    impl ManagerSettingsSource for PanicSource {
        fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("intentional settings worker exit")
        }
        fn save_stepwise(
            &self,
            _request: SaveStepwiseSettings,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn reset_stepwise(
            &self,
            _request: ResetStepwiseSettings,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn test_stepwise(
            &self,
            _request: TestStepwiseSettings,
        ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
            panic!("not used")
        }
        fn save_image_overlay(
            &self,
            _request: SaveImageOverlaySettings,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn reset_image_overlay(
            &self,
            _request: ResetImageOverlaySettings,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn save_extra_args(
            &self,
            _request: SaveExtraArgs,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn reset_extra_args(
            &self,
            _request: ResetExtraArgs,
        ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
            panic!("not used")
        }
        fn test_compatibility_settings(
            &self,
            _settings: codex_plus_core::settings::BackendSettings,
        ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
            panic!("not used")
        }
    }

    let dispatcher = SettingsDispatcher::spawn(Arc::new(PanicSource), Arc::new(|| {}));
    dispatcher.request_load(1).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Err(DispatchError::WorkerStopped) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            other => panic!("expected worker stop, got {other:?}"),
        }
    }
}

fn receive(dispatcher: &SettingsDispatcher) -> SettingsResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for settings response"),
            Err(error) => panic!("settings worker stopped: {error:?}"),
        }
    }
}

fn input(workspace: &ManagerSettingsWorkspace) -> StepwiseSettingsInput {
    let settings = &workspace.stepwise.settings;
    StepwiseSettingsInput {
        enabled: settings.enabled,
        direct_send: settings.direct_send,
        base_url: settings.base_url.clone(),
        api_key_env: settings.api_key_env.clone(),
        model: settings.model.clone(),
        max_items: settings.max_items,
        max_input_chars: settings.max_input_chars,
        max_output_tokens: settings.max_output_tokens,
        timeout_ms: settings.timeout_ms,
    }
}

fn test_request(workspace: &ManagerSettingsWorkspace) -> TestStepwiseSettings {
    TestStepwiseSettings {
        expected_revision: workspace.stepwise.revision,
        settings: input(workspace),
        secret_change: StepwiseSecretChange::Keep,
    }
}

fn save_stepwise_request(workspace: &ManagerSettingsWorkspace) -> SaveStepwiseSettings {
    SaveStepwiseSettings {
        expected_revision: workspace.stepwise.revision,
        settings: input(workspace),
        secret_change: StepwiseSecretChange::Keep,
    }
}
