use std::fmt;
use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    EnhancementError, EnhancementSettingsSource, EnhancementWorkspace, ResetEnhancements,
    SaveEnhancements,
};

use super::{DispatchError, try_receive};

enum EnhancementRequest {
    Load {
        request_id: u64,
    },
    Save {
        request_id: u64,
        request: SaveEnhancements,
    },
    Reset {
        request_id: u64,
        request: ResetEnhancements,
    },
}

impl EnhancementRequest {
    fn request_id(&self) -> u64 {
        match self {
            Self::Load { request_id }
            | Self::Save { request_id, .. }
            | Self::Reset { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Load { .. } => "load",
            Self::Save { .. } => "save",
            Self::Reset { .. } => "reset",
        }
    }
}

impl fmt::Debug for EnhancementRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnhancementRequest")
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .finish()
    }
}

pub enum EnhancementResponse {
    Loaded {
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementError>,
    },
    Saved {
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementError>,
    },
    Reset {
        request_id: u64,
        result: Result<Arc<EnhancementWorkspace>, EnhancementError>,
    },
}

impl EnhancementResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Loaded { request_id, .. }
            | Self::Saved { request_id, .. }
            | Self::Reset { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "load",
            Self::Saved { .. } => "save",
            Self::Reset { .. } => "reset",
        }
    }

    fn event(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "native.enhancements.load",
            Self::Saved { .. } => "native.enhancements.save",
            Self::Reset { .. } => "native.enhancements.reset",
        }
    }

    fn error(&self) -> Option<&EnhancementError> {
        match self {
            Self::Loaded { result, .. }
            | Self::Saved { result, .. }
            | Self::Reset { result, .. } => result.as_ref().err(),
        }
    }
}

impl fmt::Debug for EnhancementResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnhancementResponse")
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .field("success", &self.error().is_none())
            .field("error_kind", &self.error().map(EnhancementError::kind))
            .finish()
    }
}

pub struct EnhancementDispatcher {
    requests: mpsc::Sender<EnhancementRequest>,
    responses: mpsc::Receiver<EnhancementResponse>,
}

impl EnhancementDispatcher {
    pub fn spawn(
        source: Arc<dyn EnhancementSettingsSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-enhancements-worker".to_owned())
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
                        EnhancementRequest::Load { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    EnhancementRequest::Load {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            EnhancementResponse::Loaded {
                                request_id,
                                result: source.load().map(Arc::new),
                            }
                        }
                        EnhancementRequest::Save {
                            request_id,
                            request,
                        } => EnhancementResponse::Saved {
                            request_id,
                            result: source.save(request).map(Arc::new),
                        },
                        EnhancementRequest::Reset {
                            request_id,
                            request,
                        } => EnhancementResponse::Reset {
                            request_id,
                            result: source.reset(request).map(Arc::new),
                        },
                    };
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native enhancements worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(EnhancementRequest::Load { request_id })
    }

    pub fn request_save(
        &self,
        request_id: u64,
        request: SaveEnhancements,
    ) -> Result<(), DispatchError> {
        self.send(EnhancementRequest::Save {
            request_id,
            request,
        })
    }

    pub fn request_reset(
        &self,
        request_id: u64,
        request: ResetEnhancements,
    ) -> Result<(), DispatchError> {
        self.send(EnhancementRequest::Reset {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<EnhancementResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: EnhancementRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(response: &EnhancementResponse) {
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
