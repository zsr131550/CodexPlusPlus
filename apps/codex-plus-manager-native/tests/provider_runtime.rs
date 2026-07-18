use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::relay_config::{CodexContextEntries, RelayStatus};
use codex_plus_core::settings::{RelayMode, RelayProfile};
use codex_plus_manager_native::runtime::provider::{
    ActivationResponse, ProviderActivationDispatcher, ProviderDispatcher, StoreResponse,
};
use codex_plus_manager_native::state::provider::{LiveMutationCommand, OperationToken};
use codex_plus_manager_service::{
    ApplyActiveProvider, ClearLiveProvider, DiagnoseProviderProfile, DoctorOutcome,
    DoctorRecommendation, FetchProviderModels, ProviderActivationError,
    ProviderActivationErrorKind, ProviderActivationSource, ProviderActivationSummary,
    ProviderDoctorReport, ProviderDocument, ProviderError, ProviderKind, ProviderLiveFiles,
    ProviderLiveRevision, ProviderLiveWorkspace, ProviderModelsResult, ProviderMutationGuard,
    ProviderMutationOutcome, ProviderNetworkError, ProviderNetworkFailureKind, ProviderProfile,
    ProviderRevision, ProviderRollbackOutcome, ProviderSource, ProviderTestOutcome,
    ProviderTestResult, ProviderWorkspace, SafeEndpoint, SaveProviderWorkspace,
    TestProviderProfile,
};

const SECRET: &str = "dispatcher-secret-sentinel";

fn diagnostic_test_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

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

struct ExitActivationSource {
    exited: Option<mpsc::Sender<()>>,
}

impl Drop for ExitActivationSource {
    fn drop(&mut self) {
        if let Some(exited) = self.exited.take() {
            let _ = exited.send(());
        }
    }
}

impl ProviderActivationSource for ExitActivationSource {
    fn load_live_workspace(&self) -> Result<ProviderLiveWorkspace, ProviderActivationError> {
        Ok(live_workspace())
    }

