use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    DiagnoseProviderProfile, FetchProviderModels, ProviderDoctorReport, ProviderError,
    ProviderModelsResult, ProviderNetworkError, ProviderSource, ProviderTestResult,
    ProviderWorkspace, SaveProviderWorkspace, TestProviderProfile,
};

use super::{DispatchError, try_receive};
use crate::state::provider::OperationToken;

enum StoreRequest {
    Load {
        request_id: u64,
    },
    Save {
        request_id: u64,
        request: SaveProviderWorkspace,
    },
}

#[derive(Debug)]
pub enum StoreResponse {
    Load {
        request_id: u64,
        result: Result<Arc<ProviderWorkspace>, ProviderError>,
    },
    Save {
        request_id: u64,
        result: Result<Arc<ProviderWorkspace>, ProviderError>,
    },
}

impl StoreResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Load { request_id, .. } | Self::Save { request_id, .. } => *request_id,
        }
    }
}

struct TestRequest {
    token: OperationToken,
    request: TestProviderProfile,
}

struct ModelsRequest {
    token: OperationToken,
    request: FetchProviderModels,
}

struct DoctorRequest {
    token: OperationToken,
    request: DiagnoseProviderProfile,
}

#[derive(Debug)]
pub struct TestResponse {
    pub token: OperationToken,
    pub result: Result<ProviderTestResult, ProviderNetworkError>,
}

#[derive(Debug)]
pub struct ModelsResponse {
    pub token: OperationToken,
    pub result: Result<ProviderModelsResult, ProviderNetworkError>,
}

#[derive(Debug)]
pub struct DoctorResponse {
    pub token: OperationToken,
    pub result: Result<ProviderDoctorReport, ProviderNetworkError>,
}

pub struct ProviderDispatcher {
    store_requests: mpsc::Sender<StoreRequest>,
    store_responses: mpsc::Receiver<StoreResponse>,
    test_requests: mpsc::Sender<TestRequest>,
    test_responses: mpsc::Receiver<TestResponse>,
    models_requests: mpsc::Sender<ModelsRequest>,
    models_responses: mpsc::Receiver<ModelsResponse>,
    doctor_requests: mpsc::Sender<DoctorRequest>,
    doctor_responses: mpsc::Receiver<DoctorResponse>,
}

