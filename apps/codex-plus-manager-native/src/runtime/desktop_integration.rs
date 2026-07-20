use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    DesktopIntegrationError, DesktopIntegrationMutation, DesktopIntegrationSource,
    DesktopIntegrationWorkspace, MigrateStartAtSignIn, RepairDesktopIntegration, SetStartAtSignIn,
};

use super::{DispatchError, try_receive};

enum DesktopIntegrationRequest {
    Inspect {
        request_id: u64,
    },
    Repair {
        request_id: u64,
        request: RepairDesktopIntegration,
    },
    Migrate {
        request_id: u64,
        request: MigrateStartAtSignIn,
    },
    Set {
        request_id: u64,
        request: SetStartAtSignIn,
    },
}

impl DesktopIntegrationRequest {
    const fn is_inspect(&self) -> bool {
        matches!(self, Self::Inspect { .. })
    }
}

#[derive(Debug)]
pub enum DesktopIntegrationResponse {
    Inspect {
        request_id: u64,
        result: Result<Arc<DesktopIntegrationWorkspace>, DesktopIntegrationError>,
    },
    Repair {
        request_id: u64,
        result: Result<Arc<DesktopIntegrationMutation>, DesktopIntegrationError>,
    },
    Migrate {
        request_id: u64,
        result: Result<Arc<DesktopIntegrationMutation>, DesktopIntegrationError>,
    },
    Set {
        request_id: u64,
        enabled: bool,
        result: Result<Arc<DesktopIntegrationMutation>, DesktopIntegrationError>,
    },
}

impl DesktopIntegrationResponse {
    pub const fn request_id(&self) -> u64 {
        match self {
            Self::Inspect { request_id, .. }
            | Self::Repair { request_id, .. }
            | Self::Migrate { request_id, .. }
            | Self::Set { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Inspect { .. } => "inspect",
            Self::Repair { .. } => "repair",
            Self::Migrate { .. } => "migrate",
            Self::Set { enabled: true, .. } => "enable",
            Self::Set { enabled: false, .. } => "disable",
        }
    }

    fn error(&self) -> Option<&DesktopIntegrationError> {
        match self {
            Self::Inspect { result, .. } => result.as_ref().err(),
            Self::Repair { result, .. }
            | Self::Migrate { result, .. }
            | Self::Set { result, .. } => result.as_ref().err(),
        }
    }
}

pub struct DesktopIntegrationDispatcher {
    requests: mpsc::Sender<DesktopIntegrationRequest>,
    responses: mpsc::Receiver<DesktopIntegrationResponse>,
}

impl DesktopIntegrationDispatcher {
    pub fn spawn(
        source: Arc<dyn DesktopIntegrationSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<DesktopIntegrationRequest>();
        let (response_tx, response_rx) = mpsc::channel::<DesktopIntegrationResponse>();
        thread::Builder::new()
            .name("native-desktop-integration-worker".to_owned())
            .spawn(move || {
                let mut pending = None;
                while let Some(mut request) = pending.take().or_else(|| request_rx.recv().ok()) {
                    if request.is_inspect() {
                        loop {
                            match request_rx.try_recv() {
                                Ok(next) if next.is_inspect() => request = next,
                                Ok(barrier) => {
                                    pending = Some(barrier);
                                    break;
                                }
                                Err(
                                    mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected,
                                ) => {
                                    break;
                                }
                            }
                        }
                    }

                    let response = execute_request(source.as_ref(), request);
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native desktop integration worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_inspect(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(DesktopIntegrationRequest::Inspect { request_id })
    }

    pub fn request_repair(
        &self,
        request_id: u64,
        request: RepairDesktopIntegration,
    ) -> Result<(), DispatchError> {
        self.send(DesktopIntegrationRequest::Repair {
            request_id,
            request,
        })
    }

    pub fn request_migrate(
        &self,
        request_id: u64,
        request: MigrateStartAtSignIn,
    ) -> Result<(), DispatchError> {
        self.send(DesktopIntegrationRequest::Migrate {
            request_id,
            request,
        })
    }

    pub fn request_set(
        &self,
        request_id: u64,
        request: SetStartAtSignIn,
    ) -> Result<(), DispatchError> {
        self.send(DesktopIntegrationRequest::Set {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<DesktopIntegrationResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: DesktopIntegrationRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn execute_request(
    source: &dyn DesktopIntegrationSource,
    request: DesktopIntegrationRequest,
) -> DesktopIntegrationResponse {
    match request {
        DesktopIntegrationRequest::Inspect { request_id } => DesktopIntegrationResponse::Inspect {
            request_id,
            result: source.inspect().map(Arc::new),
        },
        DesktopIntegrationRequest::Repair {
            request_id,
            request,
        } => DesktopIntegrationResponse::Repair {
            request_id,
            result: source.repair(request).map(Arc::new),
        },
        DesktopIntegrationRequest::Migrate {
            request_id,
            request,
        } => DesktopIntegrationResponse::Migrate {
            request_id,
            result: source.migrate_sign_in(request).map(Arc::new),
        },
        DesktopIntegrationRequest::Set {
            request_id,
            request,
        } => DesktopIntegrationResponse::Set {
            request_id,
            enabled: request.enabled,
            result: source.set_start_at_sign_in(request).map(Arc::new),
        },
    }
}

fn log_failure(response: &DesktopIntegrationResponse) {
    let Some(error) = response.error() else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.desktop_integration_failed",
        serde_json::json!({
            "operation": response.operation(),
            "request_id": response.request_id(),
            "error_kind": format!("{:?}", error.kind()),
        }),
    );
}
