use std::fmt;
use std::sync::{Arc, mpsc};
use std::thread;

use codex_plus_manager_service::{
    ManagerSettingsError, ManagerSettingsSource, ManagerSettingsWorkspace, ResetExtraArgs,
    ResetImageOverlaySettings, ResetStepwiseSettings, SafeSettingsGroup, SaveExtraArgs,
    SaveImageOverlaySettings, SaveStepwiseSettings, StepwiseTestOutcome, TestStepwiseSettings,
};

use super::{DispatchError, try_receive};

enum SettingsRequest {
    Load {
        request_id: u64,
    },
    SaveStepwise {
        request_id: u64,
        request: SaveStepwiseSettings,
    },
    ResetStepwise {
        request_id: u64,
        request: ResetStepwiseSettings,
    },
    TestStepwise {
        request_id: u64,
        request: TestStepwiseSettings,
    },
    SaveImage {
        request_id: u64,
        request: SaveImageOverlaySettings,
    },
    ResetImage {
        request_id: u64,
        request: ResetImageOverlaySettings,
    },
    SaveExtraArgs {
        request_id: u64,
        request: SaveExtraArgs,
    },
    ResetExtraArgs {
        request_id: u64,
        request: ResetExtraArgs,
    },
}

impl SettingsRequest {
    fn request_id(&self) -> u64 {
        match self {
            Self::Load { request_id }
            | Self::SaveStepwise { request_id, .. }
            | Self::ResetStepwise { request_id, .. }
            | Self::TestStepwise { request_id, .. }
            | Self::SaveImage { request_id, .. }
            | Self::ResetImage { request_id, .. }
            | Self::SaveExtraArgs { request_id, .. }
            | Self::ResetExtraArgs { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Load { .. } => "load",
            Self::SaveStepwise { .. } => "save_stepwise",
            Self::ResetStepwise { .. } => "reset_stepwise",
            Self::TestStepwise { .. } => "test_stepwise",
            Self::SaveImage { .. } => "save_image",
            Self::ResetImage { .. } => "reset_image",
            Self::SaveExtraArgs { .. } => "save_extra_args",
            Self::ResetExtraArgs { .. } => "reset_extra_args",
        }
    }

    fn group(&self) -> Option<SafeSettingsGroup> {
        match self {
            Self::Load { .. } => None,
            Self::SaveStepwise { .. } | Self::ResetStepwise { .. } | Self::TestStepwise { .. } => {
                Some(SafeSettingsGroup::Stepwise)
            }
            Self::SaveImage { .. } | Self::ResetImage { .. } => {
                Some(SafeSettingsGroup::ImageOverlay)
            }
            Self::SaveExtraArgs { .. } | Self::ResetExtraArgs { .. } => {
                Some(SafeSettingsGroup::ExtraArgs)
            }
        }
    }
}

impl fmt::Debug for SettingsRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsRequest")
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .field("group", &self.group())
            .finish()
    }
}

