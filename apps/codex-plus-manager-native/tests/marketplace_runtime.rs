use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::runtime::marketplace::{MarketplaceDispatcher, MarketplaceResponse};
use codex_plus_manager_service::{
    PluginMarketplaceError, PluginMarketplaceErrorKind, PluginMarketplaceKind,
    PluginMarketplaceRepair, PluginMarketplaceRepairOutcome, PluginMarketplaceRevision,
    PluginMarketplaceSource, PluginMarketplaceStatus, PluginMarketplaceWorkspace,
    RepairPluginMarketplace,
};

struct BlockingMarketplaceSource {
    inspect_calls: Arc<AtomicUsize>,
    inspect_started: mpsc::Sender<usize>,
    release_first: Mutex<mpsc::Receiver<()>>,
    repair_calls: Arc<Mutex<Vec<PluginMarketplaceKind>>>,
}

impl PluginMarketplaceSource for BlockingMarketplaceSource {
    fn inspect(&self) -> Result<PluginMarketplaceWorkspace, PluginMarketplaceError> {
        let call = self.inspect_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.inspect_started.send(call).unwrap();
        if call == 1 {
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(workspace(call as u8))
    }

    fn repair(
        &self,
        request: RepairPluginMarketplace,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        self.repair_calls.lock().unwrap().push(request.kind);
        Ok(PluginMarketplaceRepair {
            outcome: PluginMarketplaceRepairOutcome::Initialized,
            initialized: true,
            configured: true,
            workspace: workspace(9),
        })
    }
}

struct FailingMarketplaceSource;

impl PluginMarketplaceSource for FailingMarketplaceSource {
    fn inspect(&self) -> Result<PluginMarketplaceWorkspace, PluginMarketplaceError> {
        Err(PluginMarketplaceError::new(
            PluginMarketplaceErrorKind::InspectFailed,
        ))
    }

    fn repair(
        &self,
        _request: RepairPluginMarketplace,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        Err(PluginMarketplaceError::new(
            PluginMarketplaceErrorKind::WriteFailed,
        ))
    }
}

#[test]
fn adjacent_inspections_coalesce_to_the_latest_request_and_wake() {
    let inspect_calls = Arc::new(AtomicUsize::new(0));
    let repair_calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_count_for_callback = Arc::clone(&wake_count);
    let dispatcher = MarketplaceDispatcher::spawn(
        Arc::new(BlockingMarketplaceSource {
            inspect_calls: Arc::clone(&inspect_calls),
            inspect_started: started_tx,
            release_first: Mutex::new(release_rx),
            repair_calls,
        }),
        Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_inspection(1).unwrap();
    assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    dispatcher.request_inspection(2).unwrap();
    dispatcher.request_inspection(3).unwrap();
    release_tx.send(()).unwrap();

    let first = receive(&dispatcher);
    let second = receive(&dispatcher);
    assert_eq!(first.request_id(), 1);
    assert_eq!(second.request_id(), 3);
    assert_eq!(inspect_calls.load(Ordering::SeqCst), 2);
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake_count.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wake_count.load(Ordering::SeqCst), 2);
}

#[test]
fn repairs_remain_fifo_and_are_never_coalesced() {
    let repair_calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let dispatcher = MarketplaceDispatcher::spawn(
        Arc::new(BlockingMarketplaceSource {
            inspect_calls: Arc::new(AtomicUsize::new(0)),
            inspect_started: started_tx,
            release_first: Mutex::new(release_rx),
            repair_calls: Arc::clone(&repair_calls),
        }),
        Arc::new(|| {}),
    );

    dispatcher
        .request_repair(7, repair_request(PluginMarketplaceKind::Local))
        .unwrap();
    dispatcher
        .request_repair(8, repair_request(PluginMarketplaceKind::Remote))
        .unwrap();

    let first = receive(&dispatcher);
    let second = receive(&dispatcher);
    assert_eq!([first.request_id(), second.request_id()], [7, 8]);
    assert_eq!(
        *repair_calls.lock().unwrap(),
        vec![PluginMarketplaceKind::Local, PluginMarketplaceKind::Remote,]
    );
    assert!(matches!(
        second,
        MarketplaceResponse::Repaired {
            kind: PluginMarketplaceKind::Remote,
            ..
        }
    ));
}

#[test]
fn worker_logs_only_safe_failure_metadata_before_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let dispatcher =
        MarketplaceDispatcher::spawn(Arc::new(FailingMarketplaceSource), Arc::new(|| {}));

    dispatcher.request_inspection(41).unwrap();
    let response = receive(&dispatcher);

    assert!(matches!(
        response,
        MarketplaceResponse::Inspected {
            request_id: 41,
            result: Err(_),
        }
    ));
    let log = std::fs::read_to_string(log_path).unwrap();
    codex_plus_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert!(log.contains("native_manager.plugin_marketplace_failed"));
    assert!(log.contains("InspectFailed"));
    assert!(log.contains("41"));
    assert!(!log.contains("codex_home"));
    assert!(!log.contains("marketplace_root"));
    assert!(!log.contains("compatibility_detail"));
}

fn receive(dispatcher: &MarketplaceDispatcher) -> MarketplaceResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for marketplace response"),
            Err(error) => panic!("marketplace dispatcher stopped: {error:?}"),
        }
    }
}

fn repair_request(kind: PluginMarketplaceKind) -> RepairPluginMarketplace {
    RepairPluginMarketplace {
        expected_revision: PluginMarketplaceRevision::from_digest([1; 32]),
        kind,
        confirmed_kind: kind,
    }
}

fn workspace(revision_value: u8) -> PluginMarketplaceWorkspace {
    PluginMarketplaceWorkspace {
        revision: PluginMarketplaceRevision::from_digest([revision_value; 32]),
        local: status(false),
        remote: status(false),
    }
}

fn status(healthy: bool) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: healthy,
        config_registered: healthy,
        needs_repair: !healthy,
        plugin_count: usize::from(healthy),
        skill_count: usize::from(healthy),
    }
}
