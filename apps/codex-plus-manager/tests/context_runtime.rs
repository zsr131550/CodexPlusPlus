use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::context_ownership::ContextOwnershipRevision;
use codex_plus_manager::runtime::DispatchError;
use codex_plus_manager::runtime::context::{ContextDispatcher, ContextResponse};
use codex_plus_manager_service::{
    ContextBundle, ContextEntryDraft, ContextEntryKey, ContextEntryLiveState, ContextEntrySummary,
    ContextKind, ContextOwnershipOutcome, ContextSyncDiffSummary, ContextSyncGuard,
    ContextSyncKeys, ContextSyncOutcome, ContextSyncPreview, ContextSyncScope, ContextToolsError,
    ContextToolsSource, ContextWorkspace, DeleteContextEntry, LoadContextEntryDraft,
    PreviewContextSync, ProviderActivationSummary, ProviderDocument, ProviderLiveRevision,
    ProviderRevision, ProviderWorkspace, SaveContextEntry, SetContextEntryEnabled,
    SyncContextToLive,
};

const SECRET: &str = "native-context-runtime-secret-sentinel";

fn revision(character: char) -> ProviderRevision {
    ProviderRevision::parse(character.to_string().repeat(64)).unwrap()
}

fn bundle(character: char) -> ContextBundle {
    let provider_revision = revision(character);
    ContextBundle {
        context: ContextWorkspace {
            provider_revision: provider_revision.clone(),
            live_revision: ProviderLiveRevision::parse(character.to_string().repeat(64)).unwrap(),
            ownership_revision: ContextOwnershipRevision::parse(character.to_string().repeat(64))
                .unwrap(),
            active_provider_id: Some("relay-a".to_string()),
            active_provider_name: Some("Relay A".to_string()),
            entries: vec![ContextEntrySummary {
                key: key(ContextKind::Mcp, "alpha"),
                display_name: "alpha".to_string(),
                enabled: true,
                live_state: ContextEntryLiveState::Matching,
            }],
            unmanaged_live_count: 0,
            sync_needed: true,
        },
        provider: ProviderWorkspace {
            revision: provider_revision,
            document: ProviderDocument {
                profiles: Vec::new(),
                common_config_contents: String::new(),
                context_config_contents: String::new(),
                default_test_model: String::new(),
            },
            activation: ProviderActivationSummary {
                enabled: true,
                active_profile_id: None,
                active_profile_kind: None,
            },
            context_options: codex_plus_core::relay_config::CodexContextEntries {
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                plugins: Vec::new(),
            },
        },
    }
}

fn key(kind: ContextKind, id: &str) -> ContextEntryKey {
    ContextEntryKey {
        kind,
        id: id.to_string(),
    }
}

fn guard() -> ContextSyncGuard {
    ContextSyncGuard {
        expected_provider_revision: revision('a'),
        expected_live_revision: ProviderLiveRevision::parse("a".repeat(64)).unwrap(),
        expected_ownership_revision: ContextOwnershipRevision::parse("a".repeat(64)).unwrap(),
    }
}

struct RecordingSource {
    calls: Mutex<Vec<&'static str>>,
    load_calls: AtomicUsize,
    started: Option<mpsc::Sender<usize>>,
    release_first: Option<Mutex<mpsc::Receiver<()>>>,
    call_threads: Mutex<Vec<thread::ThreadId>>,
    exited: Option<mpsc::Sender<()>>,
}

impl RecordingSource {
    fn immediate() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            load_calls: AtomicUsize::new(0),
            started: None,
            release_first: None,
            call_threads: Mutex::new(Vec::new()),
            exited: None,
        }
    }

    fn record(&self, operation: &'static str) {
        self.calls.lock().unwrap().push(operation);
        self.call_threads
            .lock()
            .unwrap()
            .push(thread::current().id());
    }

    fn blocking_first_load(started: mpsc::Sender<usize>, release: mpsc::Receiver<()>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            load_calls: AtomicUsize::new(0),
            started: Some(started),
            release_first: Some(Mutex::new(release)),
            call_threads: Mutex::new(Vec::new()),
            exited: None,
        }
    }

    fn reporting_exit(exited: mpsc::Sender<()>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            load_calls: AtomicUsize::new(0),
            started: None,
            release_first: None,
            call_threads: Mutex::new(Vec::new()),
            exited: Some(exited),
        }
    }
}

