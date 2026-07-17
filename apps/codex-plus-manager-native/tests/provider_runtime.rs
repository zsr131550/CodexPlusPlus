use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_core::settings::{RelayMode, RelayProfile};
use codex_plus_manager_native::runtime::provider::{ProviderDispatcher, StoreResponse};
use codex_plus_manager_native::state::provider::OperationToken;
use codex_plus_manager_service::{
    DiagnoseProviderProfile, DoctorOutcome, DoctorRecommendation, FetchProviderModels,
    ProviderActivationSummary, ProviderDoctorReport, ProviderDocument, ProviderError, ProviderKind,
    ProviderModelsResult, ProviderNetworkError, ProviderNetworkFailureKind, ProviderProfile,
    ProviderRevision, ProviderSource, ProviderTestOutcome, ProviderTestResult, ProviderWorkspace,
    SafeEndpoint, SaveProviderWorkspace, TestProviderProfile,
};

const SECRET: &str = "dispatcher-secret-sentinel";

fn ordinary_profile() -> ProviderProfile {
    ProviderProfile::Ordinary(RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "model-a".to_string(),
        api_key: SECRET.to_string(),
        relay_mode: RelayMode::PureApi,
        ..RelayProfile::default()
    })
}

fn workspace(revision: char) -> ProviderWorkspace {
    ProviderWorkspace {
        revision: ProviderRevision::parse(revision.to_string().repeat(64)).unwrap(),
        document: ProviderDocument {
            profiles: vec![ordinary_profile()],
            common_config_contents: String::new(),
            context_config_contents: String::new(),
            default_test_model: "model-a".to_string(),
        },
        activation: ProviderActivationSummary {
            enabled: true,
            active_profile_id: Some("relay-a".to_string()),
            active_profile_kind: Some(ProviderKind::Ordinary),
        },
        context_options: CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
    }
}

fn save_request(revision: char) -> SaveProviderWorkspace {
    let workspace = workspace(revision);
    SaveProviderWorkspace {
        expected_revision: workspace.revision,
        document: workspace.document,
    }
}

struct BlockingSource {
    calls: Arc<Mutex<Vec<&'static str>>>,
    load_started: mpsc::Sender<()>,
    load_release: Mutex<mpsc::Receiver<()>>,
    test_started: mpsc::Sender<()>,
    test_release: Mutex<mpsc::Receiver<()>>,
}

impl ProviderSource for BlockingSource {
    fn load_workspace(&self) -> Result<ProviderWorkspace, ProviderError> {
        let first = {
            let mut calls = self.calls.lock().unwrap();
            calls.push("load");
            calls.iter().filter(|call| **call == "load").count() == 1
        };
        if first {
            self.load_started.send(()).unwrap();
            self.load_release.lock().unwrap().recv().unwrap();
        }
        Ok(workspace('a'))
    }

    fn save_workspace(
        &self,
        request: SaveProviderWorkspace,
    ) -> Result<ProviderWorkspace, ProviderError> {
        self.calls.lock().unwrap().push("save");
        let mut saved = workspace('b');
        saved.document = request.document;
        Ok(saved)
    }

