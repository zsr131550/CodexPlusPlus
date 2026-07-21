use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    ForgetZedRemoteProject, OpenZedRemoteProject, SaveZedPreferences, ZedRemoteError,
    ZedRemoteOpenOutcome, ZedRemoteSource, ZedRemoteWorkspace,
};

use super::{DispatchError, try_receive};

enum ZedRemoteRequest {
    Load {
        request_id: u64,
    },
    SavePreferences {
        request_id: u64,
        request: SaveZedPreferences,
    },
    Open {
        request_id: u64,
        request: OpenZedRemoteProject,
    },
    Forget {
        request_id: u64,
        request: ForgetZedRemoteProject,
    },
}

#[derive(Debug)]
pub enum ZedRemoteResponse {
    Load {
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteError>,
    },
    SavePreferences {
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteError>,
    },
    Open {
        request_id: u64,
        result: Result<Arc<ZedRemoteOpenOutcome>, ZedRemoteError>,
    },
    Forget {
        request_id: u64,
        result: Result<Arc<ZedRemoteWorkspace>, ZedRemoteError>,
    },
}

impl ZedRemoteResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Load { request_id, .. }
            | Self::SavePreferences { request_id, .. }
            | Self::Open { request_id, .. }
            | Self::Forget { request_id, .. } => *request_id,
        }
    }
}

pub struct ZedRemoteDispatcher {
    requests: mpsc::Sender<ZedRemoteRequest>,
    responses: mpsc::Receiver<ZedRemoteResponse>,
}

