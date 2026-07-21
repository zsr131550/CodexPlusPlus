use std::fmt;
use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    LaunchCodex, LaunchOutcome, LoadMaintenance, MaintenanceError, MaintenanceSource,
    MaintenanceWorkspace, SaveCodexAppPath,
};

use super::{DispatchError, try_receive};

enum MaintenanceRequest {
    Load {
        request_id: u64,
        request: LoadMaintenance,
    },
    SaveAppPath {
        request_id: u64,
        request: SaveCodexAppPath,
    },
    Launch {
        request_id: u64,
        request: LaunchCodex,
    },
}

impl fmt::Debug for MaintenanceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load {
                request_id,
                request,
            } => formatter
                .debug_struct("Load")
                .field("request_id", request_id)
                .field("log_lines", &request.log_lines)
                .finish(),
            Self::SaveAppPath {
                request_id,
                request,
            } => formatter
                .debug_struct("SaveAppPath")
                .field("request_id", request_id)
                .field("request", request)
                .finish(),
            Self::Launch {
                request_id,
                request,
            } => formatter
                .debug_struct("Launch")
                .field("request_id", request_id)
                .field("request", request)
                .finish(),
        }
    }
}

pub enum MaintenanceResponse {
    Loaded {
        request_id: u64,
        result: Result<Arc<MaintenanceWorkspace>, MaintenanceError>,
    },
    AppPathSaved {
        request_id: u64,
        result: Result<Arc<MaintenanceWorkspace>, MaintenanceError>,
    },
    Launched {
        request_id: u64,
        result: Result<LaunchOutcome, MaintenanceError>,
    },
}

impl MaintenanceResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Loaded { request_id, .. }
            | Self::AppPathSaved { request_id, .. }
            | Self::Launched { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "load",
            Self::AppPathSaved { .. } => "save_path",
            Self::Launched { .. } => "launch",
        }
    }

    fn event(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "native.maintenance.load",
            Self::AppPathSaved { .. } => "native.maintenance.save_path",
            Self::Launched { .. } => "native.maintenance.launch",
        }
    }

    fn error(&self) -> Option<&MaintenanceError> {
        match self {
            Self::Loaded { result, .. } | Self::AppPathSaved { result, .. } => {
                result.as_ref().err()
            }
            Self::Launched { result, .. } => result.as_ref().err(),
        }
    }
}

impl fmt::Debug for MaintenanceResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MaintenanceResponse")
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .field("success", &self.error().is_none())
            .field("error_kind", &self.error().map(MaintenanceError::kind))
            .finish()
    }
}

pub struct MaintenanceDispatcher {
    requests: mpsc::Sender<MaintenanceRequest>,
    responses: mpsc::Receiver<MaintenanceResponse>,
}

impl MaintenanceDispatcher {
    pub fn spawn(source: Arc<dyn MaintenanceSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-maintenance-worker".to_owned())
            .spawn(move || {
                let mut pending = None;
                loop {
                    let request = match pending.take() {
                        Some(request) => request,
                        None => match request_rx.recv() {
                            Ok(request) => request,
                            Err(_) => break,
                        },
                    };
                    let response = match request {
                        MaintenanceRequest::Load {
                            mut request_id,
                            mut request,
                        } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    MaintenanceRequest::Load {
                                        request_id: next_id,
                                        request: next_request,
                                    } => {
                                        if next_id >= request_id {
                                            request_id = next_id;
                                            request = next_request;
                                        }
                                    }
                                    mutation => {
                                        pending = Some(mutation);
                                        break;
                                    }
                                }
                            }
                            MaintenanceResponse::Loaded {
                                request_id,
                                result: source.load_workspace(request).map(Arc::new),
                            }
                        }
                        MaintenanceRequest::SaveAppPath {
                            request_id,
                            request,
                        } => MaintenanceResponse::AppPathSaved {
                            request_id,
                            result: source.save_app_path(request).map(Arc::new),
                        },
                        MaintenanceRequest::Launch {
                            request_id,
                            request,
                        } => MaintenanceResponse::Launched {
                            request_id,
                            result: source.launch(request),
                        },
                    };
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native maintenance worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_load(&self, request_id: u64, log_lines: usize) -> Result<(), DispatchError> {
        self.send(MaintenanceRequest::Load {
            request_id,
            request: LoadMaintenance { log_lines },
        })
    }

    pub fn request_save(
        &self,
        request_id: u64,
        request: SaveCodexAppPath,
    ) -> Result<(), DispatchError> {
        self.send(MaintenanceRequest::SaveAppPath {
            request_id,
            request,
        })
    }

    pub fn request_launch(
        &self,
        request_id: u64,
        request: LaunchCodex,
    ) -> Result<(), DispatchError> {
        self.send(MaintenanceRequest::Launch {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<MaintenanceResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: MaintenanceRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(response: &MaintenanceResponse) {
    let Some(error) = response.error() else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        response.event(),
        serde_json::json!({
            "operation": response.operation(),
            "request_id": response.request_id(),
            "success": false,
            "error_kind": format!("{:?}", error.kind()),
        }),
    );
}