impl Drop for RecordingSource {
    fn drop(&mut self) {
        if let Some(exited) = self.exited.take() {
            let _ = exited.send(());
        }
    }
}

impl ContextToolsSource for RecordingSource {
    fn load_workspace(&self) -> Result<ContextBundle, ContextToolsError> {
        self.record("workspace");
        let call = self.load_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(started) = &self.started {
            started.send(call).unwrap();
        }
        if call == 1
            && let Some(release) = &self.release_first
        {
            release.lock().unwrap().recv().unwrap();
        }
        Ok(bundle(char::from_digit((call as u32).min(9), 10).unwrap()))
    }

    fn load_entry_draft(
        &self,
        request: LoadContextEntryDraft,
    ) -> Result<ContextEntryDraft, ContextToolsError> {
        self.record("draft");
        Ok(ContextEntryDraft {
            provider_revision: request.expected_provider_revision,
            key: request.key,
            toml_body: format!("token = \"{SECRET}\"\n"),
        })
    }

    fn save_entry(&self, _request: SaveContextEntry) -> Result<ContextBundle, ContextToolsError> {
        self.record("save");
        Ok(bundle('b'))
    }

    fn set_entry_enabled(
        &self,
        _request: SetContextEntryEnabled,
    ) -> Result<ContextBundle, ContextToolsError> {
        self.record("toggle");
        Ok(bundle('b'))
    }

    fn delete_entry(
        &self,
        _request: DeleteContextEntry,
    ) -> Result<ContextBundle, ContextToolsError> {
        self.record("delete");
        Ok(bundle('b'))
    }

    fn preview_context_sync(
        &self,
        request: PreviewContextSync,
    ) -> Result<ContextSyncPreview, ContextToolsError> {
        self.record("preview");
        Ok(ContextSyncPreview {
            guard: request.guard,
            active_provider_id: Some("relay-a".to_string()),
            diff: ContextSyncDiffSummary::default(),
            keys: ContextSyncKeys::default(),
        })
    }

    fn sync_context_to_live(
        &self,
        _request: SyncContextToLive,
    ) -> Result<ContextSyncOutcome, ContextToolsError> {
        self.record("sync");
        Ok(ContextSyncOutcome {
            bundle: bundle('b'),
            backup_path: Some("C:/private/backup.toml".to_string()),
            ownership: ContextOwnershipOutcome::Updated,
            diff: ContextSyncDiffSummary::default(),
        })
    }
}

fn receive(dispatcher: &ContextDispatcher) -> Result<ContextResponse, DispatchError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return Ok(response),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for context response"),
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn adjacent_workspace_refreshes_coalesce_to_greatest_id() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(RecordingSource::blocking_first_load(started_tx, release_rx));
    let dispatcher = ContextDispatcher::spawn(source, Arc::new(|| {}));

    dispatcher.request_workspace(1).unwrap();
    assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    dispatcher.request_workspace(2).unwrap();
    dispatcher.request_workspace(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).unwrap().request_id(), 1);
    assert_eq!(receive(&dispatcher).unwrap().request_id(), 3);
}

#[test]
fn draft_load_does_not_coalesce_or_cross_a_mutation() {
    let source = Arc::new(RecordingSource::immediate());
    let dispatcher = ContextDispatcher::spawn(source.clone(), Arc::new(|| {}));
    let draft = LoadContextEntryDraft {
        expected_provider_revision: revision('a'),
        key: key(ContextKind::Mcp, "alpha"),
    };
    dispatcher.request_draft(1, draft.clone()).unwrap();
    dispatcher
        .request_toggle(
            2,
            SetContextEntryEnabled {
                expected_provider_revision: revision('a'),
                key: key(ContextKind::Mcp, "alpha"),
                enabled: false,
            },
        )
        .unwrap();
    dispatcher.request_draft(3, draft).unwrap();

    let ids = [
        receive(&dispatcher).unwrap().request_id(),
        receive(&dispatcher).unwrap().request_id(),
        receive(&dispatcher).unwrap().request_id(),
    ];
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(*source.calls.lock().unwrap(), ["draft", "toggle", "draft"]);
}

