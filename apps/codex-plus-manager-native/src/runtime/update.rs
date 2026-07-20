use std::fmt;
use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    InstallStarted, InstallUpdate, UpdateCheckResult, UpdateError, UpdateProgress, UpdateSource,
};

use super::{DispatchError, try_receive};

enum UpdateRequest {
    Check {
        request_id: u64,
    },
    Install {
        request_id: u64,
        request: InstallUpdate,
    },
}

pub enum UpdateResponse {
    Checked {
        request_id: u64,
        result: Result<Arc<UpdateCheckResult>, UpdateError>,
    },
    Progress {
        request_id: u64,
        progress: UpdateProgress,
    },
    Installed {
        request_id: u64,
        result: Result<InstallStarted, UpdateError>,
    },
}

impl UpdateResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Checked { request_id, .. }
            | Self::Progress { request_id, .. }
            | Self::Installed { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Checked { .. } => "check",
            Self::Progress { .. } => "progress",
            Self::Installed { .. } => "install",
        }
    }

    fn error(&self) -> Option<&UpdateError> {
        match self {
            Self::Checked { result, .. } => result.as_ref().err(),
            Self::Installed { result, .. } => result.as_ref().err(),
            Self::Progress { .. } => None,
        }
    }
}

impl fmt::Debug for UpdateResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("UpdateResponse");
        debug
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .field("success", &self.error().is_none());
        if let Self::Progress { progress, .. } = self {
            debug
                .field("downloaded_bytes", &progress.downloaded_bytes)
                .field("has_total_bytes", &progress.total_bytes.is_some());
        } else {
            debug.field("error_kind", &self.error().map(UpdateError::kind));
        }
        debug.finish()
    }
}

pub struct UpdateDispatcher {
    requests: mpsc::Sender<UpdateRequest>,
    responses: mpsc::Receiver<UpdateResponse>,
}

impl UpdateDispatcher {
    pub fn spawn(source: Arc<dyn UpdateSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-update-worker".to_owned())
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
                        UpdateRequest::Check { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    UpdateRequest::Check {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    install => {
                                        pending = Some(install);
                                        break;
                                    }
                                }
                            }
                            UpdateResponse::Checked {
                                request_id,
                                result: source.check().map(Arc::new),
                            }
                        }
                        UpdateRequest::Install {
                            request_id,
                            request,
                        } => {
                            let progress_tx = response_tx.clone();
                            let progress_wake = Arc::clone(&wake);
                            let progress = Arc::new(move |progress| {
                                if progress_tx
                                    .send(UpdateResponse::Progress {
                                        request_id,
                                        progress,
                                    })
                                    .is_ok()
                                {
                                    progress_wake();
                                }
                            });
                            UpdateResponse::Installed {
                                request_id,
                                result: source.install(request, progress),
                            }
                        }
                    };
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native update worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_check(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(UpdateRequest::Check { request_id })
    }

    pub fn request_install(
        &self,
        request_id: u64,
        request: InstallUpdate,
    ) -> Result<(), DispatchError> {
        self.send(UpdateRequest::Install {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<UpdateResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: UpdateRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(response: &UpdateResponse) {
    let Some(error) = response.error() else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        match response {
            UpdateResponse::Checked { .. } => "native.update.check",
            UpdateResponse::Installed { .. } => "native.update.install",
            UpdateResponse::Progress { .. } => return,
        },
        serde_json::json!({
            "operation": response.operation(),
            "request_id": response.request_id(),
            "success": false,
            "error_kind": format!("{:?}", error.kind()),
        }),
    );
}