impl ProviderDispatcher {
    pub fn spawn(source: Arc<dyn ProviderSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (store_request_tx, store_request_rx) = mpsc::channel();
        let (store_response_tx, store_response_rx) = mpsc::channel();
        let store_source = Arc::clone(&source);
        let store_wake = Arc::clone(&wake);
        thread::Builder::new()
            .name("native-provider-store".to_string())
            .spawn(move || {
                let mut pending = None;
                loop {
                    let request = match pending.take() {
                        Some(request) => request,
                        None => match store_request_rx.recv() {
                            Ok(request) => request,
                            Err(_) => break,
                        },
                    };
                    let response = match request {
                        StoreRequest::Load { mut request_id } => {
                            while let Ok(next) = store_request_rx.try_recv() {
                                match next {
                                    StoreRequest::Load {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    save @ StoreRequest::Save { .. } => {
                                        pending = Some(save);
                                        break;
                                    }
                                }
                            }
                            let result = store_source.load_workspace().map(Arc::new);
                            if let Err(error) = &result {
                                log_store_failure("load", error);
                            }
                            StoreResponse::Load { request_id, result }
                        }
                        StoreRequest::Save {
                            request_id,
                            request,
                        } => {
                            let result = store_source.save_workspace(request).map(Arc::new);
                            if let Err(error) = &result {
                                log_store_failure("save", error);
                            }
                            StoreResponse::Save { request_id, result }
                        }
                    };
                    if store_response_tx.send(response).is_err() {
                        break;
                    }
                    store_wake();
                }
            })
            .expect("spawn native provider store worker");

        let (test_request_tx, test_request_rx) = mpsc::channel::<TestRequest>();
        let (test_response_tx, test_response_rx) = mpsc::channel();
        let test_source = Arc::clone(&source);
        let test_wake = Arc::clone(&wake);
        thread::Builder::new()
            .name("native-provider-test".to_string())
            .spawn(move || {
                while let Ok(request) = test_request_rx.recv() {
                    let result = test_source.test_profile(request.request);
                    if let Err(error) = &result {
                        log_network_failure("test", error);
                    }
                    if test_response_tx
                        .send(TestResponse {
                            token: request.token,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    test_wake();
                }
            })
            .expect("spawn native provider test worker");

        let (models_request_tx, models_request_rx) = mpsc::channel::<ModelsRequest>();
        let (models_response_tx, models_response_rx) = mpsc::channel();
        let models_source = Arc::clone(&source);
        let models_wake = Arc::clone(&wake);
        thread::Builder::new()
            .name("native-provider-models".to_string())
            .spawn(move || {
                while let Ok(request) = models_request_rx.recv() {
                    let result = models_source.fetch_models(request.request);
                    if let Err(error) = &result {
                        log_network_failure("models", error);
                    }
                    if models_response_tx
                        .send(ModelsResponse {
                            token: request.token,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    models_wake();
                }
            })
            .expect("spawn native provider models worker");

        let (doctor_request_tx, doctor_request_rx) = mpsc::channel::<DoctorRequest>();
        let (doctor_response_tx, doctor_response_rx) = mpsc::channel();
        let doctor_source = source;
        let doctor_wake = wake;
        thread::Builder::new()
            .name("native-provider-doctor".to_string())
            .spawn(move || {
                while let Ok(request) = doctor_request_rx.recv() {
                    let result = doctor_source.diagnose_profile(request.request);
                    if let Err(error) = &result {
                        log_network_failure("doctor", error);
                    }
                    if doctor_response_tx
                        .send(DoctorResponse {
                            token: request.token,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    doctor_wake();
                }
            })
            .expect("spawn native provider doctor worker");

        Self {
            store_requests: store_request_tx,
            store_responses: store_response_rx,
            test_requests: test_request_tx,
            test_responses: test_response_rx,
            models_requests: models_request_tx,
            models_responses: models_response_rx,
            doctor_requests: doctor_request_tx,
            doctor_responses: doctor_response_rx,
        }
    }

    pub fn request_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.store_requests
            .send(StoreRequest::Load { request_id })
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn request_save(
        &self,
        request_id: u64,
        request: SaveProviderWorkspace,
    ) -> Result<(), DispatchError> {
        self.store_requests
            .send(StoreRequest::Save {
                request_id,
                request,
            })
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn request_test(
        &self,
        token: OperationToken,
        request: TestProviderProfile,
    ) -> Result<(), DispatchError> {
        self.test_requests
            .send(TestRequest { token, request })
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn request_models(
        &self,
        token: OperationToken,
        request: FetchProviderModels,
    ) -> Result<(), DispatchError> {
        self.models_requests
            .send(ModelsRequest { token, request })
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn request_doctor(
        &self,
        token: OperationToken,
        request: DiagnoseProviderProfile,
    ) -> Result<(), DispatchError> {
        self.doctor_requests
            .send(DoctorRequest { token, request })
            .map_err(|_| DispatchError::WorkerStopped)
    }

    pub fn try_recv_store(&self) -> Result<Option<StoreResponse>, DispatchError> {
        try_receive(&self.store_responses)
    }

    pub fn try_recv_test(&self) -> Result<Option<TestResponse>, DispatchError> {
        try_receive(&self.test_responses)
    }

    pub fn try_recv_models(&self) -> Result<Option<ModelsResponse>, DispatchError> {
        try_receive(&self.models_responses)
    }

    pub fn try_recv_doctor(&self) -> Result<Option<DoctorResponse>, DispatchError> {
        try_receive(&self.doctor_responses)
    }
}

fn log_store_failure(operation: &str, error: &ProviderError) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.provider_store_failed",
        serde_json::json!({
            "operation": operation,
            "kind": format!("{:?}", error.kind()),
        }),
    );
}

fn log_network_failure(operation: &str, error: &ProviderNetworkError) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "native_manager.provider_operation_failed",
        serde_json::json!({
            "operation": operation,
            "kind": format!("{:?}", error.kind()),
            "httpStatus": error.http_status(),
            "endpoint": error.endpoint().map(|endpoint| endpoint.as_str()),
        }),
    );
}