#[test]
fn stored_and_live_mutations_remain_fifo() {
    let source = Arc::new(RecordingSource::immediate());
    let dispatcher = ContextDispatcher::spawn(source.clone(), Arc::new(|| {}));
    dispatcher
        .request_delete(
            1,
            DeleteContextEntry {
                expected_provider_revision: revision('a'),
                key: key(ContextKind::Mcp, "alpha"),
                confirmed_key: key(ContextKind::Mcp, "alpha"),
            },
        )
        .unwrap();
    dispatcher
        .request_preview(
            2,
            PreviewContextSync {
                guard: guard(),
                scope: ContextSyncScope::ActiveProvider,
            },
        )
        .unwrap();
    dispatcher
        .request_sync(
            3,
            SyncContextToLive {
                guard: guard(),
                scope: ContextSyncScope::ActiveProvider,
            },
        )
        .unwrap();

    assert_eq!(receive(&dispatcher).unwrap().request_id(), 1);
    assert_eq!(receive(&dispatcher).unwrap().request_id(), 2);
    assert_eq!(receive(&dispatcher).unwrap().request_id(), 3);
    assert_eq!(*source.calls.lock().unwrap(), ["delete", "preview", "sync"]);
}

#[test]
fn every_response_preserves_request_id_and_wakes_once() {
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_for_callback = Arc::clone(&wake_count);
    let dispatcher = ContextDispatcher::spawn(
        Arc::new(RecordingSource::immediate()),
        Arc::new(move || {
            wake_for_callback.fetch_add(1, Ordering::SeqCst);
        }),
    );

    dispatcher.request_workspace(41).unwrap();
    assert_eq!(receive(&dispatcher).unwrap().request_id(), 41);
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake_count.load(Ordering::SeqCst) != 1 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wake_count.load(Ordering::SeqCst), 1);
}

#[test]
fn service_calls_run_off_the_ui_thread() {
    let ui_thread = thread::current().id();
    let source = Arc::new(RecordingSource::immediate());
    let dispatcher = ContextDispatcher::spawn(source.clone(), Arc::new(|| {}));

    dispatcher.request_workspace(1).unwrap();
    receive(&dispatcher).unwrap();

    assert_ne!(source.call_threads.lock().unwrap()[0], ui_thread);
}

#[test]
fn drop_stops_an_idle_worker() {
    let (exited_tx, exited_rx) = mpsc::channel();
    let source = Arc::new(RecordingSource::reporting_exit(exited_tx));
    let dispatcher = ContextDispatcher::spawn(source, Arc::new(|| {}));

    drop(dispatcher);

    exited_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("context worker should release the source");
}

struct PanickingSource;

impl ContextToolsSource for PanickingSource {
    fn load_workspace(&self) -> Result<ContextBundle, ContextToolsError> {
        panic!("injected worker exit")
    }
    fn load_entry_draft(
        &self,
        _request: LoadContextEntryDraft,
    ) -> Result<ContextEntryDraft, ContextToolsError> {
        unreachable!()
    }
    fn save_entry(&self, _request: SaveContextEntry) -> Result<ContextBundle, ContextToolsError> {
        unreachable!()
    }
    fn set_entry_enabled(
        &self,
        _request: SetContextEntryEnabled,
    ) -> Result<ContextBundle, ContextToolsError> {
        unreachable!()
    }
    fn delete_entry(
        &self,
        _request: DeleteContextEntry,
    ) -> Result<ContextBundle, ContextToolsError> {
        unreachable!()
    }
    fn preview_context_sync(
        &self,
        _request: PreviewContextSync,
    ) -> Result<ContextSyncPreview, ContextToolsError> {
        unreachable!()
    }
    fn sync_context_to_live(
        &self,
        _request: SyncContextToLive,
    ) -> Result<ContextSyncOutcome, ContextToolsError> {
        unreachable!()
    }
}

#[test]
fn worker_failure_maps_to_stable_worker_stopped() {
    let dispatcher = ContextDispatcher::spawn(Arc::new(PanickingSource), Arc::new(|| {}));
    dispatcher.request_workspace(1).unwrap();

    assert_eq!(
        receive(&dispatcher).unwrap_err(),
        DispatchError::WorkerStopped
    );
}

#[test]
fn runtime_debug_never_exposes_toml_body() {
    let dispatcher =
        ContextDispatcher::spawn(Arc::new(RecordingSource::immediate()), Arc::new(|| {}));
    dispatcher
        .request_draft(
            1,
            LoadContextEntryDraft {
                expected_provider_revision: revision('a'),
                key: key(ContextKind::Mcp, "alpha"),
            },
        )
        .unwrap();

    let response = receive(&dispatcher).unwrap();
    assert!(!format!("{response:?}").contains(SECRET));
}
