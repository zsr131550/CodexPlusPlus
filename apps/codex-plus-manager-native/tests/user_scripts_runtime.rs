use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::runtime::user_scripts::{UserScriptDispatcher, UserScriptResponse};
use codex_plus_manager_service::{
    DeleteUserScript, InstallMarketScript, ScriptMarketRevision, ScriptMarketWorkspace,
    SetUserScriptEnabled, SetUserScriptsEnabled, UserScriptBackupEvidence, UserScriptError,
    UserScriptErrorKind, UserScriptMutationOutcome, UserScriptRevision, UserScriptSource,
    UserScriptWorkspace,
};

struct BlockingSource {
    local_calls: Arc<AtomicUsize>,
    local_started: mpsc::Sender<usize>,
    release_first: Mutex<mpsc::Receiver<()>>,
    operations: Arc<Mutex<Vec<String>>>,
}

impl UserScriptSource for BlockingSource {
    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        let call = self.local_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.local_started.send(call).unwrap();
        if call == 1 {
            self.release_first.lock().unwrap().recv().unwrap();
        }
        self.operations
            .lock()
            .unwrap()
            .push(format!("local:{call}"));
        Ok(local(call as u8))
    }

    fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError> {
        self.operations.lock().unwrap().push("market".to_string());
        Ok(market())
    }

    fn install(
        &self,
        request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("install:{}", request.script_id));
        Ok(outcome())
    }

    fn set_global_enabled(
        &self,
        request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("global:{}", request.enabled));
        Ok(outcome())
    }

    fn set_script_enabled(
        &self,
        request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("toggle:{}:{}", request.key, request.enabled));
        Ok(outcome())
    }

    fn delete(
        &self,
        request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("delete:{}", request.key));
        Ok(outcome())
    }
}

struct FailingSource;

impl UserScriptSource for FailingSource {
    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        Err(UserScriptError::with_compatibility_detail(
            UserScriptErrorKind::InspectFailed,
            "secret-source C:/private/scripts".to_string(),
        ))
    }

    fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError> {
        Err(UserScriptError::with_compatibility_detail(
            UserScriptErrorKind::MarketRefreshFailed,
            "https://private.invalid/index.json".to_string(),
        ))
    }

    fn install(
        &self,
        _request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Err(UserScriptError::new(UserScriptErrorKind::IntegrityMismatch))
    }

    fn set_global_enabled(
        &self,
        _request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Err(UserScriptError::new(UserScriptErrorKind::WriteFailed))
    }

    fn set_script_enabled(
        &self,
        _request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Err(UserScriptError::with_compatibility_detail(
            UserScriptErrorKind::WriteFailed,
            "secret-key-and-name".to_string(),
        ))
    }

    fn delete(
        &self,
        _request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Err(UserScriptError::new(UserScriptErrorKind::BackupFailed))
    }
}

struct DropSource {
    dropped: Option<mpsc::Sender<()>>,
}

impl Drop for DropSource {
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

impl UserScriptSource for DropSource {
    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        Ok(local(1))
    }

    fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError> {
        Ok(market())
    }

    fn install(
        &self,
        _request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Ok(outcome())
    }

    fn set_global_enabled(
        &self,
        _request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Ok(outcome())
    }

    fn set_script_enabled(
        &self,
        _request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Ok(outcome())
    }

    fn delete(
        &self,
        _request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        Ok(outcome())
    }
}