pub enum SettingsResponse {
    Loaded {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    StepwiseSaved {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    StepwiseReset {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    StepwiseTested {
        request_id: u64,
        result: Result<StepwiseTestOutcome, ManagerSettingsError>,
    },
    ImageSaved {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    ImageReset {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    ExtraArgsSaved {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
    ExtraArgsReset {
        request_id: u64,
        result: Result<Arc<ManagerSettingsWorkspace>, ManagerSettingsError>,
    },
}

impl SettingsResponse {
    pub fn request_id(&self) -> u64 {
        match self {
            Self::Loaded { request_id, .. }
            | Self::StepwiseSaved { request_id, .. }
            | Self::StepwiseReset { request_id, .. }
            | Self::StepwiseTested { request_id, .. }
            | Self::ImageSaved { request_id, .. }
            | Self::ImageReset { request_id, .. }
            | Self::ExtraArgsSaved { request_id, .. }
            | Self::ExtraArgsReset { request_id, .. } => *request_id,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "load",
            Self::StepwiseSaved { .. } => "save_stepwise",
            Self::StepwiseReset { .. } => "reset_stepwise",
            Self::StepwiseTested { .. } => "test_stepwise",
            Self::ImageSaved { .. } => "save_image",
            Self::ImageReset { .. } => "reset_image",
            Self::ExtraArgsSaved { .. } => "save_extra_args",
            Self::ExtraArgsReset { .. } => "reset_extra_args",
        }
    }

    fn event(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "native.settings.load",
            Self::StepwiseSaved { .. } => "native.settings.save_stepwise",
            Self::StepwiseReset { .. } => "native.settings.reset_stepwise",
            Self::StepwiseTested { .. } => "native.settings.test_stepwise",
            Self::ImageSaved { .. } => "native.settings.save_image",
            Self::ImageReset { .. } => "native.settings.reset_image",
            Self::ExtraArgsSaved { .. } => "native.settings.save_extra_args",
            Self::ExtraArgsReset { .. } => "native.settings.reset_extra_args",
        }
    }

    fn group(&self) -> Option<SafeSettingsGroup> {
        match self {
            Self::Loaded { .. } => None,
            Self::StepwiseSaved { .. }
            | Self::StepwiseReset { .. }
            | Self::StepwiseTested { .. } => Some(SafeSettingsGroup::Stepwise),
            Self::ImageSaved { .. } | Self::ImageReset { .. } => {
                Some(SafeSettingsGroup::ImageOverlay)
            }
            Self::ExtraArgsSaved { .. } | Self::ExtraArgsReset { .. } => {
                Some(SafeSettingsGroup::ExtraArgs)
            }
        }
    }

    fn error(&self) -> Option<&ManagerSettingsError> {
        match self {
            Self::Loaded { result, .. }
            | Self::StepwiseSaved { result, .. }
            | Self::StepwiseReset { result, .. }
            | Self::ImageSaved { result, .. }
            | Self::ImageReset { result, .. }
            | Self::ExtraArgsSaved { result, .. }
            | Self::ExtraArgsReset { result, .. } => result.as_ref().err(),
            Self::StepwiseTested { result, .. } => result.as_ref().err(),
        }
    }
}

impl fmt::Debug for SettingsResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsResponse")
            .field("operation", &self.operation())
            .field("request_id", &self.request_id())
            .field("group", &self.group())
            .field("success", &self.error().is_none())
            .field("error_kind", &self.error().map(ManagerSettingsError::kind))
            .finish()
    }
}

pub struct SettingsDispatcher {
    requests: mpsc::Sender<SettingsRequest>,
    responses: mpsc::Receiver<SettingsResponse>,
}

impl SettingsDispatcher {
    pub fn spawn(
        source: Arc<dyn ManagerSettingsSource>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        thread::Builder::new()
            .name("native-settings-worker".to_owned())
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
                        SettingsRequest::Load { mut request_id } => {
                            while let Ok(next) = request_rx.try_recv() {
                                match next {
                                    SettingsRequest::Load {
                                        request_id: next_id,
                                    } => request_id = request_id.max(next_id),
                                    ordered => {
                                        pending = Some(ordered);
                                        break;
                                    }
                                }
                            }
                            SettingsResponse::Loaded {
                                request_id,
                                result: source.load_workspace().map(Arc::new),
                            }
                        }
                        SettingsRequest::SaveStepwise {
                            request_id,
                            request,
                        } => SettingsResponse::StepwiseSaved {
                            request_id,
                            result: source.save_stepwise(request).map(Arc::new),
                        },
                        SettingsRequest::ResetStepwise {
                            request_id,
                            request,
                        } => SettingsResponse::StepwiseReset {
                            request_id,
                            result: source.reset_stepwise(request).map(Arc::new),
                        },
                        SettingsRequest::TestStepwise {
                            request_id,
                            request,
                        } => SettingsResponse::StepwiseTested {
                            request_id,
                            result: source.test_stepwise(request),
                        },
                        SettingsRequest::SaveImage {
                            request_id,
                            request,
                        } => SettingsResponse::ImageSaved {
                            request_id,
                            result: source.save_image_overlay(request).map(Arc::new),
                        },
                        SettingsRequest::ResetImage {
                            request_id,
                            request,
                        } => SettingsResponse::ImageReset {
                            request_id,
                            result: source.reset_image_overlay(request).map(Arc::new),
                        },
                        SettingsRequest::SaveExtraArgs {
                            request_id,
                            request,
                        } => SettingsResponse::ExtraArgsSaved {
                            request_id,
                            result: source.save_extra_args(request).map(Arc::new),
                        },
                        SettingsRequest::ResetExtraArgs {
                            request_id,
                            request,
                        } => SettingsResponse::ExtraArgsReset {
                            request_id,
                            result: source.reset_extra_args(request).map(Arc::new),
                        },
                    };
                    log_failure(&response);
                    if response_tx.send(response).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .expect("spawn native settings worker");
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    pub fn request_load(&self, request_id: u64) -> Result<(), DispatchError> {
        self.send(SettingsRequest::Load { request_id })
    }

    pub fn request_save_stepwise(
        &self,
        request_id: u64,
        request: SaveStepwiseSettings,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::SaveStepwise {
            request_id,
            request,
        })
    }

    pub fn request_reset_stepwise(
        &self,
        request_id: u64,
        request: ResetStepwiseSettings,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::ResetStepwise {
            request_id,
            request,
        })
    }

    pub fn request_test_stepwise(
        &self,
        request_id: u64,
        request: TestStepwiseSettings,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::TestStepwise {
            request_id,
            request,
        })
    }

    pub fn request_save_image(
        &self,
        request_id: u64,
        request: SaveImageOverlaySettings,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::SaveImage {
            request_id,
            request,
        })
    }

    pub fn request_reset_image(
        &self,
        request_id: u64,
        request: ResetImageOverlaySettings,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::ResetImage {
            request_id,
            request,
        })
    }

    pub fn request_save_args(
        &self,
        request_id: u64,
        request: SaveExtraArgs,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::SaveExtraArgs {
            request_id,
            request,
        })
    }

    pub fn request_reset_args(
        &self,
        request_id: u64,
        request: ResetExtraArgs,
    ) -> Result<(), DispatchError> {
        self.send(SettingsRequest::ResetExtraArgs {
            request_id,
            request,
        })
    }

    pub fn try_recv(&self) -> Result<Option<SettingsResponse>, DispatchError> {
        try_receive(&self.responses)
    }

    fn send(&self, request: SettingsRequest) -> Result<(), DispatchError> {
        self.requests
            .send(request)
            .map_err(|_| DispatchError::WorkerStopped)
    }
}

fn log_failure(response: &SettingsResponse) {
    let Some(error) = response.error() else {
        return;
    };
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        response.event(),
        serde_json::json!({
            "operation": response.operation(),
            "request_id": response.request_id(),
            "group": response.group().map(|group| format!("{group:?}")),
            "success": false,
            "error_kind": format!("{:?}", error.kind()),
        }),
    );
}
