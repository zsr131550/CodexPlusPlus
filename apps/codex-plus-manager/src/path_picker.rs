use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use serde::Serialize;
use serde_json::Value;

const RESPONSES_PATH_ENV: &str = "CODEX_PLUS_NATIVE_PATH_PICKER_RESPONSES_PATH";
const RECORD_PATH_ENV: &str = "CODEX_PLUS_NATIVE_PATH_PICKER_RECORD_PATH";
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathPickerTarget {
    MaintenanceExecutable,
    MaintenanceDirectory,
    SettingsOverlayImage,
}

impl PathPickerTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaintenanceExecutable => "maintenance_executable",
            Self::MaintenanceDirectory => "maintenance_directory",
            Self::SettingsOverlayImage => "settings_overlay_image",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "maintenance_executable" => Some(Self::MaintenanceExecutable),
            "maintenance_directory" => Some(Self::MaintenanceDirectory),
            "settings_overlay_image" => Some(Self::SettingsOverlayImage),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPickerErrorKind {
    DialogFailed,
    FixtureReadFailed,
    FixtureInvalid,
    FixtureMissingResponse,
    RecordWriteFailed,
    WorkerStopped,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PathPickerError {
    kind: PathPickerErrorKind,
}

impl PathPickerError {
    pub fn new(kind: PathPickerErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> PathPickerErrorKind {
        self.kind
    }
}

impl fmt::Debug for PathPickerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PathPickerError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for PathPickerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            PathPickerErrorKind::DialogFailed => "path picker dialog failed",
            PathPickerErrorKind::FixtureReadFailed => "path picker fixture read failed",
            PathPickerErrorKind::FixtureInvalid => "path picker fixture is invalid",
            PathPickerErrorKind::FixtureMissingResponse => {
                "path picker fixture response is missing"
            }
            PathPickerErrorKind::RecordWriteFailed => "path picker record write failed",
            PathPickerErrorKind::WorkerStopped => "path picker worker stopped",
        })
    }
}

impl std::error::Error for PathPickerError {}

pub trait PathPicker: Send + Sync + 'static {
    fn pick(&self, target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError>;
}

pub struct RfdPathPicker;

impl fmt::Debug for RfdPathPicker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RfdPathPicker")
    }
}

impl PathPicker for RfdPathPicker {
    fn pick(&self, target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError> {
        let selected = match target {
            PathPickerTarget::MaintenanceExecutable => pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Select Codex executable")
                    .pick_file(),
            )
            .map(|handle| handle.path().to_path_buf()),
            PathPickerTarget::MaintenanceDirectory => pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Select Codex application directory")
                    .pick_folder(),
            )
            .map(|handle| handle.path().to_path_buf()),
            PathPickerTarget::SettingsOverlayImage => pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Select overlay image")
                    .add_filter("Images", IMAGE_EXTENSIONS)
                    .pick_file(),
            )
            .map(|handle| handle.path().to_path_buf()),
        };
        Ok(selected)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PathPickerRequest {
    pub request_id: u64,
    pub target: PathPickerTarget,
}

impl PathPickerRequest {
    pub fn new(request_id: u64, target: PathPickerTarget) -> Self {
        Self { request_id, target }
    }
}

impl fmt::Debug for PathPickerRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PathPickerRequest")
            .field("request_id", &self.request_id)
            .field("target", &self.target)
            .finish()
    }
}

pub struct PathPickerResponse {
    pub request_id: u64,
    pub target: PathPickerTarget,
    pub path: Option<PathBuf>,
    pub error: Option<PathPickerError>,
}

impl PathPickerResponse {
    fn from_result(
        request: PathPickerRequest,
        result: Result<Option<PathBuf>, PathPickerError>,
    ) -> Self {
        match result {
            Ok(path) => Self {
                request_id: request.request_id,
                target: request.target,
                path,
                error: None,
            },
            Err(error) => Self {
                request_id: request.request_id,
                target: request.target,
                path: None,
                error: Some(error),
            },
        }
    }

    pub fn selected(&self) -> bool {
        self.path.is_some() && self.error.is_none()
    }

