use std::sync::Arc;

use codex_plus_core::update::UpdateTarget;
use codex_plus_manager::i18n::{Locale, ThemeMode};
use codex_plus_manager::state::update::{UpdateFailureKind, UpdatePhase, UpdateViewState};
use codex_plus_manager::theme;
use codex_plus_manager::views::about::{self, UpdateAction};
use codex_plus_manager::views::shell::ShellViewModel;
use codex_plus_manager_service::{
    UpdateAvailability, UpdateCheckResult, UpdateDownload, UpdateEnvironment,
    UpdateEnvironmentError, UpdateProgress, UpdateService,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};

mod common;

struct HarnessState {
    model: ShellViewModel,
    update: UpdateViewState,
    emitted: Vec<UpdateAction>,
}

fn render(ui: &mut egui::Ui, state: &mut HarnessState) {
    egui_extras::install_image_loaders(ui.ctx());
    theme::apply(ui.ctx(), state.model.theme);
    let mut actions = Vec::new();
    about::render(ui, &state.model, &state.update, &mut actions);
    state.emitted.extend(actions);
}

fn harness(update: UpdateViewState) -> Harness<'static, HarnessState> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 640.0))
        .build_ui_state(
            render,
            HarnessState {
                model: common::model(Locale::En, ThemeMode::Dark),
                update,
                emitted: Vec::new(),
            },
        )
}

#[test]
fn about_available_update_exposes_safe_details_and_explicit_install_action() {
    let mut update = UpdateViewState::default();
    let request = update.begin_check(false).unwrap();
    update.apply_check_response(request, Ok(available_result()));
    let mut harness = harness(update);

    assert!(
        harness
            .get_by_label("Version 2.0.0 is available")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .get_by_label("safe release summary")
            .rect()
            .is_positive()
    );
    assert!(
        harness
            .get_by_label("CodexPlusPlus-2.0.0-setup.exe")
            .rect()
            .is_positive()
    );
    assert!(harness.query_by_label("https://updates.invalid").is_none());

    harness.get_by_label("Download and install").click();
    harness.step();
    assert!(
        harness
            .state()
            .emitted
            .contains(&UpdateAction::RequestInstall)
    );
}

#[test]
fn about_confirmation_names_exact_version_and_cancel_is_explicit() {
    let mut update = UpdateViewState::default();
    let request = update.begin_check(false).unwrap();
    update.apply_check_response(request, Ok(available_result()));
    update.request_install_confirmation();
    let mut harness = harness(update);

    assert!(harness.get_by_label("Confirm update").rect().is_positive());
    assert!(
        harness
            .get_by_label("Install version 2.0.0?")
            .rect()
            .is_positive()
    );
    harness.get_by_label("Cancel").click();
    harness.step();
    assert!(
        harness
            .state()
            .emitted
            .contains(&UpdateAction::CancelInstall)
    );
    assert!(
        !harness
            .state()
            .emitted
            .contains(&UpdateAction::ConfirmInstall)
    );
}

#[test]
fn about_renders_current_downloading_launching_and_error_states() {
    let mut current = UpdateViewState::default();
    let check = current.begin_check(false).unwrap();
    current.apply_check_response(check, Ok(current_result()));
    assert!(
        harness(current)
            .get_by_label("Codex++ is up to date")
            .rect()
            .is_positive()
    );

    let mut downloading = UpdateViewState::default();
    let check = downloading.begin_check(false).unwrap();
    downloading.apply_check_response(check, Ok(available_result()));
    downloading.request_install_confirmation();
    let (install, _) = downloading.confirm_install().unwrap();
    downloading.apply_progress(
        install,
        UpdateProgress {
            downloaded_bytes: 40,
            total_bytes: Some(100),
        },
    );
    assert!(
        harness(downloading)
            .get_by_label("40 / 100 bytes")
            .rect()
            .is_positive()
    );

    let mut launching = UpdateViewState::default();
    let check = launching.begin_check(false).unwrap();
    launching.apply_check_response(check, Ok(available_result()));
    launching.request_install_confirmation();
    let (install, _) = launching.confirm_install().unwrap();
    launching.apply_install_response(
        install,
        Ok(codex_plus_manager_service::InstallStarted {
            version: "2.0.0".to_owned(),
        }),
    );
    assert!(
        harness(launching)
            .get_by_label("Installer opened. Exiting Codex++...")
            .rect()
            .is_positive()
    );

    let mut error = UpdateViewState::default();
    let check = error.begin_check(false).unwrap();
    error.apply_check_response(check, Err(UpdateFailureKind::MetadataFetchFailed));
    let mut harness = harness(error);
    assert_eq!(harness.state().update.phase, UpdatePhase::Error);
    harness.get_by_label("Retry update check").click();
    harness.step();
    assert!(harness.state().emitted.contains(&UpdateAction::Check));
}

fn current_result() -> Arc<UpdateCheckResult> {
    Arc::new(UpdateCheckResult {
        installed_version: "1.2.36".to_owned(),
        latest_version: "1.2.36".to_owned(),
        summary: String::new(),
        availability: UpdateAvailability::Current,
    })
}

fn available_result() -> Arc<UpdateCheckResult> {
    Arc::new(UpdateService::new(CandidateEnvironment).check().unwrap())
}

struct CandidateEnvironment;

impl UpdateEnvironment for CandidateEnvironment {
    type Artifact = Vec<u8>;

    fn current_version(&self) -> String {
        "1.2.36".to_owned()
    }
    fn target(&self) -> UpdateTarget {
        UpdateTarget::WindowsX64
    }
    fn fetch_release_metadata(
        &self,
        _maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        Ok(br#"{"version":"2.0.0","body":"safe release summary","assets":[{"name":"CodexPlusPlus-2.0.0-setup.exe","url":"https://updates.invalid/CodexPlusPlus-2.0.0-setup.exe"}]}"#.to_vec())
    }
    fn open_asset_download(&self, _url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        panic!("not used")
    }
    fn create_update_artifact(
        &self,
        _safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        panic!("not used")
    }
    fn publish_update_artifact(
        &self,
        _artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("not used")
    }
    fn cleanup_update_artifact(&self, _artifact: &mut Self::Artifact) {}
    fn launch_update_artifact(
        &self,
        _artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        panic!("not used")
    }
}