#[test]
fn adjacent_reads_coalesce_without_crossing_mutations() {
    let local_calls = Arc::new(AtomicUsize::new(0));
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_for_worker = Arc::clone(&wake_count);
    let dispatcher = UserScriptDispatcher::spawn(
        Arc::new(BlockingSource {
            local_calls: Arc::clone(&local_calls),
            local_started: started_tx,
            release_first: Mutex::new(release_rx),
            operations: Arc::clone(&operations),
        }),
        Arc::new(move || {
            wake_for_worker.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_local(1).unwrap();
    assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    dispatcher.request_local(2).unwrap();
    dispatcher.request_local(3).unwrap();
    dispatcher
        .request_set_global(
            4,
            SetUserScriptsEnabled {
                expected_revision: revision(1),
                enabled: false,
            },
        )
        .unwrap();
    dispatcher.request_local(5).unwrap();
    dispatcher.request_market(6).unwrap();
    dispatcher.request_market(7).unwrap();
    dispatcher
        .request_delete(8, delete_request("user:custom.js"))
        .unwrap();
    dispatcher.request_market(9).unwrap();
    release_tx.send(()).unwrap();

    let ids = (0..7)
        .map(|_| receive(&dispatcher).request_id())
        .collect::<Vec<_>>();
    assert_eq!(ids, [1, 3, 4, 5, 7, 8, 9]);
    assert_eq!(local_calls.load(Ordering::SeqCst), 3);
    assert_eq!(wake_count.load(Ordering::SeqCst), 7);
}

#[test]
fn install_toggle_and_delete_mutations_remain_fifo() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let dispatcher = UserScriptDispatcher::spawn(
        Arc::new(BlockingSource {
            local_calls: Arc::new(AtomicUsize::new(0)),
            local_started: started_tx,
            release_first: Mutex::new(release_rx),
            operations: Arc::clone(&operations),
        }),
        Arc::new(|| {}),
    );

    dispatcher.request_install(10, install_request()).unwrap();
    dispatcher
        .request_set_script(
            11,
            SetUserScriptEnabled {
                expected_revision: revision(1),
                key: "user:custom.js".to_string(),
                enabled: true,
            },
        )
        .unwrap();
    dispatcher
        .request_delete(12, delete_request("user:custom.js"))
        .unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 10);
    assert_eq!(receive(&dispatcher).request_id(), 11);
    assert_eq!(receive(&dispatcher).request_id(), 12);
    assert_eq!(
        *operations.lock().unwrap(),
        [
            "install:demo",
            "toggle:user:custom.js:true",
            "delete:user:custom.js"
        ]
    );
}

#[test]
fn worker_logs_only_safe_failure_metadata_before_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher = UserScriptDispatcher::spawn(Arc::new(FailingSource), Arc::new(|| {}));

    dispatcher
        .request_set_script(
            41,
            SetUserScriptEnabled {
                expected_revision: revision(1),
                key: "secret-key-and-name".to_string(),
                enabled: true,
            },
        )
        .unwrap();
    assert!(matches!(
        receive(&dispatcher),
        UserScriptResponse::MutationFinished {
            request_id: 41,
            result: Err(_),
        }
    ));
    let log = std::fs::read_to_string(log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert!(log.contains("native_manager.user_scripts_failed"));
    assert!(log.contains("set_script_enabled"));
    assert!(log.contains("WriteFailed"));
    assert!(log.contains("41"));
    assert!(!log.contains("secret-key-and-name"));
    assert!(!log.contains("C:/private"));
    assert!(!log.contains("https://"));
}

#[test]
fn dropping_dispatcher_stops_an_idle_worker() {
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let dispatcher = UserScriptDispatcher::spawn(
        Arc::new(DropSource {
            dropped: Some(dropped_tx),
        }),
        Arc::new(|| {}),
    );

    drop(dispatcher);

    dropped_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should release the source");
}

fn receive(dispatcher: &UserScriptDispatcher) -> UserScriptResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for user script response"),
            Err(error) => panic!("user script dispatcher stopped: {error:?}"),
        }
    }
}

fn revision(value: u8) -> UserScriptRevision {
    UserScriptRevision::from_digest([value; 32])
}

fn local(value: u8) -> UserScriptWorkspace {
    UserScriptWorkspace {
        revision: revision(value),
        globally_enabled: true,
        scripts: Vec::new(),
    }
}

fn market() -> ScriptMarketWorkspace {
    ScriptMarketWorkspace {
        revision: ScriptMarketRevision::from_digest([1; 32]),
        updated_at: None,
        entries: Vec::new(),
    }
}

fn outcome() -> UserScriptMutationOutcome {
    UserScriptMutationOutcome {
        workspace: local(2),
        backup: UserScriptBackupEvidence::none(),
    }
}

fn install_request() -> InstallMarketScript {
    InstallMarketScript {
        expected_local_revision: revision(1),
        expected_market_revision: ScriptMarketRevision::from_digest([1; 32]),
        script_id: "demo".to_string(),
        confirmed_script_id: "demo".to_string(),
        confirmed_version: "2".to_string(),
        acknowledge_unverified: true,
    }
}

fn delete_request(key: &str) -> DeleteUserScript {
    DeleteUserScript {
        expected_revision: revision(1),
        key: key.to_string(),
        confirmed_key: key.to_string(),
    }
}
