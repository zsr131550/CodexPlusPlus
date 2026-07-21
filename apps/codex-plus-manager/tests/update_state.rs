use std::io::Write;
use std::sync::Arc;

use codex_plus_core::update::UpdateTarget;
use codex_plus_manager::state::update::{
    UpdateFailureKind, UpdateInstallEffect, UpdatePhase, UpdateViewState,
};
use codex_plus_manager_service::{
    UpdateAvailability, UpdateCheckResult, UpdateDownload, UpdateEnvironment,
    UpdateEnvironmentError, UpdateProgress, UpdateService,
};

struct CandidateEnvironment;

impl UpdateEnvironment for CandidateEnvironment {
    type Artifact = Vec<u8>;

    fn current_version(&self) -> String {
        "1.0.0".to_owned()
    }

    fn target(&self) -> UpdateTarget {
        UpdateTarget::WindowsX64
    }

    fn fetch_release_metadata(
        &self,
        _maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        Ok(br#"{
            "version": "2.0.0",
            "notes": "safe summary",
            "pub_date": "2026-07-20T00:00:00Z",
            "platforms": {},
            "assets": [{
                "name": "CodexPlusPlus-2.0.0-setup.exe",
                "browser_download_url": "https://updates.invalid/CodexPlusPlus-2.0.0-setup.exe"
            }]
        }"#
        .to_vec())
    }

    fn open_asset_download(&self, _url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        panic!("state tests never download")
    }

    fn create_update_artifact(
        &self,
        _safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        panic!("state tests never create artifacts")
    }

    fn publish_update_artifact(
        &self,
        _artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("state tests never publish artifacts")
    }

    fn cleanup_update_artifact(&self, _artifact: &mut Self::Artifact) {}

    fn launch_update_artifact(
        &self,
        _artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("state tests never launch artifacts")
    }
}

fn available_result() -> Arc<UpdateCheckResult> {
    Arc::new(UpdateService::new(CandidateEnvironment).check().unwrap())
}

fn current_result(version: &str) -> Arc<UpdateCheckResult> {
    Arc::new(UpdateCheckResult {
        installed_version: version.to_owned(),
        latest_version: version.to_owned(),
        summary: "last good".to_owned(),
        availability: UpdateAvailability::Current,
    })
}

#[test]
fn update_check_coalesces_and_preserves_last_good_when_retry_fails() {
    let mut state = UpdateViewState::default();

    let first = state.begin_check(true).unwrap();
    assert_eq!(state.phase, UpdatePhase::Checking);
    assert!(state.check_is_silent());
    assert_eq!(state.begin_check(false), None);

    let good = current_result("1.0.0");
    assert!(state.apply_check_response(first, Ok(Arc::clone(&good))));
    assert_eq!(state.phase, UpdatePhase::Current);

    let retry = state.begin_check(false).unwrap();
    assert!(!state.check_is_silent());
    assert!(!state.apply_check_response(first, Ok(current_result("9.9.9"))));
    assert!(state.apply_check_response(retry, Err(UpdateFailureKind::MetadataFetchFailed)));
    assert_eq!(state.phase, UpdatePhase::Error);
    assert!(Arc::ptr_eq(state.result.as_ref().unwrap(), &good));
}

#[test]
fn update_confirmation_freezes_version_and_cancel_has_no_side_effect() {
    let mut state = UpdateViewState::default();
    let request = state.begin_check(false).unwrap();
    state.apply_check_response(request, Ok(available_result()));
    assert_eq!(state.phase, UpdatePhase::Available);

    assert!(state.request_install_confirmation());
    assert_eq!(state.confirmation_version(), Some("2.0.0"));
    state.cancel_install_confirmation();
    assert_eq!(state.phase, UpdatePhase::Available);
    assert_eq!(state.confirmation_version(), None);

    assert!(state.request_install_confirmation());
    let (install_id, install) = state.confirm_install().unwrap();
    assert_eq!(install.confirmed_version, "2.0.0");
    assert_eq!(state.phase, UpdatePhase::Downloading);
    assert!(state.confirm_install().is_none());

    assert!(!state.apply_progress(
        install_id + 1,
        UpdateProgress {
            downloaded_bytes: 99,
            total_bytes: Some(100),
        },
    ));
    assert!(state.apply_progress(
        install_id,
        UpdateProgress {
            downloaded_bytes: 40,
            total_bytes: Some(100),
        },
    ));
    assert!(!state.apply_progress(
        install_id,
        UpdateProgress {
            downloaded_bytes: 39,
            total_bytes: Some(100),
        },
    ));
}

#[test]
fn successful_launch_requests_exactly_one_explicit_exit() {
    let mut state = UpdateViewState::default();
    let check = state.begin_check(false).unwrap();
    state.apply_check_response(check, Ok(available_result()));
    state.request_install_confirmation();
    let (install_id, _) = state.confirm_install().unwrap();

    let started = codex_plus_manager_service::InstallStarted {
        version: "2.0.0".to_owned(),
    };
    assert_eq!(
        state.apply_install_response(install_id, Ok(started.clone())),
        UpdateInstallEffect::RequestExit
    );
    assert_eq!(state.phase, UpdatePhase::Launching);
    assert_eq!(
        state.apply_install_response(install_id, Ok(started)),
        UpdateInstallEffect::Ignored
    );
}

#[test]
fn failed_install_consumes_candidate_until_a_fresh_check() {
    let mut state = UpdateViewState::default();
    let check = state.begin_check(false).unwrap();
    state.apply_check_response(check, Ok(available_result()));
    state.request_install_confirmation();
    let (install_id, _) = state.confirm_install().unwrap();

    assert_eq!(
        state.apply_install_response(install_id, Err(UpdateFailureKind::DownloadFailed)),
        UpdateInstallEffect::Failed
    );
    assert_eq!(state.phase, UpdatePhase::Error);
    assert!(!state.request_install_confirmation());

    let refresh = state.begin_check(false).unwrap();
    state.apply_check_response(refresh, Ok(available_result()));
    assert!(state.request_install_confirmation());
}

#[allow(dead_code)]
fn artifact_is_write(_artifact: &mut impl Write) {}
