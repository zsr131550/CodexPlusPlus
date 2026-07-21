use std::sync::Arc;

use codex_plus_manager_service::{
    InstallStarted, InstallUpdate, UpdateAvailability, UpdateCheckResult, UpdateErrorKind,
    UpdateProgress,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Launching,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFailureKind {
    MetadataFetchFailed,
    MetadataTooLarge,
    MetadataInvalid,
    InvalidAsset,
    InsecureAsset,
    InvalidRevision,
    ConfirmationRequired,
    DownloadFailed,
    DownloadTooLarge,
    WriteFailed,
    PublishFailed,
    LaunchFailed,
    WorkerStopped,
    NoCompatibleAsset,
}

impl From<UpdateErrorKind> for UpdateFailureKind {
    fn from(kind: UpdateErrorKind) -> Self {
        match kind {
            UpdateErrorKind::MetadataFetchFailed => Self::MetadataFetchFailed,
            UpdateErrorKind::MetadataTooLarge => Self::MetadataTooLarge,
            UpdateErrorKind::MetadataInvalid => Self::MetadataInvalid,
            UpdateErrorKind::InvalidAsset => Self::InvalidAsset,
            UpdateErrorKind::InsecureAsset => Self::InsecureAsset,
            UpdateErrorKind::InvalidRevision => Self::InvalidRevision,
            UpdateErrorKind::ConfirmationRequired => Self::ConfirmationRequired,
            UpdateErrorKind::DownloadFailed => Self::DownloadFailed,
            UpdateErrorKind::DownloadTooLarge => Self::DownloadTooLarge,
            UpdateErrorKind::WriteFailed => Self::WriteFailed,
            UpdateErrorKind::PublishFailed => Self::PublishFailed,
            UpdateErrorKind::LaunchFailed => Self::LaunchFailed,
            UpdateErrorKind::WorkerStopped => Self::WorkerStopped,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateInstallEffect {
    Ignored,
    Failed,
    RequestExit,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateViewState {
    pub phase: UpdatePhase,
    pub result: Option<Arc<UpdateCheckResult>>,
    pub error: Option<UpdateFailureKind>,
    pub progress: Option<UpdateProgress>,
    current_check_id: Option<u64>,
    current_install_id: Option<u64>,
    current_check_silent: bool,
    confirmation: Option<codex_plus_manager_service::UpdateCandidate>,
    installing_version: Option<String>,
    candidate_consumed: bool,
    next_request_id: u64,
}

impl UpdateViewState {
    pub fn begin_check(&mut self, silent: bool) -> Option<u64> {
        if self.current_check_id.is_some()
            || self.current_install_id.is_some()
            || self.phase == UpdatePhase::Launching
        {
            return None;
        }
        let request_id = self.next_request_id();
        self.current_check_id = Some(request_id);
        self.current_check_silent = silent;
        self.confirmation = None;
        self.phase = UpdatePhase::Checking;
        Some(request_id)
    }

    pub fn check_is_silent(&self) -> bool {
        self.current_check_id.is_some() && self.current_check_silent
    }

    pub fn apply_check_response(
        &mut self,
        request_id: u64,
        result: Result<Arc<UpdateCheckResult>, UpdateFailureKind>,
    ) -> bool {
        if self.current_check_id != Some(request_id) {
            return false;
        }
        self.current_check_id = None;
        self.current_check_silent = false;
        match result {
            Ok(result) => {
                self.error = None;
                self.candidate_consumed = false;
                self.phase = match result.availability {
                    UpdateAvailability::Current => UpdatePhase::Current,
                    UpdateAvailability::Available(_) => UpdatePhase::Available,
                    UpdateAvailability::Unavailable => {
                        self.error = Some(UpdateFailureKind::NoCompatibleAsset);
                        UpdatePhase::Error
                    }
                };
                self.result = Some(result);
            }
            Err(error) => {
                self.error = Some(error);
                self.phase = UpdatePhase::Error;
            }
        }
        true
    }

    pub fn request_install_confirmation(&mut self) -> bool {
        if self.phase != UpdatePhase::Available || self.candidate_consumed {
            return false;
        }
        let Some(candidate) = self.result.as_ref().and_then(|result| {
            if let UpdateAvailability::Available(candidate) = &result.availability {
                Some(candidate.clone())
            } else {
                None
            }
        }) else {
            return false;
        };
        self.confirmation = Some(candidate);
        true
    }

    pub fn confirmation_version(&self) -> Option<&str> {
        self.confirmation
            .as_ref()
            .map(|candidate| candidate.version.as_str())
    }

    pub fn cancel_install_confirmation(&mut self) {
        self.confirmation = None;
    }

    pub fn confirm_install(&mut self) -> Option<(u64, InstallUpdate)> {
        if self.phase != UpdatePhase::Available || self.candidate_consumed {
            return None;
        }
        let candidate = self.confirmation.take()?;
        let request_id = self.next_request_id();
        self.current_install_id = Some(request_id);
        self.installing_version = Some(candidate.version.clone());
        self.candidate_consumed = true;
        self.progress = None;
        self.error = None;
        self.phase = UpdatePhase::Downloading;
        Some((
            request_id,
            InstallUpdate {
                revision: candidate.revision,
                confirmed_version: candidate.version,
            },
        ))
    }

    pub fn apply_progress(&mut self, request_id: u64, progress: UpdateProgress) -> bool {
        if self.current_install_id != Some(request_id) || self.phase != UpdatePhase::Downloading {
            return false;
        }
        if progress
            .total_bytes
            .is_some_and(|total| progress.downloaded_bytes > total)
        {
            return false;
        }
        if let Some(previous) = self.progress
            && (progress.downloaded_bytes < previous.downloaded_bytes
                || (previous.total_bytes.is_some() && progress.total_bytes != previous.total_bytes))
        {
            return false;
        }
        self.progress = Some(progress);
        true
    }

    pub fn apply_install_response(
        &mut self,
        request_id: u64,
        result: Result<InstallStarted, UpdateFailureKind>,
    ) -> UpdateInstallEffect {
        if self.current_install_id != Some(request_id) || self.phase != UpdatePhase::Downloading {
            return UpdateInstallEffect::Ignored;
        }
        self.current_install_id = None;
        self.progress = None;
        match result {
            Ok(started) if self.installing_version.as_deref() == Some(started.version.as_str()) => {
                self.installing_version = None;
                self.error = None;
                self.phase = UpdatePhase::Launching;
                UpdateInstallEffect::RequestExit
            }
            Ok(_) => {
                self.installing_version = None;
                self.error = Some(UpdateFailureKind::InvalidRevision);
                self.phase = UpdatePhase::Error;
                UpdateInstallEffect::Failed
            }
            Err(error) => {
                self.installing_version = None;
                self.error = Some(error);
                self.phase = UpdatePhase::Error;
                UpdateInstallEffect::Failed
            }
        }
    }

    pub fn fail_worker(&mut self) {
        if let Some(request_id) = self.current_check_id {
            let _ = self.apply_check_response(request_id, Err(UpdateFailureKind::WorkerStopped));
        } else if let Some(request_id) = self.current_install_id {
            let _ = self.apply_install_response(request_id, Err(UpdateFailureKind::WorkerStopped));
        }
    }

    pub fn available_candidate(&self) -> Option<&codex_plus_manager_service::UpdateCandidate> {
        if self.candidate_consumed {
            return None;
        }
        self.result.as_ref().and_then(|result| {
            if let UpdateAvailability::Available(candidate) = &result.availability {
                Some(candidate)
            } else {
                None
            }
        })
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("update request id overflow");
        self.next_request_id
    }
}
