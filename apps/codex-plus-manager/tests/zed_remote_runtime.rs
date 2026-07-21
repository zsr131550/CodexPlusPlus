use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::zed_remote::{ZedAvailability, ZedOpenStrategy, ZedRemoteRegistryRevision};
use codex_plus_manager::runtime::zed_remote::{ZedRemoteDispatcher, ZedRemoteResponse};
use codex_plus_manager_service::{
    ForgetZedRemoteProject, OpenZedRemoteProject, SaveZedPreferences, ZedProjectRevision,
    ZedRememberOutcome, ZedRemoteError, ZedRemoteOpenOutcome, ZedRemoteSource, ZedRemoteWorkspace,
    ZedSettingsRevision,
};

struct OrderedSource {
    operations: Arc<Mutex<Vec<&'static str>>>,
    first_load_started: mpsc::Sender<()>,
    release_first_load: Mutex<mpsc::Receiver<()>>,
    load_count: Mutex<usize>,
}

impl ZedRemoteSource for OrderedSource {
    fn load_workspace(&self) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        self.operations.lock().unwrap().push("load");
        let mut count = self.load_count.lock().unwrap();
        *count += 1;
        if *count == 1 {
            self.first_load_started.send(()).unwrap();
            self.release_first_load.lock().unwrap().recv().unwrap();
        }
        Ok(workspace())
    }

    fn save_preferences(
        &self,
        _request: SaveZedPreferences,
    ) -> Result<ZedRemoteWorkspace, ZedRemoteError> {
        self.operations.lock().unwrap().push("save");
        Ok(workspace())
    }

    fn open_project(
        &self,
        _request: OpenZedRemoteProject,
    ) -> Result<ZedRemoteOpenOutcome, ZedRemoteError> {
        self.operations.lock().unwrap().push("launch");
        Ok(ZedRemoteOpenOutcome {
            workspace: workspace(),
            strategy: ZedOpenStrategy::Default,
            url: "zed://redacted".to_owned(),
            remember: ZedRememberOutcome::NotRequested,
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

#[test]
fn load_save_load_obeys_the_mutation_barrier() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatcher = ZedRemoteDispatcher::spawn(
        Arc::new(OrderedSource {
            operations: Arc::clone(&operations),
            first_load_started: started_tx,
            release_first_load: Mutex::new(release_rx),
            load_count: Mutex::new(0),
        }),
        Arc::new(|| {}),
    );

    dispatcher.request_load(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher
        .request_save_preferences(2, save_request())
        .unwrap();
    dispatcher.request_load(3).unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(receive(&dispatcher).request_id(), 1);
    assert_eq!(receive(&dispatcher).request_id(), 2);
    assert_eq!(receive(&dispatcher).request_id(), 3);
    assert_eq!(*operations.lock().unwrap(), vec!["load", "save", "load"]);
}

#[test]
fn launch_and_forget_are_fifo_and_never_coalesced() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let dispatcher = ZedRemoteDispatcher::spawn(
        Arc::new(OrderedSource {
            operations: Arc::clone(&operations),
            first_load_started: started_tx,
            release_first_load: Mutex::new(release_rx),
            load_count: Mutex::new(1),
        }),
        Arc::new(|| {}),
    );

    dispatcher.request_open(7, open_request()).unwrap();
    dispatcher.request_forget(8, forget_request()).unwrap();
    assert!(matches!(
        receive(&dispatcher),
        ZedRemoteResponse::Open { request_id: 7, .. }
    ));
    assert!(matches!(
        receive(&dispatcher),
        ZedRemoteResponse::Forget { request_id: 8, .. }
    ));
    assert_eq!(*operations.lock().unwrap(), vec!["launch", "forget"]);
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

fn workspace() -> ZedRemoteWorkspace {
    ZedRemoteWorkspace {
        settings_revision: ZedSettingsRevision::from_digest([1; 32]),
        registry_revision: ZedRemoteRegistryRevision::from_digest([2; 32]),
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

fn save_request() -> SaveZedPreferences {
    SaveZedPreferences {
        expected_revision: ZedSettingsRevision::from_digest([1; 32]),
        default_strategy: ZedOpenStrategy::NewWindow,
        registry_enabled: false,
    }
}

fn open_request() -> OpenZedRemoteProject {
    OpenZedRemoteProject {
        project_id: "project-a".to_owned(),
        confirmed_project_id: "project-a".to_owned(),
        expected_project_revision: ZedProjectRevision::from_digest([3; 32]),
        expected_registry_revision: ZedRemoteRegistryRevision::from_digest([2; 32]),
        strategy: ZedOpenStrategy::Default,
        confirmed_strategy: ZedOpenStrategy::Default,
        remember: false,
        confirmed_remember: false,
    }
}

fn forget_request() -> ForgetZedRemoteProject {
    ForgetZedRemoteProject {
        expected_registry_revision: ZedRemoteRegistryRevision::from_digest([2; 32]),
        project_id: "project-a".to_owned(),
        confirmed_project_id: "project-a".to_owned(),
    }
}
