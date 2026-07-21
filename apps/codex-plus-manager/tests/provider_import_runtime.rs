use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::relay_config::CodexContextEntries;
use codex_plus_manager::runtime::import::{ImportDispatcher, ImportResponse};
use codex_plus_manager_service::{
    CcsDiscovery, ConfirmPendingImport, DismissPendingImport, ImportCcsProviders,
    PendingImportSnapshot, ProviderActivationSummary, ProviderDocument, ProviderImportError,
    ProviderImportOutcome, ProviderImportSource, ProviderRevision, ProviderWorkspace,
};

struct BlockingSource {
    calls: Mutex<Vec<&'static str>>,
    discovery_calls: AtomicUsize,
    started: mpsc::Sender<usize>,
    release_first: Mutex<mpsc::Receiver<()>>,
}

impl ProviderImportSource for BlockingSource {
    fn discover_ccs(&self) -> Result<CcsDiscovery, ProviderImportError> {
        self.calls.lock().unwrap().push("discover");
        let call = self.discovery_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.started.send(call).unwrap();
        if call == 1 {
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(discovery(call))
    }

    fn import_ccs(
        &self,
        _request: ImportCcsProviders,
    ) -> Result<ProviderImportOutcome, ProviderImportError> {
        self.calls.lock().unwrap().push("import");
        Ok(outcome())
    }

    fn load_pending(&self) -> Result<PendingImportSnapshot, ProviderImportError> {
        self.calls.lock().unwrap().push("pending_load");
        Ok(PendingImportSnapshot { pending: None })
    }

    fn confirm_pending(
        &self,
        _request: ConfirmPendingImport,
    ) -> Result<ProviderImportOutcome, ProviderImportError> {
        self.calls.lock().unwrap().push("pending_confirm");
        Ok(outcome())
    }

    fn dismiss_pending(
        &self,
        _request: DismissPendingImport,
    ) -> Result<PendingImportSnapshot, ProviderImportError> {
        self.calls.lock().unwrap().push("pending_dismiss");
        Ok(PendingImportSnapshot { pending: None })
    }
}

fn discovery(call: usize) -> CcsDiscovery {
    CcsDiscovery {
        source_path: format!("fixture-{call}.db"),
        source_revision: "a".repeat(64),
        provider_revision: ProviderRevision::parse("b".repeat(64)).unwrap(),
        providers: Vec::new(),
        importable_count: 0,
        duplicate_count: 0,
    }
}

fn outcome() -> ProviderImportOutcome {
    ProviderImportOutcome {
        imported: 1,
        duplicates: 0,
        profile_id: None,
        profile_name: None,
        workspace: ProviderWorkspace {
            revision: ProviderRevision::parse("c".repeat(64)).unwrap(),
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: false,
                active_profile_id: None,
                active_profile_kind: None,
            },
            context_options: CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    }
}

fn receive(dispatcher: &ImportDispatcher) -> ImportResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for import response"),
            Err(error) => panic!("import worker stopped: {error:?}"),
        }
    }
}

#[test]
fn adjacent_discovery_reads_coalesce_and_wake_once_per_response() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wakes = Arc::clone(&wake_count);
    let source = Arc::new(BlockingSource {
        calls: Mutex::new(Vec::new()),
        discovery_calls: AtomicUsize::new(0),
        started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = ImportDispatcher::spawn(
        source,
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_discovery(1).unwrap();
    assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    dispatcher.request_discovery(2).unwrap();
    dispatcher.request_discovery(3).unwrap();
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
fn mutation_remains_between_reads_and_request_ids_are_preserved() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(BlockingSource {
        calls: Mutex::new(Vec::new()),
        discovery_calls: AtomicUsize::new(0),
        started: started_tx,
        release_first: Mutex::new(release_rx),
    });
    let dispatcher = ImportDispatcher::spawn(source.clone(), Arc::new(|| {}));

    dispatcher.request_discovery(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher
        .request_ccs_import(
            2,
            ImportCcsProviders {
                source_revision: "a".repeat(64),
                provider_revision: ProviderRevision::parse("b".repeat(64)).unwrap(),
            },
        )
        .unwrap();
    dispatcher.request_discovery(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert!(matches!(
        receive(&dispatcher),
        ImportResponse::CcsImport { request_id: 2, .. }
    ));
    assert_eq!(receive(&dispatcher).request_id(), 3);
    assert_eq!(
        source.calls.lock().unwrap().as_slice(),
        ["discover", "import", "discover"]
    );
}
