use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    DeleteSessions, ProviderSyncError, ProviderSyncOutcome, ProviderSyncSource,
    ProviderSyncWorkspace, RunProviderSync, SessionDeleteResult, SessionLoadResult, SessionSource,
    SetProviderAutoRepair,
};

use super::{DispatchError, try_receive};

enum SessionRequest {
    LoadSessions {
        request_id: u64,
    },
    DeleteSessions {
        request_id: u64,
        request: DeleteSessions,
    },
    LoadProviderSync {
        request_id: u64,
    },
    RunProviderSync {
        request_id: u64,
        request: RunProviderSync,
    },
    SetAutoRepair {
        request_id: u64,
        request: SetProviderAutoRepair,
    },
}

pub enum SessionResponse {
    SessionsLoaded {
        request_id: u64,
        result: SessionLoadResult,
    },
    SessionsDeleted {
        request_id: u64,
        result: SessionDeleteResult,
    },
    ProviderSyncLoaded {
        request_id: u64,
        result: Result<Arc<ProviderSyncWorkspace>, ProviderSyncError>,
    },
    ProviderSyncRan {
        request_id: u64,
        result: Result<Arc<ProviderSyncOutcome>, ProviderSyncError>,
    },
    AutoRepairSaved {
        request_id: u64,
        result: Result<Arc<ProviderSyncWorkspace>, ProviderSyncError>,
    },
}

impl SessionResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::SessionsLoaded { request_id, .. }
            | Self::SessionsDeleted { request_id, .. }
            | Self::ProviderSyncLoaded { request_id, .. }
            | Self::ProviderSyncRan { request_id, .. }
            | Self::AutoRepairSaved { request_id, .. } => *request_id,
        }
    }
}

pub struct SessionDispatcher {
    requests: mpsc::Sender<SessionRequest>,
    responses: mpsc::Receiver<SessionResponse>,
}

impl SessionDispatcher {
    pub fn spawn(
        session_source: Arc<dyn SessionSource>,
        provider_sync_source: Arc<dyn ProviderSyncSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<SessionRequest>();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-sessions-worker".to_owned())
            .spawn(move || {
                let mut pending = None;
                while let Some(request) = next_request(&request_rx, &mut pending) {
                    let response = match request {
                        SessionRequest::LoadSessions { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    SessionRequest::LoadSessions {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            SessionResponse::SessionsLoaded {
                                request_id,
                                result: session_source.load_workspace(),
                            }
                        }
                        SessionRequest::DeleteSessions {
                            request_id,
                            request,
                        } => SessionResponse::SessionsDeleted {
                            request_id,
                            result: session_source.delete_sessions(request),
                        },
                        SessionRequest::LoadProviderSync { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    SessionRequest::LoadProviderSync {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            SessionResponse::ProviderSyncLoaded {
                                request_id,
                                result: provider_sync_source
                                    .load_provider_sync_workspace()
                                    .map(Arc::new),
                            }
                        }
                        SessionRequest::RunProviderSync {
                            request_id,
                            request,
                        } => SessionResponse::ProviderSyncRan {
                            request_id,
                            result: provider_sync_source
                                .run_provider_sync(request)
                                .map(Arc::new),
                        },
                        SessionRequest::SetAutoRepair {
                            request_id,
                            request,
                        } => SessionResponse::AutoRepairSaved {
                            request_id,
                            result: provider_sync_source
                                .set_provider_auto_repair(request)
                                .map(Arc::new),
                        },
                    };
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native sessions worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_session_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(SessionRequest::LoadSessions { request_id })
    }

    pub fn request_delete(
        &self,
        request_id: u64,
        request: DeleteSessions,
    ) -> Result<(), DispatchError> {
        self.send(SessionRequest::DeleteSessions {
            request_id,
            request,
        })
    }

    pub fn request_provider_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(SessionRequest::LoadProviderSync { request_id })
    }

    pub fn request_provider_run(
        &self,
        request_id: u64,
        request: RunProviderSync,
    ) -> Result<(), DispatchError> {
        self.send(SessionRequest::RunProviderSync {
            request_id,
            request,
        })
    }

    pub fn request_auto_repair(
        &self,
        request_id: u64,
        request: SetProviderAutoRepair,
    ) -> Result<(), DispatchError> {
        self.send(SessionRequest::SetAutoRepair {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<SessionResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: SessionRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn next_request(
    receiver: &mpsc::Receiver<SessionRequest>,
    pending: &mut Option<SessionRequest>,
) -> Option<SessionRequest> {
    pending.take().or_else(|| receiver.recv().ok())
}

fn log_failure(response: &SessionResponse) {
    let (operation, request_id, kind) = match response {
        SessionResponse::SessionsLoaded { request_id, result } => (
            "load_sessions",
            *request_id,
            result
                .as_ref()
                .err()
                .map(|error| format!("{:?}", error.kind())),
        ),
        SessionResponse::SessionsDeleted { request_id, result } => (
            "delete_sessions",
            *request_id,
            result
                .as_ref()
                .err()
                .map(|error| format!("{:?}", error.kind())),
        ),
        SessionResponse::ProviderSyncLoaded { request_id, result } => (
            "load_provider_sync",
            *request_id,
            result
                .as_ref()
                .err()
                .map(|error| format!("{:?}", error.kind())),
        ),
        SessionResponse::ProviderSyncRan { request_id, result } => (
            "run_provider_sync",
            *request_id,
            result
                .as_ref()
                .err()
                .map(|error| format!("{:?}", error.kind())),
        ),
        SessionResponse::AutoRepairSaved { request_id, result } => (
            "set_auto_repair",
            *request_id,
            result
                .as_ref()
                .err()
                .map(|error| format!("{:?}", error.kind())),
        ),
    };
    let Some(kind) = kind else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.sessions_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": request_id,
            "error_kind": kind,
        }),
    );
}
