use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use codex_plus_manager_native::path_picker::{
    PathPicker, PathPickerDispatchError, PathPickerDispatcher, PathPickerError,
    PathPickerErrorKind, PathPickerRequest, PathPickerResponse, PathPickerTarget,
};

struct FakePathPicker {
    results: Mutex<VecDeque<Result<Option<PathBuf>, PathPickerError>>>,
}

impl FakePathPicker {
    fn from_results(
        results: impl IntoIterator<Item = Result<Option<PathBuf>, PathPickerError>>,
    ) -> Self {
        Self {
            results: Mutex::new(results.into_iter().collect()),
        }
    }
}

impl PathPicker for FakePathPicker {
    fn pick(&self, _target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError> {
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .expect("fake picker response")
    }
}

struct BlockingPathPicker {
    started: mpsc::Sender<(PathPickerTarget, String)>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl PathPicker for BlockingPathPicker {
    fn pick(&self, target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError> {
        self.started
            .send((
                target,
                thread::current().name().unwrap_or_default().to_owned(),
            ))
            .unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(Some(PathBuf::from(match target {
            PathPickerTarget::MaintenanceExecutable => "C:/fixture/Codex.exe",
            PathPickerTarget::MaintenanceDirectory => "C:/fixture/Codex",
            PathPickerTarget::SettingsOverlayImage => "C:/fixture/overlay.png",
        })))
    }
}

struct PanicPathPicker;

impl PathPicker for PanicPathPicker {
    fn pick(&self, _target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError> {
        panic!("intentional picker worker exit")
    }
}

fn receive(dispatcher: &PathPickerDispatcher) -> PathPickerResponse {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Ok(Some(response)) => return response,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            Ok(None) => panic!("timed out waiting for picker response"),
            Err(error) => panic!("picker worker stopped: {error:?}"),
        }
    }
}

#[test]
fn path_picker_worker_routes_typed_results_and_cancel_without_debugging_paths() {
    let picker = Arc::new(FakePathPicker::from_results([
        Ok(Some(PathBuf::from("C:/fixture/Codex.exe"))),
        Ok(None),
        Err(PathPickerError::new(PathPickerErrorKind::DialogFailed)),
    ]));
    let dispatcher = PathPickerDispatcher::spawn(picker, Arc::new(|| {}));

    dispatcher
        .request(PathPickerRequest::new(
            1,
            PathPickerTarget::MaintenanceExecutable,
        ))
        .unwrap();
    dispatcher
        .request(PathPickerRequest::new(
            2,
            PathPickerTarget::SettingsOverlayImage,
        ))
        .unwrap();
    dispatcher
        .request(PathPickerRequest::new(
            3,
            PathPickerTarget::MaintenanceDirectory,
        ))
        .unwrap();

    let first = receive(&dispatcher);
    let second = receive(&dispatcher);
    let third = receive(&dispatcher);
    assert_eq!(
        [first.request_id, second.request_id, third.request_id],
        [1, 2, 3]
    );
    assert_eq!(first.target, PathPickerTarget::MaintenanceExecutable);
    assert!(first.selected());
    assert!(first.path.is_some());
    assert_eq!(second.target, PathPickerTarget::SettingsOverlayImage);
    assert!(second.cancelled());
    assert_eq!(
        third.error.unwrap().kind(),
        PathPickerErrorKind::DialogFailed
    );

    for debug in [format!("{first:?}"), format!("{second:?}")] {
        assert!(!debug.contains("Codex.exe"));
        assert!(!debug.contains("overlay.png"));
        assert!(!debug.contains("C:/fixture"));
    }
}

#[test]
fn path_picker_worker_is_fifo_and_never_runs_two_dialogs_at_once() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let dispatcher = PathPickerDispatcher::spawn(
        Arc::new(BlockingPathPicker {
            started: started_tx,
            release: Mutex::new(release_rx),
        }),
        Arc::new(|| {}),
    );

    dispatcher
        .request(PathPickerRequest::new(
            10,
            PathPickerTarget::MaintenanceExecutable,
        ))
        .unwrap();
    dispatcher
        .request(PathPickerRequest::new(
            11,
            PathPickerTarget::MaintenanceDirectory,
        ))
        .unwrap();

    let first_started = started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(first_started.0, PathPickerTarget::MaintenanceExecutable);
    assert_eq!(first_started.1, "native-path-picker-worker");
    assert!(started_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(receive(&dispatcher).request_id, 10);
    let second_started = started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(second_started.0, PathPickerTarget::MaintenanceDirectory);
    assert_eq!(second_started.1, "native-path-picker-worker");
    release_tx.send(()).unwrap();
    assert_eq!(receive(&dispatcher).request_id, 11);
}

#[test]
fn path_picker_worker_disconnect_is_reported_without_blocking() {
    let dispatcher = PathPickerDispatcher::spawn(Arc::new(PanicPathPicker), Arc::new(|| {}));
    dispatcher
        .request(PathPickerRequest::new(
            20,
            PathPickerTarget::SettingsOverlayImage,
        ))
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match dispatcher.try_recv() {
            Err(PathPickerDispatchError::WorkerStopped) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            other => panic!("expected worker disconnect, got {other:?}"),
        }
    }
}
