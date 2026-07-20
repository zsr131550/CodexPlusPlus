use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_core::desktop_integration::{ShortcutSnapshot, WindowsDesktopSnapshot};
use codex_plus_core::startup_registration::{
    OwnedStringValueSnapshot, StartupRegistrationSnapshot,
};
use codex_plus_manager_native::runtime::desktop_integration::{
    DesktopIntegrationDispatcher, DesktopIntegrationResponse,
};
use codex_plus_manager_service::{
    DesktopIntegrationEnvironment, DesktopIntegrationEnvironmentError,
    DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationMutation,
    DesktopIntegrationMutationKind, DesktopIntegrationService, DesktopIntegrationSource,
    MigrateStartAtSignIn, RepairDesktopIntegration, SetStartAtSignIn,
};

fn workspace() -> codex_plus_manager_service::DesktopIntegrationWorkspace {
    #[derive(Clone)]
    struct Environment(DesktopIntegrationEnvironmentSnapshot);
    impl DesktopIntegrationEnvironment for Environment {
        fn inspect_desktop_integration(
            &self,
        ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError>
        {
            Ok(self.0.clone())
        }
        fn apply_desktop_repair_operation(
            &self,
            _: &codex_plus_core::desktop_integration::DesktopRepairOperation,
        ) -> Result<(), DesktopIntegrationEnvironmentError> {
            Ok(())
        }
        fn apply_startup_registration_operation(
            &self,
            _: &codex_plus_core::startup_registration::StartupRegistrationOperation,
        ) -> Result<(), DesktopIntegrationEnvironmentError> {
            Ok(())
        }
    }
    let manager = std::path::PathBuf::from(r"C:\stable\codex-plus-plus-manager.exe");
    let launcher = std::path::PathBuf::from(r"C:\stable\codex-plus-plus.exe");
    let shortcut = |target| ShortcutSnapshot {
        target,
        arguments: String::new(),
    };
    let snapshot = DesktopIntegrationEnvironmentSnapshot::Windows {
        repair: Box::new(WindowsDesktopSnapshot {
            current_exe: manager.clone(),
            launcher_is_file: true,
            desktop_dir: Some(r"C:\Desktop".into()),
            programs_dir: Some(r"C:\Programs".into()),
            desktop_manager: Some(shortcut(manager.clone())),
            start_menu_launcher: Some(shortcut(launcher.clone())),
            start_menu_manager: Some(shortcut(manager.clone())),
            protocol_command: Some(format!("\"{}\" \"%1\"", manager.display())),
        }),
        sign_in: StartupRegistrationSnapshot {
            launcher_path: launcher,
            launcher_is_file: true,
            canonical_run: OwnedStringValueSnapshot::Absent,
            legacy_run: OwnedStringValueSnapshot::Absent,
            legacy_shortcut: None,
        },
    };
    DesktopIntegrationService::new(Environment(snapshot))
        .inspect()
        .unwrap()
}

struct OrderedSource {
    workspace: codex_plus_manager_service::DesktopIntegrationWorkspace,
    calls: Arc<Mutex<Vec<&'static str>>>,
    inspections: AtomicUsize,
    first_started: mpsc::Sender<()>,
    release_first: Mutex<mpsc::Receiver<()>>,
    exited: Option<mpsc::Sender<()>>,
}

impl Drop for OrderedSource {
    fn drop(&mut self) {
        if let Some(exited) = self.exited.take() {
            let _ = exited.send(());
        }
    }
}

impl DesktopIntegrationSource for OrderedSource {
    fn inspect(
        &self,
    ) -> Result<
        codex_plus_manager_service::DesktopIntegrationWorkspace,
        codex_plus_manager_service::DesktopIntegrationError,
    > {
        self.calls.lock().unwrap().push("inspect");
        if self.inspections.fetch_add(1, Ordering::SeqCst) == 0 {
            self.first_started.send(()).unwrap();
            self.release_first.lock().unwrap().recv().unwrap();
        }
        Ok(self.workspace.clone())
    }

    fn repair(
        &self,
        _: RepairDesktopIntegration,
    ) -> Result<DesktopIntegrationMutation, codex_plus_manager_service::DesktopIntegrationError>
    {
        self.calls.lock().unwrap().push("repair");
        Ok(self.mutation(DesktopIntegrationMutationKind::Repair))
    }

    fn migrate_sign_in(
        &self,
        _: MigrateStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, codex_plus_manager_service::DesktopIntegrationError>
    {
        self.calls.lock().unwrap().push("migrate");
        Ok(self.mutation(DesktopIntegrationMutationKind::MigrateSignIn))
    }

    fn set_start_at_sign_in(
        &self,
        request: SetStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, codex_plus_manager_service::DesktopIntegrationError>
    {
        self.calls
            .lock()
            .unwrap()
            .push(if request.enabled { "enable" } else { "disable" });
        Ok(self.mutation(if request.enabled {
            DesktopIntegrationMutationKind::EnableSignIn
        } else {
            DesktopIntegrationMutationKind::DisableSignIn
        }))
    }
}

impl OrderedSource {
    fn mutation(&self, kind: DesktopIntegrationMutationKind) -> DesktopIntegrationMutation {
        DesktopIntegrationMutation {
            kind,
            applied_operation_count: 1,
            workspace: self.workspace.clone(),
        }
    }
}

fn receive(dispatcher: &DesktopIntegrationDispatcher) -> DesktopIntegrationResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for desktop integration response"),
            Err(error) => panic!("dispatcher stopped: {error:?}"),
        }
    }
}

#[test]
fn adjacent_inspections_coalesce_while_mutations_remain_fifo_barriers() {
    let workspace = workspace();
    let revision = workspace.revision;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(OrderedSource {
        workspace,
        calls: Arc::clone(&calls),
        inspections: AtomicUsize::new(0),
        first_started: started_tx,
        release_first: Mutex::new(release_rx),
        exited: None,
    });
    let dispatcher = DesktopIntegrationDispatcher::spawn(source, Arc::new(|| {}));

    dispatcher.request_inspect(1).unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatcher.request_inspect(2).unwrap();
    dispatcher.request_inspect(3).unwrap();
    dispatcher
        .request_set(
            4,
            SetStartAtSignIn {
                expected_revision: revision,
                enabled: true,
            },
        )
        .unwrap();
    dispatcher.request_inspect(5).unwrap();
    dispatcher.request_inspect(6).unwrap();
    release_tx.send(()).unwrap();

    let responses = (0..4)
        .map(|_| receive(&dispatcher).request_id())
        .collect::<Vec<_>>();
    assert_eq!(responses, vec![1, 3, 4, 6]);
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["inspect", "inspect", "enable", "inspect"]
    );
}

#[test]
fn dropping_idle_dispatcher_terminates_worker_without_calls_or_hang() {
    let (started_tx, _started_rx) = mpsc::channel();
    let (_release_tx, release_rx) = mpsc::channel();
    let (exited_tx, exited_rx) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = DesktopIntegrationDispatcher::spawn(
        Arc::new(OrderedSource {
            workspace: workspace(),
            calls: Arc::clone(&calls),
            inspections: AtomicUsize::new(0),
            first_started: started_tx,
            release_first: Mutex::new(release_rx),
            exited: Some(exited_tx),
        }),
        Arc::new(|| {}),
    );

    drop(dispatcher);

    exited_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("worker should release source after request channel closes");
    assert!(calls.lock().unwrap().is_empty());
}