    fn test_profile(
        &self,
        _request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError> {
        self.calls.lock().unwrap().push("test");
        self.test_started.send(()).unwrap();
        self.test_release.lock().unwrap().recv().unwrap();
        Ok(ProviderTestResult {
            http_status: Some(200),
            endpoint: None,
            outcome: ProviderTestOutcome::Success,
        })
    }

    fn fetch_models(
        &self,
        _request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        self.calls.lock().unwrap().push("models");
        Ok(ProviderModelsResult {
            models: vec!["model-a".to_string()],
            endpoint: SafeEndpoint::parse("https://example.test/v1/models").unwrap(),
        })
    }

    fn diagnose_profile(
        &self,
        _request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError> {
        self.calls.lock().unwrap().push("doctor");
        Ok(ProviderDoctorReport {
            profile_name: "Relay A".to_string(),
            model: "model-a".to_string(),
            outcome: DoctorOutcome::Passed,
            recommendation: DoctorRecommendation::Ready,
            checks: Vec::new(),
        })
    }
}

fn token(id: u64) -> OperationToken {
    OperationToken {
        request_id: id,
        profile_id: "relay-a".to_string(),
        edit_generation: 7,
    }
}

fn receive_store(dispatcher: &ProviderDispatcher) -> StoreResponse {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(response) = dispatcher.try_recv_store().unwrap() {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for store response"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn store_lane_coalesces_adjacent_loads_but_never_reorders_or_coalesces_saves() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (load_started_tx, load_started_rx) = mpsc::channel();
    let (load_release_tx, load_release_rx) = mpsc::channel();
    let (test_started_tx, _test_started_rx) = mpsc::channel();
    let (_test_release_tx, test_release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_for_callback = wake_count.clone();
    let dispatcher = ProviderDispatcher::spawn(
        Arc::new(BlockingSource {
            calls: calls.clone(),
            load_started: load_started_tx,
            load_release: Mutex::new(load_release_rx),
            test_started: test_started_tx,
            test_release: Mutex::new(test_release_rx),
        }),
        Arc::new(move || {
            wake_for_callback.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_load(1).unwrap();
    load_started_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    dispatcher.request_load(2).unwrap();
    dispatcher.request_load(3).unwrap();
    dispatcher.request_save(4, save_request('a')).unwrap();
    dispatcher.request_save(5, save_request('a')).unwrap();
    load_release_tx.send(()).unwrap();

    let responses = (0..4)
        .map(|_| receive_store(&dispatcher))
        .collect::<Vec<_>>();
    assert_eq!(
        responses
            .iter()
            .map(StoreResponse::request_id)
            .collect::<Vec<_>>(),
        [1, 3, 4, 5]
    );
    assert_eq!(&*calls.lock().unwrap(), &["load", "load", "save", "save"]);
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake_count.load(Ordering::SeqCst) < 4 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wake_count.load(Ordering::SeqCst), 4);
}

#[test]
fn independent_network_lanes_complete_while_connectivity_is_blocked() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (load_started_tx, _load_started_rx) = mpsc::channel();
    let (_load_release_tx, load_release_rx) = mpsc::channel();
    let (test_started_tx, test_started_rx) = mpsc::channel();
    let (test_release_tx, test_release_rx) = mpsc::channel();
    let dispatcher = ProviderDispatcher::spawn(
        Arc::new(BlockingSource {
            calls: calls.clone(),
            load_started: load_started_tx,
            load_release: Mutex::new(load_release_rx),
            test_started: test_started_tx,
            test_release: Mutex::new(test_release_rx),
        }),
        Arc::new(|| {}),
    );

    dispatcher
        .request_test(
            token(1),
            TestProviderProfile {
                profile: ordinary_profile(),
                default_test_model: "model-a".to_string(),
            },
        )
        .unwrap();
    test_started_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    dispatcher
        .request_models(
            token(2),
            FetchProviderModels {
                profile: ordinary_profile(),
            },
        )
        .unwrap();
    dispatcher
        .request_doctor(
            token(3),
            DiagnoseProviderProfile {
                profile: ordinary_profile(),
                default_test_model: "model-a".to_string(),
            },
        )
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let models = loop {
        if let Some(response) = dispatcher.try_recv_models().unwrap() {
            break response;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    let doctor = loop {
        if let Some(response) = dispatcher.try_recv_doctor().unwrap() {
            break response;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(models.token, token(2));
    assert!(models.result.is_ok());
    assert_eq!(doctor.token, token(3));
    assert!(doctor.result.is_ok());
    assert!(dispatcher.try_recv_test().unwrap().is_none());

    test_release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if dispatcher.try_recv_test().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

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

impl ProviderSource for ExitSource {
    fn load_workspace(&self) -> Result<ProviderWorkspace, ProviderError> {
        Ok(workspace('a'))
    }
    fn save_workspace(
        &self,
        _request: SaveProviderWorkspace,
    ) -> Result<ProviderWorkspace, ProviderError> {
        Ok(workspace('a'))
    }
    fn test_profile(
        &self,
        _request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError> {
        unreachable!()
    }
    fn fetch_models(
        &self,
        _request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        unreachable!()
    }
    fn diagnose_profile(
        &self,
        _request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError> {
        unreachable!()
    }
}

#[test]
fn dropping_dispatcher_closes_all_idle_lanes_and_releases_source() {
    let (exited_tx, exited_rx) = mpsc::channel();
    let dispatcher = ProviderDispatcher::spawn(
        Arc::new(ExitSource {
            exited: Some(exited_tx),
        }),
        Arc::new(|| {}),
    );

    drop(dispatcher);

    exited_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("all provider workers should release the source");
}

struct FailingSource;

impl ProviderSource for FailingSource {
    fn load_workspace(&self) -> Result<ProviderWorkspace, ProviderError> {
        Ok(workspace('a'))
    }
    fn save_workspace(
        &self,
        _request: SaveProviderWorkspace,
    ) -> Result<ProviderWorkspace, ProviderError> {
        Ok(workspace('a'))
    }
    fn test_profile(
        &self,
        _request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError> {
        Err(ProviderNetworkError::for_failure(
            ProviderNetworkFailureKind::Timeout,
            None,
            SafeEndpoint::parse(&format!(
                "https://user:{SECRET}@example.test/v1?token={SECRET}"
            )),
        ))
    }
    fn fetch_models(
        &self,
        _request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        unreachable!()
    }
    fn diagnose_profile(
        &self,
        _request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError> {
        unreachable!()
    }
}

#[test]
fn worker_diagnostics_include_only_stable_sanitized_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher = ProviderDispatcher::spawn(Arc::new(FailingSource), Arc::new(|| {}));
    dispatcher
        .request_test(
            token(9),
            TestProviderProfile {
                profile: ordinary_profile(),
                default_test_model: SECRET.to_string(),
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if dispatcher.try_recv_test().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let log = std::fs::read_to_string(&log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);

    assert!(log.contains("native_manager.provider_operation_failed"));
    assert!(log.contains("Timeout"));
    assert!(!log.contains(SECRET));
}