    pub fn cancelled(&self) -> bool {
        self.path.is_none() && self.error.is_none()
    }
}

impl fmt::Debug for PathPickerResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PathPickerResponse")
            .field("request_id", &self.request_id)
            .field("target", &self.target)
            .field("selected", &self.selected())
            .field("cancelled", &self.cancelled())
            .field(
                "error_kind",
                &self.error.as_ref().map(PathPickerError::kind),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPickerDispatchError {
    WorkerStopped,
}

pub struct PathPickerDispatcher {
    requests: mpsc::Sender<PathPickerRequest>,
    responses: mpsc::Receiver<PathPickerResponse>,
}

impl PathPickerDispatcher {
    pub fn spawn(picker: Arc<dyn PathPicker>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<PathPickerRequest>();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-path-picker-worker".to_owned())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let response =
                        PathPickerResponse::from_result(request, picker.pick(request.target));
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native path picker worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn spawn_for_environment(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::spawn(path_picker_from_environment(), wake)
    }

    pub fn request(&self, request: PathPickerRequest) -> Result<(), PathPickerDispatchError> {
        self.requests
            .send(request)
            .map_err(|_| PathPickerDispatchError::WorkerStopped)
    }

    pub fn try_recv(&self) -> Result<Option<PathPickerResponse>, PathPickerDispatchError> {
        match self.responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(PathPickerDispatchError::WorkerStopped),
        }
    }
}

pub fn path_picker_from_environment() -> Arc<dyn PathPicker> {
    configured_path_picker(
        std::env::var_os(RESPONSES_PATH_ENV).filter(|value| !value.is_empty()),
        std::env::var_os(RECORD_PATH_ENV).filter(|value| !value.is_empty()),
    )
}

fn configured_path_picker(
    responses_path: Option<std::ffi::OsString>,
    record_path: Option<std::ffi::OsString>,
) -> Arc<dyn PathPicker> {
    if responses_path.is_none() && record_path.is_none() {
        return Arc::new(RfdPathPicker);
    }
    Arc::new(IsolatedPathPicker::new(
        responses_path.map(PathBuf::from),
        record_path.map(PathBuf::from),
    ))
}

enum FixtureSelection {
    Selected(PathBuf),
    Cancelled,
}

struct IsolatedPathPicker {
    state: Mutex<IsolatedPathPickerState>,
    record_path: Option<PathBuf>,
}

struct IsolatedPathPickerState {
    responses: Result<BTreeMap<PathPickerTarget, VecDeque<FixtureSelection>>, PathPickerErrorKind>,
    records: Vec<PickerRecord>,
}

impl IsolatedPathPicker {
    fn new(responses_path: Option<PathBuf>, record_path: Option<PathBuf>) -> Self {
        let responses = responses_path
            .ok_or(PathPickerErrorKind::FixtureMissingResponse)
            .and_then(load_fixture_responses);
        Self {
            state: Mutex::new(IsolatedPathPickerState {
                responses,
                records: Vec::new(),
            }),
            record_path,
        }
    }

    fn record(
        &self,
        state: &mut IsolatedPathPickerState,
        target: PathPickerTarget,
        selected: bool,
        cancelled: bool,
    ) -> Result<(), PathPickerError> {
        let Some(path) = &self.record_path else {
            return Ok(());
        };
        state.records.push(PickerRecord {
            target: target.as_str(),
            selected,
            cancelled,
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| PathPickerError::new(PathPickerErrorKind::RecordWriteFailed))?;
        }
        let bytes = serde_json::to_vec_pretty(&state.records)
            .map_err(|_| PathPickerError::new(PathPickerErrorKind::RecordWriteFailed))?;
        std::fs::write(path, bytes)
            .map_err(|_| PathPickerError::new(PathPickerErrorKind::RecordWriteFailed))
    }
}

impl PathPicker for IsolatedPathPicker {
    fn pick(&self, target: PathPickerTarget) -> Result<Option<PathBuf>, PathPickerError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = match &mut state.responses {
            Ok(responses) => responses
                .get_mut(&target)
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| PathPickerError::new(PathPickerErrorKind::FixtureMissingResponse))
                .map(|selection| match selection {
                    FixtureSelection::Selected(path) => Some(path),
                    FixtureSelection::Cancelled => None,
                }),
            Err(kind) => Err(PathPickerError::new(*kind)),
        };
        let selected = result.as_ref().is_ok_and(Option::is_some);
        let cancelled = result.as_ref().is_ok_and(Option::is_none);
        self.record(&mut state, target, selected, cancelled)?;
        result
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerRecord {
    target: &'static str,
    selected: bool,
    cancelled: bool,
}

fn load_fixture_responses(
    path: PathBuf,
) -> Result<BTreeMap<PathPickerTarget, VecDeque<FixtureSelection>>, PathPickerErrorKind> {
    let bytes = std::fs::read(path).map_err(|_| PathPickerErrorKind::FixtureReadFailed)?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| PathPickerErrorKind::FixtureInvalid)?;
    let object = value
        .as_object()
        .ok_or(PathPickerErrorKind::FixtureInvalid)?;
    let mut responses = BTreeMap::new();
    for (key, value) in object {
        let target = PathPickerTarget::from_str(key).ok_or(PathPickerErrorKind::FixtureInvalid)?;
        let values = match value {
            Value::Array(values) => values.as_slice(),
            single => std::slice::from_ref(single),
        };
        let mut selections = VecDeque::new();
        for value in values {
            selections.push_back(match value {
                Value::String(path) if !path.is_empty() => {
                    FixtureSelection::Selected(PathBuf::from(path))
                }
                Value::Null => FixtureSelection::Cancelled,
                _ => return Err(PathPickerErrorKind::FixtureInvalid),
            });
        }
        responses.insert(target, selections);
    }
    Ok(responses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_picker_records_only_safe_ordered_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let responses_path = temp.path().join("responses.json");
        let record_path = temp.path().join("records.json");
        std::fs::write(
            &responses_path,
            serde_json::to_vec(&serde_json::json!({
                "maintenance_executable": ["C:/private/Codex.exe", null],
                "settings_overlay_image": "C:/private/overlay.png"
            }))
            .unwrap(),
        )
        .unwrap();
        let picker = IsolatedPathPicker::new(Some(responses_path), Some(record_path.clone()));

        assert!(
            picker
                .pick(PathPickerTarget::MaintenanceExecutable)
                .unwrap()
                .is_some()
        );
        assert!(
            picker
                .pick(PathPickerTarget::MaintenanceExecutable)
                .unwrap()
                .is_none()
        );
        assert!(
            picker
                .pick(PathPickerTarget::SettingsOverlayImage)
                .unwrap()
                .is_some()
        );

        let record = std::fs::read_to_string(record_path).unwrap();
        assert!(!record.contains("C:/private"));
        assert!(!record.contains("Codex.exe"));
        assert!(!record.contains("overlay.png"));
        let value: Value = serde_json::from_str(&record).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 3);
        assert_eq!(value[0]["target"], "maintenance_executable");
        assert_eq!(value[0]["selected"], true);
        assert_eq!(value[1]["cancelled"], true);
    }

    #[test]
    fn any_partial_isolation_override_fails_closed_without_rfd_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let record_path = temp.path().join("records.json");
        let picker = configured_path_picker(None, Some(record_path.into_os_string()));

        let error = picker
            .pick(PathPickerTarget::MaintenanceDirectory)
            .unwrap_err();

        assert_eq!(error.kind(), PathPickerErrorKind::FixtureMissingResponse);
    }

    #[test]
    fn invalid_or_exhausted_fixture_responses_are_typed() {
        let temp = tempfile::tempdir().unwrap();
        let responses_path = temp.path().join("responses.json");
        std::fs::write(&responses_path, br#"{"maintenance_directory": []}"#).unwrap();
        let picker = IsolatedPathPicker::new(Some(responses_path), None);

        let error = picker
            .pick(PathPickerTarget::MaintenanceDirectory)
            .unwrap_err();

        assert_eq!(error.kind(), PathPickerErrorKind::FixtureMissingResponse);
    }
}
