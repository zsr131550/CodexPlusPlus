use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::relay_environment::{
    ClashVergeTunCheck, CodexEnvFileCheck, ProxyEnvironmentCheck, RelayEnvironmentReport,
};
use codex_plus_manager::runtime::environment::{EnvironmentDispatcher, EnvironmentResponse};
use codex_plus_manager_service::{
    EnvironmentRemovalOutcome, RelayEnvironmentError, RelayEnvironmentSource,
    RelayEnvironmentWorkspace, RemoveEnvironmentConflicts,
};

struct BlockingSource {
    calls: Mutex<Vec<&'static str>>,
    inspections: AtomicUsize,
    started: mpsc::Sender<usize>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl RelayEnvironmentSource for BlockingSource {
    fn inspect(&self) -> Result<RelayEnvironmentWorkspace, RelayEnvironmentError> {
        self.calls.lock().unwrap().push("inspect");
        let call = self.inspections.fetch_add(1, Ordering::SeqCst) + 1;
        self.started.send(call).unwrap();
        if call == 1 {
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(workspace(call))
    }

    fn remove_conflicts(
        &self,
        _request: RemoveEnvironmentConflicts,
    ) -> Result<EnvironmentRemovalOutcome, RelayEnvironmentError> {
        self.calls.lock().unwrap().push("cleanup");
        Ok(EnvironmentRemovalOutcome {
            removed: Vec::new(),
            failures: Vec::new(),
            backup_path: None,
            remaining: Vec::new(),
            report: report(),
            revision: "c".repeat(64),
        })
    }
}

fn report() -> RelayEnvironmentReport {
    RelayEnvironmentReport {
        clash_verge_tun: ClashVergeTunCheck {
            enabled: false,
            config_path: None,
        },
        proxy_environment: ProxyEnvironmentCheck {
            variables: Vec::new(),
        },
        codex_env_file: CodexEnvFileCheck {
            exists: false,
            path: "fixture/.env".to_owned(),
        },
    }
}

fn workspace(call: usize) -> RelayEnvironmentWorkspace {
    RelayEnvironmentWorkspace {
        report: report(),
        conflicts: Vec::new(),
        revision: format!("{call:064x}"),
    }
}

fn receive(dispatcher: &EnvironmentDispatcher) -> EnvironmentResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for environment response"),
            Err(error) => panic!("environment worker stopped: {error:?}"),
        }
    }
}

#[test]
fn adjacent_inspections_coalesce_and_wake_once_per_response() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wakes = Arc::clone(&wake_count);
    let source = Arc::new(BlockingSource {
        calls: Mutex::new(Vec::new()),
        inspections: AtomicUsize::new(0),
        started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = EnvironmentDispatcher::spawn(
        source,
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_inspection(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_inspection(2).unwrap();
    dispatcher.request_inspection(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert_eq!(receive(&dispatcher).request_id(), 3);
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake_count.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wake_count.load(Ordering::SeqCst), 2);
}

#[test]
fn cleanup_stays_between_inspections_and_runs_off_the_test_thread() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(BlockingSource {
        calls: Mutex::new(Vec::new()),
        inspections: AtomicUsize::new(0),
        started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = EnvironmentDispatcher::spawn(source.clone(), Arc::new(|| {}));

    dispatcher.request_inspection(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher
        .request_cleanup(
            2,
            RemoveEnvironmentConflicts {
                expected_revision: "a".repeat(64),
                names: vec!["OPENAI_API_KEY".to_owned()],
            },
        )
        .unwrap();
    dispatcher.request_inspection(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert!(matches!(
        receive(&dispatcher),
        EnvironmentResponse::Cleanup { request_id: 2, .. }
    ));
    assert_eq!(receive(&dispatcher).request_id(), 3);
    assert_eq!(
        source.calls.lock().unwrap().as_slice(),
        ["inspect", "cleanup", "inspect"]
    );
}
