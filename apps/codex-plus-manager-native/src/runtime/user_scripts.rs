use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    DeleteUserScript, InstallMarketScript, ScriptMarketWorkspace, SetUserScriptEnabled,
    SetUserScriptsEnabled, UserScriptError, UserScriptMutationOutcome, UserScriptSource,
    UserScriptWorkspace,
};

use super::{DispatchError, try_receive};

enum UserScriptRequest {
    InspectLocal {
        request_id: u64,
    },
    RefreshMarket {
        request_id: u64,
    },
    Install {
        request_id: u64,
        request: InstallMarketScript,
    },
    SetGlobalEnabled {
        request_id: u64,
        request: SetUserScriptsEnabled,
    },
    SetScriptEnabled {
        request_id: u64,
        request: SetUserScriptEnabled,
    },
    Delete {
        request_id: u64,
        request: DeleteUserScript,
    },
}

pub enum UserScriptResponse {
    LocalInspected {
        request_id: u64,
        result: Result<Arc<UserScriptWorkspace>, UserScriptError>,
    },
    MarketRefreshed {
        request_id: u64,
        result: Result<Arc<ScriptMarketWorkspace>, UserScriptError>,
    },
    MutationFinished {
        request_id: u64,
        result: Result<Arc<UserScriptMutationOutcome>, UserScriptError>,
    },
}

impl UserScriptResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::LocalInspected { request_id, .. }
            | Self::MarketRefreshed { request_id, .. }
            | Self::MutationFinished { request_id, .. } => *request_id,
        }
    }
}

pub struct UserScriptDispatcher {
    requests: mpsc::Sender<UserScriptRequest>,
    responses: mpsc::Receiver<UserScriptResponse>,
}

impl UserScriptDispatcher {
    pub fn spawn(source: Arc<dyn UserScriptSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<UserScriptRequest>();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-user-scripts-worker".to_owned())
            .spawn(move || {
                let mut pending = None;
                while let Some(request) = next_request(&request_rx, &mut pending) {
                    let response = match request {
                        UserScriptRequest::InspectLocal { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    UserScriptRequest::InspectLocal {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            let result = source.inspect_local().map(Arc::new);
                            log_failure("inspect_local", request_id, result.as_ref().err());
                            UserScriptResponse::LocalInspected { request_id, result }
                        }
                        UserScriptRequest::RefreshMarket { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    UserScriptRequest::RefreshMarket {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            let result = source.refresh_market().map(Arc::new);
                            log_failure("refresh_market", request_id, result.as_ref().err());
                            UserScriptResponse::MarketRefreshed { request_id, result }
                        }
                        UserScriptRequest::Install {
                            request_id,
                            request,
                        } => {
                            let result = source.install(request).map(Arc::new);
                            log_failure("install", request_id, result.as_ref().err());
                            UserScriptResponse::MutationFinished { request_id, result }
                        }
                        UserScriptRequest::SetGlobalEnabled {
                            request_id,
                            request,
                        } => {
                            let result = source.set_global_enabled(request).map(Arc::new);
                            log_failure("set_global_enabled", request_id, result.as_ref().err());
                            UserScriptResponse::MutationFinished { request_id, result }
                        }
                        UserScriptRequest::SetScriptEnabled {
                            request_id,
                            request,
                        } => {
                            let result = source.set_script_enabled(request).map(Arc::new);
                            log_failure("set_script_enabled", request_id, result.as_ref().err());
                            UserScriptResponse::MutationFinished { request_id, result }
                        }
                        UserScriptRequest::Delete {
                            request_id,
                            request,
                        } => {
                            let result = source.delete(request).map(Arc::new);
                            log_failure("delete", request_id, result.as_ref().err());
                            UserScriptResponse::MutationFinished { request_id, result }
                        }
                    };
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native user scripts worker");

        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_local(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::InspectLocal { request_id })
    }

    pub fn request_market(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::RefreshMarket { request_id })
    }

    pub fn request_install(
        &self,
        request_id: u64,
        request: InstallMarketScript,
    ) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::Install {
            request_id,
            request,
        })
    }

    pub fn request_set_global(
        &self,
        request_id: u64,
        request: SetUserScriptsEnabled,
    ) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::SetGlobalEnabled {
            request_id,
            request,
        })
    }

    pub fn request_set_script(
        &self,
        request_id: u64,
        request: SetUserScriptEnabled,
    ) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::SetScriptEnabled {
            request_id,
            request,
        })
    }

    pub fn request_delete(
        &self,
        request_id: u64,
        request: DeleteUserScript,
    ) -> Result<(), DispatchError> {
        self.send(UserScriptRequest::Delete {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<UserScriptResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: UserScriptRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn next_request(
    receiver: &mpsc::Receiver<UserScriptRequest>,
    pending: &mut Option<UserScriptRequest>,
) -> Option<UserScriptRequest> {
    pending.take().or_else(|| receiver.recv().ok())
}

fn log_failure(operation: &str, request_id: u64, error: Option<&UserScriptError>) {
    let Some(error) = error else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.user_scripts_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": request_id,
            "error_kind": format!("{:?}", error.kind()),
        }),
    );
}
