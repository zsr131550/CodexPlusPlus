use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::runtime::DispatchError;
use codex_plus_manager_native::runtime::maintenance::{MaintenanceDispatcher, MaintenanceResponse};
use codex_plus_manager_service::{
    LaunchCodex, LaunchOutcome, LoadMaintenance, MaintenanceError, MaintenanceErrorKind,
    MaintenanceSource, MaintenanceWorkspace, PrivatePath, SaveCodexAppPath,
};

mod common;

use common::maintenance_workspace;

struct DiagnosticLogGuard;

impl Drop for DiagnosticLogGuard {
    fn drop(&mut self) {
        codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    }
}

fn diagnostic_log_guard(path: std::path::PathBuf) -> DiagnosticLogGuard {
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(path));
    DiagnosticLogGuard
}

struct BlockingMaintenanceSource {
    calls: Arc<Mutex<Vec<String>>>,
    workspaces: Mutex<VecDeque<Arc<MaintenanceWorkspace>>>,
    first_started: mpsc::Sender<()>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl MaintenanceSource for BlockingMaintenanceSource {
    fn load_workspace(
        &self,
        _request: LoadMaintenance,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        let call_number = {
            let mut calls = self.calls.lock().unwrap();
            calls.push("load".to_owned());
            calls.len()
        };
        if call_number == 1 {
            self.first_started.send(()).unwrap();
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok((*self.workspaces.lock().unwrap().pop_front().unwrap()).clone())
    }

    fn load_logs(
        &self,
        _requested_lines: usize,
    ) -> Result<codex_plus_manager_service::SafeLogDocument, MaintenanceError> {
        panic!("not used")
    }

    fn build_diagnostics(
        &self,
    ) -> Result<codex_plus_manager_service::SafeDiagnosticDocument, MaintenanceError> {
        panic!("not used")
    }

    fn save_app_path(
        &self,
        _request: SaveCodexAppPath,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        self.calls.lock().unwrap().push("save_path".to_owned());
        Ok((*maintenance_workspace("C:/fixture/Saved")).clone())
    }

    fn launch(&self, _request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError> {
        self.calls.lock().unwrap().push("launch".to_owned());
        Ok(LaunchOutcome {
            debug_port: 9229,
            helper_port: 57321,
            accepted: true,
        })
    }
}

struct FailingMaintenanceSource;

impl MaintenanceSource for FailingMaintenanceSource {
    fn load_workspace(
        &self,
        _request: LoadMaintenance,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        Err(MaintenanceError::new(
            MaintenanceErrorKind::LogReadFailed,
            None,
        ))
    }

    fn load_logs(
        &self,
        _requested_lines: usize,
    ) -> Result<codex_plus_manager_service::SafeLogDocument, MaintenanceError> {
        panic!("not used")
    }

    fn build_diagnostics(
        &self,
    ) -> Result<codex_plus_manager_service::SafeDiagnosticDocument, MaintenanceError> {
        panic!("not used")
    }

    fn save_app_path(
        &self,
        _request: SaveCodexAppPath,
    ) -> Result<MaintenanceWorkspace, MaintenanceError> {
        panic!("not used")
    }

    fn launch(&self, _request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError> {
        panic!("not used")
    }
}

fn receive(dispatcher: &MaintenanceDispatcher) -> MaintenanceResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for maintenance response"),
            Err(error) => panic!("maintenance worker stopped: {error:?}"),
        }
    }
}

#[test]
fn maintenance_runtime_coalesces_adjacent_loads_and_keeps_mutations_fifo() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(BlockingMaintenanceSource {
        calls: Arc::clone(&calls),
        workspaces: Mutex::new(VecDeque::from([
            maintenance_workspace("C:/fixture/First"),
            maintenance_workspace("C:/fixture/Second"),
            maintenance_workspace("C:/fixture/Third"),
        ])),
        first_started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = MaintenanceDispatcher::spawn(source, Arc::new(|| {}));

    dispatcher.request_load(1, 50).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_load(2, 50).unwrap();
    dispatcher.request_load(3, 100).unwrap();
    release_tx.send(()).unwrap();
    let first = receive(&dispatcher);
    assert_eq!(first.request_id(), 1);
    let first_workspace = match &first {
        MaintenanceResponse::Loaded { result, .. } => Arc::clone(result.as_ref().unwrap()),
        _ => panic!("expected load"),
    };
    let save_request = SaveCodexAppPath {
        expected_revision: first_workspace.app_path.as_ref().unwrap().revision,
        path: PrivatePath::new("C:/fixture/Saved"),
        confirmed_clear: false,
    };
    dispatcher.request_save(4, save_request).unwrap();
    dispatcher.request_load(5, 50).unwrap();
    dispatcher.request_load(6, 200).unwrap();
    dispatcher
        .request_launch(
            7,
            LaunchCodex::native(PrivatePath::new("C:/fixture/Saved"), 9229, 57321),
        )
        .unwrap();

    let rest = [
        receive(&dispatcher),
        receive(&dispatcher),
        receive(&dispatcher),
        receive(&dispatcher),
    ];
    assert_eq!(rest.map(|response| response.request_id()), [3, 4, 6, 7]);
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["load", "load", "save_path", "load", "launch"]
    );
}

#[test]
fn maintenance_runtime_logs_only_safe_failure_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    let _guard = diagnostic_log_guard(log_path.clone());
    let dispatcher =
        MaintenanceDispatcher::spawn(Arc::new(FailingMaintenanceSource), Arc::new(|| {}));

    dispatcher.request_load(41, 100).unwrap();
    let response = receive(&dispatcher);
    assert!(matches!(response, MaintenanceResponse::Loaded { .. }));

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(log.contains("native.maintenance.load"));
    assert!(log.contains("request_id"));
    assert!(log.contains("LogReadFailed"));
    assert!(!log.contains("C:/"));
    assert!(!log.contains("private-key"));
}

#[test]
fn maintenance_runtime_disconnect_maps_to_worker_stopped() {
    struct PanicSource;
    impl MaintenanceSource for PanicSource {
        fn load_workspace(
            &self,
            _request: LoadMaintenance,
        ) -> Result<MaintenanceWorkspace, MaintenanceError> {
            panic!("intentional maintenance worker exit")
        }
        fn load_logs(
            &self,
            _requested_lines: usize,
        ) -> Result<codex_plus_manager_service::SafeLogDocument, MaintenanceError> {
            panic!("not used")
        }
        fn build_diagnostics(
            &self,
        ) -> Result<codex_plus_manager_service::SafeDiagnosticDocument, MaintenanceError> {
            panic!("not used")
        }
        fn save_app_path(
            &self,
            _request: SaveCodexAppPath,
        ) -> Result<MaintenanceWorkspace, MaintenanceError> {
            panic!("not used")
        }
        fn launch(&self, _request: LaunchCodex) -> Result<LaunchOutcome, MaintenanceError> {
            panic!("not used")
        }
    }
    let dispatcher = MaintenanceDispatcher::spawn(Arc::new(PanicSource), Arc::new(|| {}));
    dispatcher.request_load(1, 50).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Err(DispatchError::WorkerStopped) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            other => panic!("expected worker stop, got {other:?}"),
        }
    }
}
