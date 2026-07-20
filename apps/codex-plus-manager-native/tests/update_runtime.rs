use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::runtime::DispatchError;
use codex_plus_manager_native::runtime::update::{UpdateDispatcher, UpdateResponse};
use codex_plus_manager_service::{
    InstallStarted, InstallUpdate, UpdateAvailability, UpdateCheckResult, UpdateError,
    UpdateErrorKind, UpdateProgress, UpdateProgressSink, UpdateSource,
};

struct BlockingSource {
    calls: Arc<Mutex<Vec<&'static str>>>,
    first_started: mpsc::Sender<()>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl UpdateSource for BlockingSource {
    fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        let first = {
            let mut calls = self.calls.lock().unwrap();
            calls.push("check");
            calls.len() == 1
        };
        if first {
            self.first_started.send(()).unwrap();
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(UpdateCheckResult {
            installed_version: "1.0.0".to_owned(),
            latest_version: "1.0.0".to_owned(),
            summary: "private-summary-sentinel".to_owned(),
            availability: UpdateAvailability::Current,
        })
    }

    fn install(
        &self,
        request: InstallUpdate,
        progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        self.calls.lock().unwrap().push("install");
        progress(UpdateProgress {
            downloaded_bytes: 4,
            total_bytes: Some(10),
        });
        progress(UpdateProgress {
            downloaded_bytes: 10,
            total_bytes: Some(10),
        });
        Ok(InstallStarted {
            version: request.confirmed_version,
        })
    }
}

struct FailingSource;

struct ExitSource {
    exited: Option<mpsc::Sender<()>>,
}

impl Drop for ExitSource {
    fn drop(&mut self) {
        if let Some(exited) = self.exited.take() {
            let _ = exited.send(());
        }
    }
}

impl UpdateSource for ExitSource {
    fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        panic!("idle update source must not be called")
    }

    fn install(
        &self,
        _request: InstallUpdate,
        _progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        panic!("idle update source must not be called")
    }
}

impl UpdateSource for FailingSource {
    fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        Err(UpdateError::new(UpdateErrorKind::MetadataFetchFailed))
    }

    fn install(
        &self,
        _request: InstallUpdate,
        _progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        panic!("not used")
    }
}

fn receive(dispatcher: &UpdateDispatcher) -> UpdateResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for update response"),
            Err(error) => panic!("update worker stopped: {error:?}"),
        }
    }
}

#[test]
fn update_runtime_coalesces_adjacent_checks_and_keeps_install_fifo() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatcher = UpdateDispatcher::spawn(
        Arc::new(BlockingSource {
            calls: Arc::clone(&calls),
            first_started: started_tx,
            release_first: Mutex::new(release_rx),
        }),
        Arc::new(|| {}),
    );

    dispatcher.request_check(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_check(2).unwrap();
    dispatcher.request_check(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert_eq!(receive(&dispatcher).request_id(), 3);
    assert_eq!(calls.lock().unwrap().as_slice(), ["check", "check"]);
}

#[test]
fn update_runtime_forwards_progress_before_install_completion() {
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let dispatcher = UpdateDispatcher::spawn(
        Arc::new(BlockingSource {
            calls: Arc::new(Mutex::new(Vec::new())),
            first_started: started_tx,
            release_first: Mutex::new(release_rx),
        }),
        Arc::new(|| {}),
    );
    let service = update_test_support::candidate_service();
    let result = service.check().unwrap();
    let candidate = match result.availability {
        UpdateAvailability::Available(candidate) => candidate,
        other => panic!("expected candidate, got {other:?}"),
    };

    dispatcher
        .request_install(
            7,
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
        )
        .unwrap();

    let responses = [
        receive(&dispatcher),
        receive(&dispatcher),
        receive(&dispatcher),
    ];
    assert!(matches!(
        responses[0],
        UpdateResponse::Progress {
            progress: UpdateProgress {
                downloaded_bytes: 4,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        responses[1],
        UpdateResponse::Progress {
            progress: UpdateProgress {
                downloaded_bytes: 10,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        responses[2],
        UpdateResponse::Installed { result: Ok(_), .. }
    ));
}

#[test]
fn update_runtime_debug_and_logs_omit_release_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher = UpdateDispatcher::spawn(Arc::new(FailingSource), Arc::new(|| {}));

    dispatcher.request_check(41).unwrap();
    let response = receive(&dispatcher);
    let debug = format!("{response:?}");
    assert!(debug.contains("MetadataFetchFailed"));
    assert!(!debug.contains("private-summary-sentinel"));

    let log = std::fs::read_to_string(log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert!(log.contains("native.update.check"));
    assert!(log.contains("MetadataFetchFailed"));
    assert!(!log.contains("https://"));
    assert!(!log.contains("private-summary-sentinel"));
}

#[test]
fn dropping_update_dispatcher_stops_idle_worker() {
    let (exited_tx, exited_rx) = mpsc::channel();
    let dispatcher = UpdateDispatcher::spawn(
        Arc::new(ExitSource {
            exited: Some(exited_tx),
        }),
        Arc::new(|| {}),
    );

    drop(dispatcher);

    exited_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("update worker should release its source");
}

#[test]
fn update_worker_disconnect_maps_to_worker_stopped() {
    struct PanicSource;

    impl UpdateSource for PanicSource {
        fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
            panic!("intentional update worker exit")
        }

        fn install(
            &self,
            _request: InstallUpdate,
            _progress: UpdateProgressSink,
        ) -> Result<InstallStarted, UpdateError> {
            panic!("not used")
        }
    }

    let dispatcher = UpdateDispatcher::spawn(Arc::new(PanicSource), Arc::new(|| {}));
    dispatcher.request_check(1).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Err(DispatchError::WorkerStopped) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            other => panic!("expected update worker stop, got {other:?}"),
        }
    }
}

mod update_test_support {
    use codex_plus_core::update::UpdateTarget;
    use codex_plus_manager_service::{
        UpdateDownload, UpdateEnvironment, UpdateEnvironmentError, UpdateService,
    };

    pub struct Environment;

    impl UpdateEnvironment for Environment {
        type Artifact = Vec<u8>;

        fn current_version(&self) -> String {
            "1.0.0".to_owned()
        }
        fn target(&self) -> UpdateTarget {
            UpdateTarget::WindowsX64
        }
        fn fetch_release_metadata(
            &self,
            _maximum_bytes: usize,
        ) -> Result<Vec<u8>, UpdateEnvironmentError> {
            Ok(br#"{"version":"2.0.0","notes":"safe","assets":[{"name":"CodexPlusPlus-2.0.0-setup.exe","browser_download_url":"https://updates.invalid/CodexPlusPlus-2.0.0-setup.exe"}]}"#.to_vec())
        }
        fn open_asset_download(
            &self,
            _url: &str,
        ) -> Result<UpdateDownload, UpdateEnvironmentError> {
            panic!("not used")
        }
        fn create_update_artifact(
            &self,
            _safe_name: &str,
        ) -> Result<Self::Artifact, UpdateEnvironmentError> {
            panic!("not used")
        }
        fn publish_update_artifact(
            &self,
            _artifact: &mut Self::Artifact,
        ) -> Result<(), UpdateEnvironmentError> {
            panic!("not used")
        }
        fn cleanup_update_artifact(&self, _artifact: &mut Self::Artifact) {}
        fn launch_update_artifact(
            &self,
            _artifact: &Self::Artifact,
        ) -> Result<(), UpdateEnvironmentError> {
            panic!("not used")
        }
    }

    pub fn candidate_service() -> UpdateService<Environment> {
        UpdateService::new(Environment)
    }
}