    fn switch_provider(
        &self,
        _request: codex_plus_manager_service::SwitchProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn apply_active_provider(
        &self,
        _request: ApplyActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn clear_live_provider(
        &self,
        _request: ClearLiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn backfill_active_provider(
        &self,
        _request: codex_plus_manager_service::BackfillActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn save_live_file(
        &self,
        _request: codex_plus_manager_service::SaveLiveFile,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }
}

#[test]
fn dropping_activation_dispatcher_releases_source_after_worker_exit() {
    let (exited_tx, exited_rx) = mpsc::channel();
    let dispatcher = ProviderActivationDispatcher::spawn(
        Arc::new(ExitActivationSource {
            exited: Some(exited_tx),
        }),
        Arc::new(|| {}),
    );

    drop(dispatcher);

    exited_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("activation worker should release its source on shutdown");
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
    let _guard = diagnostic_test_lock().lock().unwrap();
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

fn live_workspace() -> ProviderLiveWorkspace {
    ProviderLiveWorkspace {
        provider: workspace('a'),
        status: RelayStatus {
            authenticated: true,
            auth_source: "fixture".to_string(),
            account_label: None,
            config_path: "C:/isolated/config.toml".to_string(),
            configured: true,
            requires_openai_auth: true,
            has_bearer_token: true,
        },
        files: ProviderLiveFiles {
            config_path: "C:/isolated/config.toml".to_string(),
            auth_path: "C:/isolated/auth.json".to_string(),
            config_exists: true,
            auth_exists: true,
            config_contents: "model = \"model-a\"\n".to_string(),
            auth_contents: format!(r#"{{"OPENAI_API_KEY":"{SECRET}"}}"#),
        },
        revision: ProviderLiveRevision::parse("a".repeat(64)).unwrap(),
    }
}

fn live_guard() -> ProviderMutationGuard {
    ProviderMutationGuard {
        expected_provider_revision: ProviderRevision::parse("a".repeat(64)).unwrap(),
        expected_live_revision: ProviderLiveRevision::parse("a".repeat(64)).unwrap(),
    }
}

fn receive_activation(dispatcher: &ProviderActivationDispatcher) -> ActivationResponse {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(response) = dispatcher.try_recv().unwrap() {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for activation response"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

struct BlockingActivationSource {
    calls: Arc<Mutex<Vec<String>>>,
    first_load_started: mpsc::Sender<()>,
    first_load_release: Mutex<mpsc::Receiver<()>>,
}

impl BlockingActivationSource {
    fn record(&self, operation: &str) {
        let thread_name = thread::current().name().unwrap_or("unnamed").to_string();
        self.calls
            .lock()
            .unwrap()
            .push(format!("{operation}@{thread_name}"));
    }

    fn outcome(&self, operation: &str) -> ProviderMutationOutcome {
        self.record(operation);
        ProviderMutationOutcome {
            live: live_workspace(),
            backup_path: Some("C:/isolated/backups/live".to_string()),
            rollback: ProviderRollbackOutcome::NotRequired,
        }
    }
}

impl ProviderActivationSource for BlockingActivationSource {
    fn load_live_workspace(&self) -> Result<ProviderLiveWorkspace, ProviderActivationError> {
        self.record("load");
        let is_first = self
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("load@"))
            .count()
            == 1;
        if is_first {
            self.first_load_started.send(()).unwrap();
            self.first_load_release.lock().unwrap().recv().unwrap();
        }
        Ok(live_workspace())
    }

    fn switch_provider(
        &self,
        _request: codex_plus_manager_service::SwitchProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Ok(self.outcome("switch"))
    }

    fn apply_active_provider(
        &self,
        _request: ApplyActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Ok(self.outcome("reapply"))
    }

    fn clear_live_provider(
        &self,
        _request: ClearLiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Ok(self.outcome("clear"))
    }

    fn backfill_active_provider(
        &self,
        _request: codex_plus_manager_service::BackfillActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Ok(self.outcome("backfill"))
    }

    fn save_live_file(
        &self,
        _request: codex_plus_manager_service::SaveLiveFile,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Ok(self.outcome("save_file"))
    }
}

#[test]
fn activation_lane_coalesces_only_adjacent_refreshes_and_never_crosses_a_mutation() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_for_callback = Arc::clone(&wake_count);
    let dispatcher = ProviderActivationDispatcher::spawn(
        Arc::new(BlockingActivationSource {
            calls: Arc::clone(&calls),
            first_load_started: started_tx,
            first_load_release: Mutex::new(release_rx),
        }),
        Arc::new(move || {
            wake_for_callback.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_load(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_load(2).unwrap();
    dispatcher.request_load(3).unwrap();
    dispatcher
        .request_mutation(
            4,
            LiveMutationCommand::Clear(ClearLiveProvider {
                guard: live_guard(),
            }),
        )
        .unwrap();
    dispatcher.request_load(5).unwrap();
    release_tx.send(()).unwrap();

    let responses = (0..4)
        .map(|_| receive_activation(&dispatcher))
        .collect::<Vec<_>>();
    assert_eq!(
        responses
            .iter()
            .map(ActivationResponse::request_id)
            .collect::<Vec<_>>(),
        [1, 3, 4, 5]
    );
    assert_eq!(
        &*calls.lock().unwrap(),
        &[
            "load@native-provider-activation",
            "load@native-provider-activation",
            "clear@native-provider-activation",
            "load@native-provider-activation",
        ]
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake_count.load(Ordering::SeqCst) < 4 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wake_count.load(Ordering::SeqCst), 4);
}

struct FailingActivationSource;

impl ProviderActivationSource for FailingActivationSource {
    fn load_live_workspace(&self) -> Result<ProviderLiveWorkspace, ProviderActivationError> {
        Ok(live_workspace())
    }

    fn switch_provider(
        &self,
        _request: codex_plus_manager_service::SwitchProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn apply_active_provider(
        &self,
        _request: ApplyActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn clear_live_provider(
        &self,
        _request: ClearLiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        Err(ProviderActivationError::for_failure(
            ProviderActivationErrorKind::RollbackFailed,
            ProviderRollbackOutcome::Failed,
            Some(format!("C:/isolated/{SECRET}/backup")),
        ))
    }

    fn backfill_active_provider(
        &self,
        _request: codex_plus_manager_service::BackfillActiveProvider,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }

    fn save_live_file(
        &self,
        _request: codex_plus_manager_service::SaveLiveFile,
    ) -> Result<ProviderMutationOutcome, ProviderActivationError> {
        unreachable!()
    }
}

#[test]
fn activation_worker_logs_only_stable_failure_and_rollback_kinds() {
    let _guard = diagnostic_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher =
        ProviderActivationDispatcher::spawn(Arc::new(FailingActivationSource), Arc::new(|| {}));
    dispatcher
        .request_mutation(
            9,
            LiveMutationCommand::Clear(ClearLiveProvider {
                guard: live_guard(),
            }),
        )
        .unwrap();
    let response = receive_activation(&dispatcher);
    assert_eq!(response.request_id(), 9);
    assert!(matches!(
        response,
        ActivationResponse::Mutation { result: Err(_), .. }
    ));
    let log = std::fs::read_to_string(&log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);

    assert!(log.contains("native_manager.provider_activation_failed"));
    assert!(log.contains("RollbackFailed"));
    assert!(log.contains("Failed"));
    assert!(!log.contains(SECRET));
}
