use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{OverviewError, OverviewSnapshot, OverviewSource};

pub mod context;
pub mod environment;
pub mod import;
pub mod marketplace;
pub mod provider;
pub mod sessions;
pub mod user_scripts;
pub mod zed_remote;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchError {
    WorkerStopped,
}

pub(crate) fn try_receive<T>(receiver: &mpsc::Receiver<T>) -> Result<Option<T>, DispatchError> {
    match receiver.try_recv() {
        Ok(response) => Ok(Some(response)),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => Err(DispatchError::WorkerStopped),
    }
}

#[cfg(test)]
pub(crate) struct DiagnosticLogTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn diagnostic_log_test_guard(path: std::path::PathBuf) -> DiagnosticLogTestGuard {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let lock = LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(path));
    DiagnosticLogTestGuard { _lock: lock }
}

#[cfg(test)]
impl Drop for DiagnosticLogTestGuard {
    fn drop(&mut self) {
        codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    }
}

#[derive(Debug)]
pub struct OverviewResponse {
    pub request_id: u64,
    pub result: Result<Arc<OverviewSnapshot>, OverviewError>,
}

pub struct OverviewDispatcher {
    requests: mpsc::Sender<u64>,
    responses: mpsc::Receiver<OverviewResponse>,
}

impl OverviewDispatcher {
    pub fn spawn(source: Arc<dyn OverviewSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<u64>();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-overview-worker".to_owned())
            .spawn(move || {
                while let Ok(mut request_id) = request_rx.recv() {
                    for pending_id in request_rx.try_iter() {
                        request_id = request_id.max(pending_id);
                    }

                    let result = source.load_overview().map(Arc::new);
                    if let Err(error) = &result {
                        let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                            "native_manager.overview_failed",
                            serde_json::json!({
                                "kind": format!("{:?}", error.kind()),
                                "detail": error.detail(),
                            }),
                        );
                    }
                    if response_tx
                        .send(OverviewResponse { request_id, result })
                        .is_err()
                    {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native overview worker");

        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request(&self, request_id: u64) -> Result<(), DispatchError> {
        self.requests
            .send(request_id)
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn try_recv(&self) -> Result<Option<OverviewResponse>, DispatchError> {
        try_receive(&self.responses)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    use codex_plus_manager_service::{
        LocatedResource, OverviewError, OverviewErrorKind, OverviewSnapshot, OverviewSource,
        ResourcePresence, ShortcutSnapshot, UpdateCheckState,
    };

    use super::*;

    struct BlockingSource {
        calls: Arc<AtomicUsize>,
        started: mpsc::Sender<usize>,
        release_first: Mutex<mpsc::Receiver<()>>,
    }

    impl OverviewSource for BlockingSource {
        fn load_overview(&self) -> Result<OverviewSnapshot, OverviewError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.started.send(call).unwrap();
            if call == 1 {
                self.release_first.lock().unwrap().recv().unwrap();
            }
            Ok(snapshot(&format!("call-{call}")))
        }
    }

    struct ExitSource {
        exited: Option<mpsc::Sender<()>>,
        calls: Arc<AtomicUsize>,
    }

    struct FailingSource;

    impl OverviewSource for FailingSource {
        fn load_overview(&self) -> Result<OverviewSnapshot, OverviewError> {
            Err(OverviewError::new(
                OverviewErrorKind::LoadFailed,
                "deterministic failure",
            ))
        }
    }

    impl OverviewSource for ExitSource {
        fn load_overview(&self) -> Result<OverviewSnapshot, OverviewError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(snapshot("unexpected"))
        }
    }

    impl Drop for ExitSource {
        fn drop(&mut self) {
            if let Some(exited) = self.exited.take() {
                let _ = exited.send(());
            }
        }
    }

    fn snapshot(version: &str) -> OverviewSnapshot {
        OverviewSnapshot {
            codex_app: LocatedResource {
                presence: ResourcePresence::Missing,
                path: None,
            },
            codex_version: Some(version.to_owned()),
            silent_shortcut: ShortcutSnapshot {
                installed: false,
                path: None,
            },
            management_shortcut: ShortcutSnapshot {
                installed: false,
                path: None,
            },
            latest_launch: None,
            current_version: "1.2.36".to_owned(),
            update_status: UpdateCheckState::NotChecked,
            settings_path: PathBuf::from("settings.json"),
            logs_path: PathBuf::from("diagnostic.log"),
        }
    }

    fn receive(dispatcher: &OverviewDispatcher) -> OverviewResponse {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match dispatcher.try_recv() {
                Ok(Some(response)) => return response,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                Ok(None) => panic!("timed out waiting for overview response"),
                Err(error) => panic!("dispatcher stopped: {error:?}"),
            }
        }
    }

    #[test]
    fn dispatcher_coalesces_requests_queued_during_load_and_wakes_after_send() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let source = Arc::new(BlockingSource {
            calls: Arc::clone(&calls),
            started: started_tx,
            release_first: Mutex::new(release_rx),
        });
        let dispatcher = OverviewDispatcher::spawn(
            source,
            Arc::new(move || {
                wake_count_for_callback.fetch_add(1, Ordering::SeqCst);
            }),
        );

        dispatcher.request(1).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        dispatcher.request(2).unwrap();
        dispatcher.request(3).unwrap();
        release_tx.send(()).unwrap();

        let first = receive(&dispatcher);
        let second = receive(&dispatcher);
        assert_eq!([first.request_id, second.request_id], [1, 3]);
        assert_eq!(
            second.result.unwrap().codex_version.as_deref(),
            Some("call-2")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let wake_deadline = Instant::now() + Duration::from_secs(2);
        while wake_count.load(Ordering::SeqCst) != 2 && Instant::now() < wake_deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(wake_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn dropping_dispatcher_stops_idle_worker() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (exited_tx, exited_rx) = mpsc::channel();
        let dispatcher = OverviewDispatcher::spawn(
            Arc::new(ExitSource {
                exited: Some(exited_tx),
                calls: Arc::clone(&calls),
            }),
            Arc::new(|| {}),
        );

        drop(dispatcher);

        exited_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("worker should release its source after request channel closes");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn service_failures_are_logged_by_the_worker_before_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let log_path = temp.path().join("diagnostic.log");
        let _log_guard = diagnostic_log_test_guard(log_path.clone());
        let dispatcher = OverviewDispatcher::spawn(Arc::new(FailingSource), Arc::new(|| {}));

        dispatcher.request(1).unwrap();
        let response = receive(&dispatcher);
        assert_eq!(response.request_id, 1);
        assert_eq!(
            response.result.unwrap_err().kind(),
            OverviewErrorKind::LoadFailed
        );

        let log = std::fs::read_to_string(log_path).unwrap();
        assert!(log.contains("native_manager.overview_failed"));
        assert!(log.contains("deterministic failure"));
    }
}
