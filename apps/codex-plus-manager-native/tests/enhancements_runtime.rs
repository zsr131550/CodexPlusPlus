use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::settings::{BackendSettings, LaunchMode};
use codex_plus_manager_native::runtime::DispatchError;
use codex_plus_manager_native::runtime::enhancements::{
    EnhancementDispatcher, EnhancementResponse,
};
use codex_plus_manager_service::{
    EnhancementError, EnhancementErrorKind, EnhancementSettingsEnvironment,
    EnhancementSettingsService, EnhancementSettingsSource, EnhancementWorkspace, ResetEnhancements,
    SaveEnhancements,
};

#[derive(Clone)]
struct StaticEnvironment(BackendSettings);

impl EnhancementSettingsEnvironment for StaticEnvironment {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.0.clone())
    }

    fn update_enhancement_settings_if<F>(
        &self,
        _payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        Ok(predicate(&self.0).then(|| self.0.clone()))
    }
}

struct BlockingSource {
    calls: Arc<Mutex<Vec<&'static str>>>,
    workspaces: Mutex<VecDeque<Arc<EnhancementWorkspace>>>,
    first_started: mpsc::Sender<()>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl BlockingSource {
    fn workspace(&self, call: &'static str) -> EnhancementWorkspace {
        self.calls.lock().unwrap().push(call);
        (*self.workspaces.lock().unwrap().pop_front().unwrap()).clone()
    }
}

impl EnhancementSettingsSource for BlockingSource {
    fn load(&self) -> Result<EnhancementWorkspace, EnhancementError> {
        let first = self.calls.lock().unwrap().is_empty();
        if first {
            self.first_started.send(()).unwrap();
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(self.workspace("load"))
    }

    fn save(&self, _request: SaveEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
        Ok(self.workspace("save"))
    }

    fn reset(&self, _request: ResetEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
        Ok(self.workspace("reset"))
    }
}

#[test]
fn runtime_coalesces_only_adjacent_loads_and_keeps_mutations_fifo() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(BlockingSource {
        calls: Arc::clone(&calls),
        workspaces: Mutex::new((1..=5).map(workspace).collect()),
        first_started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = EnhancementDispatcher::spawn(source, Arc::new(|| {}));
    let request_workspace = workspace(20);

    dispatcher.request_load(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_load(2).unwrap();
    dispatcher.request_load(3).unwrap();
    dispatcher
        .request_save(
            4,
            SaveEnhancements {
                expected_revision: request_workspace.revision,
                settings: request_workspace.settings,
            },
        )
        .unwrap();
    dispatcher.request_load(5).unwrap();
    dispatcher.request_load(6).unwrap();
    dispatcher
        .request_reset(
            7,
            ResetEnhancements {
                expected_revision: request_workspace.revision,
                confirmed: true,
            },
        )
        .unwrap();
    release_tx.send(()).unwrap();

    let responses = (0..5).map(|_| receive(&dispatcher)).collect::<Vec<_>>();
    assert_eq!(
        responses
            .iter()
            .map(EnhancementResponse::request_id)
            .collect::<Vec<_>>(),
        [1, 3, 4, 6, 7]
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["load", "load", "save", "load", "reset"]
    );
    assert!(matches!(responses[2], EnhancementResponse::Saved { .. }));
    assert!(matches!(responses[4], EnhancementResponse::Reset { .. }));
}

struct FailingSource;

impl EnhancementSettingsSource for FailingSource {
    fn load(&self) -> Result<EnhancementWorkspace, EnhancementError> {
        Err(EnhancementError::new(
            EnhancementErrorKind::SettingsReadFailed,
        ))
    }

    fn save(&self, _request: SaveEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
        panic!("not used")
    }

    fn reset(&self, _request: ResetEnhancements) -> Result<EnhancementWorkspace, EnhancementError> {
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
fn runtime_logs_only_safe_failure_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let _guard = DiagnosticLogGuard;
    let dispatcher = EnhancementDispatcher::spawn(Arc::new(FailingSource), Arc::new(|| {}));

    dispatcher.request_load(41).unwrap();
    assert!(matches!(
        receive(&dispatcher),
        EnhancementResponse::Loaded { .. }
    ));

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(log.contains("native.enhancements.load"));
    assert!(log.contains("SettingsReadFailed"));
    assert!(log.contains("request_id"));
    assert!(!log.contains("private-adjacent-key"));
    assert!(!log.contains("C:/private"));
}

#[test]
fn runtime_disconnect_maps_to_worker_stopped() {
    struct PanicSource;
    impl EnhancementSettingsSource for PanicSource {
        fn load(&self) -> Result<EnhancementWorkspace, EnhancementError> {
            panic!("intentional enhancement worker exit")
        }

        fn save(
            &self,
            _request: SaveEnhancements,
        ) -> Result<EnhancementWorkspace, EnhancementError> {
            panic!("not used")
        }

        fn reset(
            &self,
            _request: ResetEnhancements,
        ) -> Result<EnhancementWorkspace, EnhancementError> {
            panic!("not used")
        }
    }

    let dispatcher = EnhancementDispatcher::spawn(Arc::new(PanicSource), Arc::new(|| {}));
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

fn receive(dispatcher: &EnhancementDispatcher) -> EnhancementResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for enhancement response"),
            Err(error) => panic!("enhancement worker stopped: {error:?}"),
        }
    }
}

fn workspace(seed: u8) -> Arc<EnhancementWorkspace> {
    let settings = BackendSettings {
        enhancements_enabled: seed.is_multiple_of(2),
        launch_mode: if seed.is_multiple_of(2) {
            LaunchMode::Relay
        } else {
            LaunchMode::Patch
        },
        codex_app_fast_startup: seed.is_multiple_of(3),
        relay_api_key: "private-adjacent-key".to_owned(),
        ..BackendSettings::default()
    };
    Arc::new(
        EnhancementSettingsService::new(StaticEnvironment(settings))
            .load()
            .unwrap(),
    )
}
