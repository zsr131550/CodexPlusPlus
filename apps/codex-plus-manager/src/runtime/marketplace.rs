use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    PluginMarketplaceError, PluginMarketplaceKind, PluginMarketplaceRepair,
    PluginMarketplaceSource, PluginMarketplaceWorkspace, RepairPluginMarketplace,
};

use super::{DispatchError, try_receive};

enum MarketplaceRequest {
    Inspect {
        request_id: u64,
    },
    Repair {
        request_id: u64,
        request: RepairPluginMarketplace,
    },
}

#[derive(Debug)]
pub enum MarketplaceResponse {
    Inspected {
        request_id: u64,
        result: Result<Arc<PluginMarketplaceWorkspace>, PluginMarketplaceError>,
    },
    Repaired {
        request_id: u64,
        kind: PluginMarketplaceKind,
        result: Result<Arc<PluginMarketplaceRepair>, PluginMarketplaceError>,
    },
}

impl MarketplaceResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Inspected { request_id, .. } | Self::Repaired { request_id, .. } => *request_id,
        }
    }
}

pub struct MarketplaceDispatcher {
    requests: mpsc::Sender<MarketplaceRequest>,
    responses: mpsc::Receiver<MarketplaceResponse>,
}

impl MarketplaceDispatcher {
    pub fn spawn(
        source: Arc<dyn PluginMarketplaceSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-plugin-marketplace".to_owned())
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
                        MarketplaceRequest::Inspect { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    MarketplaceRequest::Inspect {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            MarketplaceResponse::Inspected {
                                request_id,
                                result: source.inspect().map(Arc::new),
                            }
                        }
                        MarketplaceRequest::Repair {
                            request_id,
                            request,
                        } => {
                            let kind = request.kind;
                            MarketplaceResponse::Repaired {
                                request_id,
                                kind,
                                result: source.repair(request).map(Arc::new),
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
            .expect("spawn native plugin marketplace worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_inspection(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(MarketplaceRequest::Inspect { request_id })
    }

    pub fn request_repair(
        &self,
        request_id: u64,
        request: RepairPluginMarketplace,
    ) -> Result<(), DispatchError> {
        self.send(MarketplaceRequest::Repair {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<MarketplaceResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: MarketplaceRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(response: &MarketplaceResponse) {
    let (operation, kind, error) = match response {
        MarketplaceResponse::Inspected { result, .. } => ("inspect", None, result.as_ref().err()),
        MarketplaceResponse::Repaired { kind, result, .. } => {
            ("repair", Some(*kind), result.as_ref().err())
        }
    };
    let Some(error) = error else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.plugin_marketplace_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": response.request_id(),
            "marketplace_kind": kind.map(|kind| format!("{kind:?}")),
            "error_kind": format!("{:?}", error.kind()),
            "http_status": error.http_status(),
        }),
    );
}
