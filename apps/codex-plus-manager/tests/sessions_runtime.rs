use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager::runtime::sessions::{SessionDispatcher, SessionResponse};
use codex_plus_manager_service::{
    DeleteSessions, ProviderSyncError, ProviderSyncErrorKind, ProviderSyncOutcome,
    ProviderSyncResult, ProviderSyncRevision, ProviderSyncSource, ProviderSyncStatus,
    ProviderSyncTargetList, ProviderSyncWorkspace, RunProviderSync, SessionDeleteBatchOutcome,
    SessionError, SessionErrorKind, SessionRevision, SessionSource, SessionSummary,
    SessionWorkspace, SetProviderAutoRepair,
};

struct BlockingSessionSource {
    load_calls: Arc<AtomicUsize>,
    load_started: mpsc::Sender<usize>,
    release_first: Mutex<mpsc::Receiver<()>>,
    operations: Arc<Mutex<Vec<String>>>,
}

impl SessionSource for BlockingSessionSource {
    fn load_workspace(&self) -> codex_plus_manager_service::SessionLoadResult {
        let call = self.load_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.load_started.send(call).unwrap();
        if call == 1 {
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(Arc::new(workspace(call)))
    }

    fn delete_sessions(
        &self,
        request: DeleteSessions,
    ) -> codex_plus_manager_service::SessionDeleteResult {
        self.operations
            .lock()
            .unwrap()
            .push(format!("delete:{}", request.confirmed_ids.join(",")));
        Ok(Arc::new(SessionDeleteBatchOutcome {
            outcomes: Vec::new(),
            workspace: workspace(0),
        }))
    }
}

struct RecordingProviderSyncSource {
    operations: Arc<Mutex<Vec<String>>>,
}

impl ProviderSyncSource for RecordingProviderSyncSource {
    fn load_provider_sync_workspace(&self) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        self.operations
            .lock()
            .unwrap()
            .push("provider-load".to_owned());
        Ok(provider_workspace())
    }

    fn run_provider_sync(
        &self,
        request: RunProviderSync,
    ) -> Result<ProviderSyncOutcome, ProviderSyncError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("provider-run:{}", request.target_provider));
        Ok(ProviderSyncOutcome {
            result: provider_result(&request.target_provider),
            workspace: provider_workspace(),
        })
    }

    fn set_provider_auto_repair(
        &self,
        request: SetProviderAutoRepair,
    ) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("auto-repair:{}", request.enabled));
        Ok(provider_workspace())
    }
}

struct FailingSessionSource;

impl SessionSource for FailingSessionSource {
    fn load_workspace(&self) -> codex_plus_manager_service::SessionLoadResult {
        Err(SessionError::with_compatibility_detail(
            SessionErrorKind::LoadFailed,
            "secret-path-and-title".to_owned(),
        ))
    }

    fn delete_sessions(
        &self,
        _request: DeleteSessions,
    ) -> codex_plus_manager_service::SessionDeleteResult {
        Err(SessionError::new(SessionErrorKind::DeleteFailed))
    }
}

struct FailingProviderSyncSource;

impl ProviderSyncSource for FailingProviderSyncSource {
    fn load_provider_sync_workspace(&self) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        Err(ProviderSyncError::new(ProviderSyncErrorKind::LoadFailed))
    }

    fn run_provider_sync(
        &self,
        _request: RunProviderSync,
    ) -> Result<ProviderSyncOutcome, ProviderSyncError> {
        Err(ProviderSyncError::new(ProviderSyncErrorKind::SyncFailed))
    }

    fn set_provider_auto_repair(
        &self,
        _request: SetProviderAutoRepair,
    ) -> Result<ProviderSyncWorkspace, ProviderSyncError> {
        Err(ProviderSyncError::new(
            ProviderSyncErrorKind::SettingsConflict,
        ))
    }
}

#[test]
fn adjacent_reads_coalesce_without_crossing_a_mutation() {
    let load_calls = Arc::new(AtomicUsize::new(0));
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatcher = SessionDispatcher::spawn(
        Arc::new(BlockingSessionSource {
            load_calls: Arc::clone(&load_calls),
            load_started: started_tx,
            release_first: Mutex::new(release_rx),
            operations: Arc::clone(&operations),
        }),
        Arc::new(RecordingProviderSyncSource {
            operations: Arc::clone(&operations),
        }),
        Arc::new(|| {}),
    );

    dispatcher.request_session_load(1).unwrap();
    assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    dispatcher.request_session_load(2).unwrap();
    dispatcher.request_session_load(3).unwrap();
    dispatcher
        .request_delete(4, delete_request("session-1"))
        .unwrap();
    dispatcher.request_session_load(5).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert_eq!(receive(&dispatcher).request_id(), 3);
    assert_eq!(receive(&dispatcher).request_id(), 4);
    assert_eq!(receive(&dispatcher).request_id(), 5);
    assert_eq!(load_calls.load(Ordering::SeqCst), 3);
    assert_eq!(*operations.lock().unwrap(), vec!["delete:session-1"]);
}

