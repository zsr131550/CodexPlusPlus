use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    EnvironmentRemovalOutcome, RelayEnvironmentError, RelayEnvironmentSource,
    RelayEnvironmentWorkspace, RemoveEnvironmentConflicts,
};

use super::{DispatchError, try_receive};

enum EnvironmentRequest {
    Inspect {
        request_id: u64,
    },
    Cleanup {
        request_id: u64,
        request: RemoveEnvironmentConflicts,
    },
}

#[derive(Debug)]
pub enum EnvironmentResponse {
    Inspection {
        request_id: u64,
        result: Result<Arc<RelayEnvironmentWorkspace>, RelayEnvironmentError>,
    },
    Cleanup {
        request_id: u64,
        result: Result<Arc<EnvironmentRemovalOutcome>, RelayEnvironmentError>,
    },
}

impl EnvironmentResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Inspection { request_id, .. } | Self::Cleanup { request_id, .. } => *request_id,
        }
    }
}

pub struct EnvironmentDispatcher {
    requests: mpsc::Sender<EnvironmentRequest>,
    responses: mpsc::Receiver<EnvironmentResponse>,
}

impl EnvironmentDispatcher {
    pub fn spawn(
        source: Arc<dyn RelayEnvironmentSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-relay-environment".to_owned())
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
                        EnvironmentRequest::Inspect { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    EnvironmentRequest::Inspect {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    cleanup @ EnvironmentRequest::Cleanup { .. } => {
                                        pending = Some(cleanup);
                                        break;
                                    }
                                }
                            }
                            let result = source.inspect().map(Arc::new);
                            log_failure("inspect", request_id, &result);
                            EnvironmentResponse::Inspection { request_id, result }
                        }
                        EnvironmentRequest::Cleanup {
                            request_id,
                            request,
                        } => {
                            let result = source.remove_conflicts(request).map(Arc::new);
                            log_failure("cleanup", request_id, &result);
                            EnvironmentResponse::Cleanup { request_id, result }
                        }
                    };
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native relay environment worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_inspection(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(EnvironmentRequest::Inspect { request_id })
    }

    pub fn request_cleanup(
        &self,
        request_id: u64,
        request: RemoveEnvironmentConflicts,
    ) -> Result<(), DispatchError> {
        self.send(EnvironmentRequest::Cleanup {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<EnvironmentResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: EnvironmentRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure<T>(
    operation: &'static str,
    request_id: u64,
    result: &Result<T, RelayEnvironmentError>,
) {
    let Err(error) = result else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.relay_environment_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": request_id,
            "kind": format!("{:?}", error.kind()),
        }),
    );
}