impl ZedRemoteDispatcher {
    pub fn spawn(source: Arc<dyn ZedRemoteSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-zed-remote-worker".to_owned())
            .spawn(move || {
                let mut pending = None;
                loop {
                    let request = match pending.take() {
                        Some(request) => request,
                        None => match request_rx.recv() {
                            Ok(request) => request,
                            Err(_) => break,
                        },
                    };

                    let (operation, response) = match request {
                        ZedRemoteRequest::Load { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    ZedRemoteRequest::Load {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            let result = source.load_workspace().map(Arc::new);
                            ("load", ZedRemoteResponse::Load { request_id, result })
                        }
                        ZedRemoteRequest::SavePreferences {
                            request_id,
                            request,
                        } => {
                            let result = source.save_preferences(request).map(Arc::new);
                            (
                                "save_preferences",
                                ZedRemoteResponse::SavePreferences { request_id, result },
                            )
                        }
                        ZedRemoteRequest::Open {
                            request_id,
                            request,
                        } => {
                            let result = source.open_project(request).map(Arc::new);
                            ("open", ZedRemoteResponse::Open { request_id, result })
                        }
                        ZedRemoteRequest::Forget {
                            request_id,
                            request,
                        } => {
                            let result = source.forget_project(request).map(Arc::new);
                            ("forget", ZedRemoteResponse::Forget { request_id, result })
                        }
                    };
                    log_failure(operation, &response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native zed remote worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(ZedRemoteRequest::Load { request_id })
    }

    pub fn request_save_preferences(
        &self,
        request_id: u64,
        request: SaveZedPreferences,
    ) -> Result<(), DispatchError> {
        self.send(ZedRemoteRequest::SavePreferences {
            request_id,
            request,
        })
    }

    pub fn request_open(
        &self,
        request_id: u64,
        request: OpenZedRemoteProject,
    ) -> Result<(), DispatchError> {
        self.send(ZedRemoteRequest::Open {
            request_id,
            request,
        })
    }

    pub fn request_forget(
        &self,
        request_id: u64,
        request: ForgetZedRemoteProject,
    ) -> Result<(), DispatchError> {
        self.send(ZedRemoteRequest::Forget {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<ZedRemoteResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: ZedRemoteRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(operation: &'static str, response: &ZedRemoteResponse) {
    let error = match response {
        ZedRemoteResponse::Load { result, .. }
        | ZedRemoteResponse::SavePreferences { result, .. }
        | ZedRemoteResponse::Forget { result, .. } => result.as_ref().err(),
        ZedRemoteResponse::Open { result, .. } => result.as_ref().err(),
    };
    let Some(error) = error else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.zed_remote_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": response.request_id(),
            "kind": format!("{:?}", error.kind()),
        }),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::{Duration, Instant};

    use codex_plus_core::zed_remote::{
        ZedAvailability, ZedOpenStrategy, ZedRemoteRegistryRevision,
    };
    use codex_plus_manager_service::{
        ZedRemoteErrorKind, ZedRemoteOpenOutcome, ZedRemoteSource, ZedRemoteWorkspace,
        ZedSettingsRevision,
    };

    use super::*;

    fn revision(byte: u8) -> ZedRemoteRegistryRevision {
        ZedRemoteRegistryRevision::from_digest([byte; 32])
    }

    fn workspace() -> ZedRemoteWorkspace {
        ZedRemoteWorkspace {
            settings_revision: ZedSettingsRevision::from_digest([1; 32]),
            registry_revision: revision(2),
            default_strategy: ZedOpenStrategy::Default,
            registry_enabled: true,
            availability: ZedAvailability {
                platform_supported: true,
                cli_found: true,
                app_found: false,
            },
            projects: Vec::new(),
        }
    }

    struct BlockingSource {
        calls: Arc<AtomicUsize>,
        started: mpsc::Sender<usize>,
        release_first: Mutex<mpsc::Receiver<()>>,
    }

    impl ZedRemoteSource for BlockingSource {
        fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.started.send(call).unwrap();
            if call == 1 {
                self.release_first.lock().unwrap().recv().unwrap();
            }
            let mut value = workspace();
            value.settings_revision = ZedSettingsRevision::from_digest([call as u8; 32]);
            Ok(value)
        }

        fn save_preferences(
            &self,
            _request: SaveZedPreferences,
        ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            Ok(workspace())
        }

        fn open_project(
            &self,
            _request: OpenZedRemoteProject,
        ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError> {
            Ok(ZedRemoteOpenOutcome {
                workspace: workspace(),
                strategy: ZedOpenStrategy::Default,
                url: "zed://redacted".to_owned(),
                remember: codex_plus_manager_service::ZedRememberOutcome::NotRequested,
            })
        }

        fn forget_project(
            &self,
            _request: ForgetZedRemoteProject,
        ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            Ok(workspace())
        }
    }

    struct OrderedSource {
        operations: Arc<Mutex<Vec<&'static str>>>,
        started_save: mpsc::Sender<()>,
        release_save: Mutex<mpsc::Receiver<()>>,
    }

    impl ZedRemoteSource for OrderedSource {
        fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            self.operations.lock().unwrap().push("load");
            Ok(workspace())
        }

        fn save_preferences(
            &self,
            _request: SaveZedPreferences,
        ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            self.operations.lock().unwrap().push("save");
            self.started_save.send(()).unwrap();
            self.release_save.lock().unwrap().recv().unwrap();
            Ok(workspace())
        }

        fn open_project(
            &self,
            _request: OpenZedRemoteProject,
        ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError> {
            self.operations.lock().unwrap().push("open");
            Ok(ZedRemoteOpenOutcome {
                workspace: workspace(),
                strategy: ZedOpenStrategy::Default,
                url: "zed://redacted".to_owned(),
                remember: codex_plus_manager_service::ZedRememberOutcome::NotRequested,
            })
        }

        fn forget_project(
            &self,
            _request: ForgetZedRemoteProject,
        ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
            self.operations.lock().unwrap().push("forget");
            Ok(workspace())
        }
    }

    fn receive(dispatcher: &ZedRemoteDispatcher) -> ZedRemoteResponse {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match dispatcher.try_recv() {
                Ok(Some(response)) => return response,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                Ok(None) => panic!("timed out waiting for Zed response"),
                Err(error) => panic!("dispatcher stopped: {error:?}"),
            }
        }
    }

    fn save_request() -> SaveZedPreferences {
        SaveZedPreferences {
            expected_revision: ZedSettingsRevision::from_digest([1; 32]),
            default_strategy: ZedOpenStrategy::NewWindow,
            registry_enabled: true,
        }
    }

    fn open_request() -> OpenZedRemoteProject {
        OpenZedRemoteProject {
            project_id: "id".to_owned(),
            confirmed_project_id: "id".to_owned(),
            expected_project_revision: codex_plus_manager_service::ZedProjectRevision::from_digest(
                [3; 32],
            ),
            expected_registry_revision: revision(2),
            strategy: ZedOpenStrategy::Default,
            confirmed_strategy: ZedOpenStrategy::Default,
            remember: false,
            confirmed_remember: false,
        }
    }

    fn forget_request() -> ForgetZedRemoteProject {
        ForgetZedRemoteProject {
            expected_registry_revision: revision(2),
            project_id: "id".to_owned(),
            confirmed_project_id: "id".to_owned(),
        }
    }

    #[test]
    fn adjacent_loads_coalesce_to_greatest_request_id_without_crossing_mutations() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let dispatcher = ZedRemoteDispatcher::spawn(
            Arc::new(BlockingSource {
                calls: Arc::clone(&calls),
                started: started_tx,
                release_first: Mutex::new(release_rx),
            }),
            Arc::new(|| {}),
        );

        dispatcher.request_load(1).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        dispatcher.request_load(2).unwrap();
        dispatcher.request_load(3).unwrap();
        release_tx.send(()).unwrap();

        assert_eq!(receive(&dispatcher).request_id(), 1);
        assert_eq!(receive(&dispatcher).request_id(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn save_open_forget_remain_fifo_while_loads_are_coalesced() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let source = Arc::new(OrderedSource {
            operations: Arc::clone(&operations),
            started_save: started_tx,
            release_save: Mutex::new(release_rx),
        });
        let dispatcher = ZedRemoteDispatcher::spawn(source, Arc::new(|| {}));
        dispatcher
            .request_save_preferences(10, save_request())
            .unwrap();
        assert!(started_rx.recv_timeout(Duration::from_secs(2)).is_ok());
        dispatcher.request_open(11, open_request()).unwrap();
        dispatcher.request_forget(12, forget_request()).unwrap();
        release_tx.send(()).unwrap();

        assert_eq!(receive(&dispatcher).request_id(), 10);
        assert_eq!(receive(&dispatcher).request_id(), 11);
        assert_eq!(receive(&dispatcher).request_id(), 12);
        assert_eq!(*operations.lock().unwrap(), vec!["save", "open", "forget"]);
    }

    #[test]
    fn worker_logs_only_stable_error_kind() {
        struct Failing;
        impl ZedRemoteSource for Failing {
            fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
                Err(ZedRemoteError::new(ZedRemoteErrorKind::LaunchFailed))
            }
            fn save_preferences(
                &self,
                _request: SaveZedPreferences,
            ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
                unreachable!()
            }
            fn open_project(
                &self,
                _request: OpenZedRemoteProject,
            ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError> {
                unreachable!()
            }
            fn forget_project(
                &self,
                _request: ForgetZedRemoteProject,
            ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
                unreachable!()
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let log_path = temp.path().join("native-zed.log");
        let _log_guard = super::super::diagnostic_log_test_guard(log_path.clone());
        let dispatcher = ZedRemoteDispatcher::spawn(Arc::new(Failing), Arc::new(|| {}));
        dispatcher.request_load(4).unwrap();
        let response = receive(&dispatcher);
        assert!(matches!(
            response,
            ZedRemoteResponse::Load { result: Err(_), .. }
        ));
        let log = std::fs::read_to_string(log_path).unwrap();
        assert!(log.contains("native_manager.zed_remote_failed"));
        assert!(log.contains("LaunchFailed"));
        assert!(!log.contains("zed://"));
    }
}