#[test]
fn session_delete_and_provider_mutations_remain_fifo() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let dispatcher = SessionDispatcher::spawn(
        Arc::new(BlockingSessionSource {
            load_calls: Arc::new(AtomicUsize::new(0)),
            load_started: started_tx,
            release_first: Mutex::new(release_rx),
            operations: Arc::clone(&operations),
        }),
        Arc::new(RecordingProviderSyncSource {
            operations: Arc::clone(&operations),
        }),
        Arc::new(|| {}),
    );

    dispatcher
        .request_delete(7, delete_request("session-7"))
        .unwrap();
    dispatcher
        .request_provider_run(
            8,
            RunProviderSync {
                target_provider: "openai".to_owned(),
                confirmed_target_provider: "openai".to_owned(),
            },
        )
        .unwrap();
    dispatcher
        .request_auto_repair(
            9,
            SetProviderAutoRepair {
                expected_revision: ProviderSyncRevision::from_digest([1; 32]),
                enabled: true,
            },
        )
        .unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 7);
    assert_eq!(receive(&dispatcher).request_id(), 8);
    assert_eq!(receive(&dispatcher).request_id(), 9);
    assert_eq!(
        *operations.lock().unwrap(),
        vec![
            "delete:session-7",
            "provider-run:openai",
            "auto-repair:true",
        ]
    );
}

#[test]
fn worker_logs_only_safe_failure_metadata_before_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher = SessionDispatcher::spawn(
        Arc::new(FailingSessionSource),
        Arc::new(FailingProviderSyncSource),
        Arc::new(|| {}),
    );

    dispatcher.request_session_load(41).unwrap();
    assert!(matches!(
        receive(&dispatcher),
        SessionResponse::SessionsLoaded {
            request_id: 41,
            result: Err(_),
        }
    ));
    let log = std::fs::read_to_string(log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert!(log.contains("native_manager.sessions_failed"));
    assert!(log.contains("LoadFailed"));
    assert!(log.contains("41"));
    assert!(!log.contains("secret-path-and-title"));
    assert!(!log.contains("compatibility_detail"));
    assert!(!log.contains("backup"));
}

fn receive(dispatcher: &SessionDispatcher) -> SessionResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for session response"),
            Err(error) => panic!("session dispatcher stopped: {error:?}"),
        }
    }
}

fn delete_request(id: &str) -> DeleteSessions {
    DeleteSessions {
        selections: vec![codex_plus_manager_service::DeleteSessionSelection {
            id: id.to_owned(),
            expected_revision: SessionRevision::from_digest([1; 32]),
        }],
        confirmed_ids: vec![id.to_owned()],
    }
}

fn workspace(index: usize) -> SessionWorkspace {
    SessionWorkspace {
        db_paths: vec!["db.sqlite".to_owned()],
        sessions: vec![SessionSummary::new(
            format!("session-{index}"),
            "Session",
            SessionRevision::from_digest([index as u8; 32]),
        )],
        read_issues: Vec::new(),
    }
}

fn provider_workspace() -> ProviderSyncWorkspace {
    ProviderSyncWorkspace {
        targets: ProviderSyncTargetList {
            current_provider: "openai".to_owned(),
            targets: Vec::new(),
        },
        selected_target: "openai".to_owned(),
        auto_repair: false,
        revision: ProviderSyncRevision::from_digest([1; 32]),
    }
}

fn provider_result(target: &str) -> ProviderSyncResult {
    ProviderSyncResult {
        status: ProviderSyncStatus::Synced,
        message: "synced".to_owned(),
        target_provider: target.to_owned(),
        backup_dir: None,
        changed_session_files: 0,
        skipped_locked_rollout_files: Vec::new(),
        sqlite_rows_updated: 0,
        sqlite_provider_rows_updated: 0,
        sqlite_user_event_rows_updated: 0,
        sqlite_cwd_rows_updated: 0,
        updated_workspace_roots: 0,
        encrypted_content_warning: None,
    }
}
