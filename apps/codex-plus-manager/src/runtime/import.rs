use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    CcsDiscovery, ConfirmPendingImport, DismissPendingImport, ImportCcsProviders,
    PendingImportSnapshot, ProviderImportError, ProviderImportOutcome, ProviderImportSource,
};

use super::{DispatchError, try_receive};

enum ImportRequest {
    Discover {
        request_id: u64,
    },
    ImportCcs {
        request_id: u64,
        request: ImportCcsProviders,
    },
    LoadPending {
        request_id: u64,
    },
    ConfirmPending {
        request_id: u64,
        request: ConfirmPendingImport,
    },
    DismissPending {
        request_id: u64,
        request: DismissPendingImport,
    },
}

#[derive(Debug)]
pub enum ImportResponse {
    Discovery {
        request_id: u64,
        result: Result<Arc<CcsDiscovery>, ProviderImportError>,
    },
    CcsImport {
        request_id: u64,
        result: Result<ProviderImportOutcome, ProviderImportError>,
    },
    PendingLoad {
        request_id: u64,
        result: Result<PendingImportSnapshot, ProviderImportError>,
    },
    PendingConfirm {
        request_id: u64,
        result: Result<ProviderImportOutcome, ProviderImportError>,
    },
    PendingDismiss {
        request_id: u64,
        result: Result<PendingImportSnapshot, ProviderImportError>,
    },
}

impl ImportResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Discovery { request_id, .. }
            | Self::CcsImport { request_id, .. }
            | Self::PendingLoad { request_id, .. }
            | Self::PendingConfirm { request_id, .. }
            | Self::PendingDismiss { request_id, .. } => *request_id,
        }
    }
}

pub struct ImportDispatcher {
    requests: mpsc::Sender<ImportRequest>,
    responses: mpsc::Receiver<ImportResponse>,
}

impl ImportDispatcher {
    pub fn spawn(source: Arc<dyn ProviderImportSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-provider-import".to_owned())
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
                        ImportRequest::Discover { mut request_id } => {
                            coalesce_discovery(&request_rx, &mut pending, &mut request_id);
                            let result = source.discover_ccs().map(Arc::new);
                            log_failure("discover_ccs", request_id, &result);
                            ImportResponse::Discovery { request_id, result }
                        }
                        ImportRequest::ImportCcs {
                            request_id,
                            request,
                        } => {
                            let result = source.import_ccs(request);
                            log_failure("import_ccs", request_id, &result);
                            ImportResponse::CcsImport { request_id, result }
                        }
                        ImportRequest::LoadPending { mut request_id } => {
                            coalesce_pending_load(&request_rx, &mut pending, &mut request_id);
                            let result = source.load_pending();
                            log_failure("load_pending", request_id, &result);
                            ImportResponse::PendingLoad { request_id, result }
                        }
                        ImportRequest::ConfirmPending {
                            request_id,
                            request,
                        } => {
                            let result = source.confirm_pending(request);
                            log_failure("confirm_pending", request_id, &result);
                            ImportResponse::PendingConfirm { request_id, result }
                        }
                        ImportRequest::DismissPending {
                            request_id,
                            request,
                        } => {
                            let result = source.dismiss_pending(request);
                            log_failure("dismiss_pending", request_id, &result);
                            ImportResponse::PendingDismiss { request_id, result }
                        }
                    };
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native provider import worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_discovery(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(ImportRequest::Discover { request_id })
    }

    pub fn request_ccs_import(
        &self,
        request_id: u64,
        request: ImportCcsProviders,
    ) -> Result<(), DispatchError> {
        self.send(ImportRequest::ImportCcs {
            request_id,
            request,
        })
    }

    pub fn request_pending_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(ImportRequest::LoadPending { request_id })
    }

    pub fn request_pending_confirm(
        &self,
        request_id: u64,
        request: ConfirmPendingImport,
    ) -> Result<(), DispatchError> {
        self.send(ImportRequest::ConfirmPending {
            request_id,
            request,
        })
    }

    pub fn request_pending_dismiss(
        &self,
        request_id: u64,
        request: DismissPendingImport,
    ) -> Result<(), DispatchError> {
        self.send(ImportRequest::DismissPending {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<ImportResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: ImportRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn coalesce_discovery(
    receiver: &mpsc::Receiver<ImportRequest>,
    pending: &mut Option<ImportRequest>,
    request_id: &mut u64,
) {
    while let Ok(next) = receiver.try_recv() {
        match next {
            ImportRequest::Discover {
                request_id: next_id,
            } => *request_id = (*request_id).max(next_id),
            other => {
                *pending = Some(other);
                break;
            }
        }
    }
}

fn coalesce_pending_load(
    receiver: &mpsc::Receiver<ImportRequest>,
    pending: &mut Option<ImportRequest>,
    request_id: &mut u64,
) {
    while let Ok(next) = receiver.try_recv() {
        match next {
            ImportRequest::LoadPending {
                request_id: next_id,
            } => *request_id = (*request_id).max(next_id),
            other => {
                *pending = Some(other);
                break;
            }
        }
    }
}

fn log_failure<T>(
    operation: &'static str,
    request_id: u64,
    result: &Result<T, ProviderImportError>,
) {
    let Err(error) = result else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.provider_import_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": request_id,
            "kind": format!("{:?}", error.kind()),
        }),
    );
}
