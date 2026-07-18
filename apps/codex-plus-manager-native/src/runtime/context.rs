use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    ContextBundle, ContextEntryDraft, ContextSyncOutcome, ContextSyncPreview, ContextToolsError,
    ContextToolsSource, DeleteContextEntry, LoadContextEntryDraft, PreviewContextSync,
    SaveContextEntry, SetContextEntryEnabled, SyncContextToLive,
};

use super::{DispatchError, try_receive};

enum ContextRequest {
    LoadWorkspace {
        request_id: u64,
    },
    LoadDraft {
        request_id: u64,
        request: LoadContextEntryDraft,
    },
    Save {
        request_id: u64,
        request: SaveContextEntry,
    },
    Toggle {
        request_id: u64,
        request: SetContextEntryEnabled,
    },
    Delete {
        request_id: u64,
        request: DeleteContextEntry,
    },
    Preview {
        request_id: u64,
        request: PreviewContextSync,
    },
    Sync {
        request_id: u64,
        request: SyncContextToLive,
    },
}

#[derive(Debug)]
pub enum ContextResponse {
    Workspace {
        request_id: u64,
        result: Result<Arc<ContextBundle>, ContextToolsError>,
    },
    Draft {
        request_id: u64,
        result: Result<Arc<ContextEntryDraft>, ContextToolsError>,
    },
    StoredMutation {
        request_id: u64,
        result: Result<Arc<ContextBundle>, ContextToolsError>,
    },
    Preview {
        request_id: u64,
        result: Result<Arc<ContextSyncPreview>, ContextToolsError>,
    },
    Sync {
        request_id: u64,
        result: Result<Arc<ContextSyncOutcome>, ContextToolsError>,
    },
}

impl ContextResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Workspace { request_id, .. }
            | Self::Draft { request_id, .. }
            | Self::StoredMutation { request_id, .. }
            | Self::Preview { request_id, .. }
            | Self::Sync { request_id, .. } => *request_id,
        }
    }
}

pub struct ContextDispatcher {
    requests: mpsc::Sender<ContextRequest>,
    responses: mpsc::Receiver<ContextResponse>,
}

impl ContextDispatcher {
    pub fn spawn(source: Arc<dyn ContextToolsSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-context-tools".to_owned())
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
                    let (operation, response) = match request {
                        ContextRequest::LoadWorkspace { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    ContextRequest::LoadWorkspace {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            (
                                "workspace",
                                ContextResponse::Workspace {
                                    request_id,
                                    result: source.load_workspace().map(Arc::new),
                                },
                            )
                        }
                        ContextRequest::LoadDraft {
                            request_id,
                            request,
                        } => (
                            "draft",
                            ContextResponse::Draft {
                                request_id,
                                result: source.load_entry_draft(request).map(Arc::new),
                            },
                        ),
                        ContextRequest::Save {
                            request_id,
                            request,
                        } => (
                            "save",
                            ContextResponse::StoredMutation {
                                request_id,
                                result: source.save_entry(request).map(Arc::new),
                            },
                        ),
                        ContextRequest::Toggle {
                            request_id,
                            request,
                        } => (
                            "toggle",
                            ContextResponse::StoredMutation {
                                request_id,
                                result: source.set_entry_enabled(request).map(Arc::new),
                            },
                        ),
                        ContextRequest::Delete {
                            request_id,
                            request,
                        } => (
                            "delete",
                            ContextResponse::StoredMutation {
                                request_id,
                                result: source.delete_entry(request).map(Arc::new),
                            },
                        ),
                        ContextRequest::Preview {
                            request_id,
                            request,
                        } => (
                            "preview",
                            ContextResponse::Preview {
                                request_id,
                                result: source.preview_context_sync(request).map(Arc::new),
                            },
                        ),
                        ContextRequest::Sync {
                            request_id,
                            request,
                        } => (
                            "sync",
                            ContextResponse::Sync {
                                request_id,
                                result: source.sync_context_to_live(request).map(Arc::new),
                            },
                        ),
                    };
                    log_failure(operation, &response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native context tools worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_workspace(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(ContextRequest::LoadWorkspace { request_id })
    }

    pub fn request_draft(
        &self,
        request_id: u64,
        request: LoadContextEntryDraft,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::LoadDraft {
            request_id,
            request,
        })
    }

    pub fn request_save(
        &self,
        request_id: u64,
        request: SaveContextEntry,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::Save {
            request_id,
            request,
        })
    }

    pub fn request_toggle(
        &self,
        request_id: u64,
        request: SetContextEntryEnabled,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::Toggle {
            request_id,
            request,
        })
    }

    pub fn request_delete(
        &self,
        request_id: u64,
        request: DeleteContextEntry,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::Delete {
            request_id,
            request,
        })
    }

    pub fn request_preview(
        &self,
        request_id: u64,
        request: PreviewContextSync,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::Preview {
            request_id,
            request,
        })
    }

    pub fn request_sync(
        &self,
        request_id: u64,
        request: SyncContextToLive,
    ) -> Result<(), DispatchError> {
        self.send(ContextRequest::Sync {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<ContextResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: ContextRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(operation: &'static str, response: &ContextResponse) {
    let error = match response {
        ContextResponse::Workspace { result, .. }
        | ContextResponse::StoredMutation { result, .. } => result.as_ref().err(),
        ContextResponse::Draft { result, .. } => result.as_ref().err(),
        ContextResponse::Preview { result, .. } => result.as_ref().err(),
        ContextResponse::Sync { result, .. } => result.as_ref().err(),
    };
    let Some(error) = error else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.context_tools_failed",
        serde_json::json!({
            "operation": operation,
            "request_id": response.request_id(),
            "kind": format!("{:?}", error.kind()),
        }),
    );
}
