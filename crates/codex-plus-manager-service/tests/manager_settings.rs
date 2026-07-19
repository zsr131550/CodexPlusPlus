use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use codex_plus_core::settings::BackendSettings;
use codex_plus_manager_service::{
    ConfirmedSecretClear, ExtraArgsSettings, ImageOverlayFitMode, ImageOverlaySettings,
    ManagerSettingsEnvironment, ManagerSettingsErrorKind, ManagerSettingsService, PathKind,
    PrivateArgument, PrivatePath, PrivateUrl, ResetExtraArgs, ResetImageOverlaySettings,
    ResetStepwiseSettings, SafeSettingsGroup, SaveExtraArgs, SaveImageOverlaySettings,
    SaveStepwiseSettings, SecretReplacement, StepwiseSecretChange, StepwiseSettingsInput,
    StepwiseTestFailure, TestStepwiseSettings,
};

const STORED_KEY: &str = "stored-key-sentinel";
const REPLACEMENT_KEY: &str = "replacement-key-sentinel";
const PRIVATE_URL: &str = "https://private-stepwise.invalid/v1";
const IMAGE_PATH: &str = "C:/private/image-sentinel.png";

#[derive(Clone)]
struct SettingsFixture {
    environment: FakeSettingsEnvironment,
}

impl SettingsFixture {
    fn with_secret(secret: &str) -> Self {
        let settings = BackendSettings {
            codex_app_stepwise_enabled: true,
            codex_app_stepwise_base_url: PRIVATE_URL.to_owned(),
            codex_app_stepwise_api_key: secret.to_owned(),
            codex_app_stepwise_model: "private-model-sentinel".to_owned(),
            ..BackendSettings::default()
        };
        Self {
            environment: FakeSettingsEnvironment::new(settings),
        }
    }

    fn with_unknown_root() -> Self {
        let fixture = Self::with_secret(STORED_KEY);
        fixture
            .environment
            .set_path_kind(IMAGE_PATH, PathKind::File);
        fixture
    }

    fn service(&self) -> ManagerSettingsService<FakeSettingsEnvironment> {
        ManagerSettingsService::new(self.environment.clone())
    }

    fn env_key_present(&self) -> bool {
        self.environment.state.lock().unwrap().env_key_present
    }

    fn mutate_provider_setting_out_of_band(&self) {
        self.environment
            .state
            .lock()
            .unwrap()
            .settings
            .provider_sync_enabled = true;
    }

    fn mutate_image_overlay_out_of_band(&self) {
        self.environment
            .state
            .lock()
            .unwrap()
            .settings
            .codex_app_image_overlay_opacity = 77;
    }

    fn mutate_stepwise_key_out_of_band(&self) {
        self.environment
            .state
            .lock()
            .unwrap()
            .settings
            .codex_app_stepwise_api_key = "out-of-band-key-sentinel".to_owned();
    }

    fn mutate_extra_args_out_of_band(&self) {
        self.environment
            .state
            .lock()
            .unwrap()
            .settings
            .codex_extra_args = vec!["--out-of-band".to_owned()];
    }

    fn assert_provider_and_unknown_fields_preserved(&self) {
        let state = self.environment.state.lock().unwrap();
        assert!(state.settings.provider_sync_enabled);
        assert!(state.unknown_root);
    }

    fn assert_tested_key(&self, expected: &str) {
        assert_eq!(
            self.environment
                .state
                .lock()
                .unwrap()
                .tested_keys
                .last()
                .map(String::as_str),
            Some(expected)
        );
    }
}

#[derive(Clone)]
struct FakeSettingsEnvironment {
    state: Arc<Mutex<FakeSettingsState>>,
}

struct FakeSettingsState {
    settings: BackendSettings,
    env_key_present: bool,
    path_kinds: HashMap<PathBuf, PathKind>,
    unknown_root: bool,
    successful_writes: usize,
    payloads: Vec<serde_json::Value>,
    tested_keys: Vec<String>,
    candidate_had_unrelated_secret: bool,
    test_failure: Option<StepwiseTestFailure>,
}

impl FakeSettingsEnvironment {
    fn new(settings: BackendSettings) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeSettingsState {
                settings,
                env_key_present: true,
                path_kinds: HashMap::new(),
                unknown_root: true,
                successful_writes: 0,
                payloads: Vec::new(),
                tested_keys: Vec::new(),
                candidate_had_unrelated_secret: false,
                test_failure: None,
            })),
        }
    }

    fn set_path_kind(&self, path: impl Into<PathBuf>, kind: PathKind) {
        self.state
            .lock()
            .unwrap()
            .path_kinds
            .insert(path.into(), kind);
    }
}

impl ManagerSettingsEnvironment for FakeSettingsEnvironment {
    fn load_manager_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.state.lock().unwrap().settings.clone())
    }

    fn update_manager_settings_if<F>(
        &self,
        payload: serde_json::Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        let mut state = self.state.lock().unwrap();
        state.payloads.push(payload.clone());
        if !predicate(&state.settings) {
            return Ok(None);
        }
        apply_payload(&mut state.settings, &payload);
        state.successful_writes += 1;
        Ok(Some(state.settings.clone()))
    }

    fn inspect_path(&self, path: &Path) -> anyhow::Result<PathKind> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .path_kinds
            .get(path)
            .copied()
            .unwrap_or(PathKind::Missing))
    }

    fn environment_value_present(&self, _name: &str) -> bool {
        self.state.lock().unwrap().env_key_present
    }

    fn test_stepwise_candidate(
        &self,
        settings: &BackendSettings,
    ) -> Result<usize, StepwiseTestFailure> {
        let mut state = self.state.lock().unwrap();
        state
            .tested_keys
            .push(settings.codex_app_stepwise_api_key.clone());
        state.candidate_had_unrelated_secret = settings
            .relay_profiles
            .iter()
            .any(|profile| !profile.api_key.is_empty());
        if let Some(failure) = state.test_failure {
            return Err(failure);
        }
        Ok(3)
    }
}

#[test]
fn workspace_has_three_independent_revisions_and_never_returns_the_stored_key() {
    let fixture = SettingsFixture::with_secret(STORED_KEY);
    let workspace = fixture.service().load_workspace().unwrap();

    assert!(workspace.stepwise.settings.api_key_configured);
    assert_eq!(
        workspace.stepwise.settings.api_key_env_configured,
        fixture.env_key_present()
    );
    assert_eq!(workspace.image_overlay.settings.opacity, 35);
    assert!(workspace.extra_args.settings.args.is_empty());
    for debug in [
        format!("{workspace:?}"),
        format!("{:?}", workspace.stepwise.revision),
        format!("{:?}", SecretReplacement::new(REPLACEMENT_KEY)),
        format!("{:?}", PrivateUrl::new(PRIVATE_URL)),
        format!("{:?}", PrivatePath::new(IMAGE_PATH)),
        format!("{:?}", PrivateArgument::new("--private-argument")),
    ] {
        assert!(!debug.contains(STORED_KEY));
        assert!(!debug.contains("private-stepwise.invalid"));
        assert!(!debug.contains("image-sentinel"));
        assert!(!debug.contains("private-argument"));
    }
}

#[test]
fn secret_replacement_supports_wrapped_ui_editing_without_debug_disclosure() {
    let mut replacement = SecretReplacement::new("first");
    replacement.expose_mut().push_str("-second");
    let cloned = replacement.clone();

    assert_eq!(replacement.len(), 12);
    assert!(!replacement.is_empty());
    assert_eq!(replacement, cloned);
    assert_eq!(format!("{replacement:?}"), "SecretReplacement([redacted])");
    assert!(!format!("{replacement:?}").contains("first-second"));
}

#[test]
fn each_group_preserves_unknown_fields_and_rejects_only_same_scope_conflicts() {
    let fixture = SettingsFixture::with_unknown_root();
    let service = fixture.service();
    let first = service.load_workspace().unwrap();
    fixture.mutate_provider_setting_out_of_band();

    let saved = service
        .save_image_overlay(SaveImageOverlaySettings {
            expected_revision: first.image_overlay.revision,
            settings: valid_image_overlay(),
        })
        .unwrap();
    fixture.assert_provider_and_unknown_fields_preserved();
    assert_ne!(first.image_overlay.revision, saved.image_overlay.revision);
    assert_eq!(
        service
            .save_image_overlay(SaveImageOverlaySettings {
                expected_revision: first.image_overlay.revision,
                settings: valid_image_overlay(),
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::InvalidRevision
    );

    let stale_image = service.load_workspace().unwrap();
    fixture.mutate_image_overlay_out_of_band();
    assert_eq!(
        service
            .save_image_overlay(SaveImageOverlaySettings {
                expected_revision: stale_image.image_overlay.revision,
                settings: valid_image_overlay(),
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::SettingsConflict
    );

    let stale_stepwise = service.load_workspace().unwrap();
    fixture.mutate_stepwise_key_out_of_band();
    assert_eq!(
        service
            .save_stepwise(SaveStepwiseSettings {
                expected_revision: stale_stepwise.stepwise.revision,
                settings: valid_stepwise(),
                secret_change: StepwiseSecretChange::Keep,
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::SettingsConflict
    );

    let stale_args = service.load_workspace().unwrap();
    fixture.mutate_extra_args_out_of_band();
    assert_eq!(
        service
            .save_extra_args(SaveExtraArgs {
                expected_revision: stale_args.extra_args.revision,
                settings: ExtraArgsSettings {
                    args: vec![PrivateArgument::new("--new")],
                },
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::SettingsConflict
    );
}

#[test]
fn stepwise_secret_intents_and_test_stay_inside_the_service() {
    let fixture = SettingsFixture::with_secret(STORED_KEY);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let outcome = service
        .test_stepwise(TestStepwiseSettings {
            expected_revision: workspace.stepwise.revision,
            settings: valid_stepwise(),
            secret_change: StepwiseSecretChange::Keep,
        })
        .unwrap();
    assert_eq!(outcome.item_count, 3);
    fixture.assert_tested_key(STORED_KEY);

    service
        .test_stepwise(TestStepwiseSettings {
            expected_revision: workspace.stepwise.revision,
            settings: valid_stepwise(),
            secret_change: StepwiseSecretChange::Replace(SecretReplacement::new(REPLACEMENT_KEY)),
        })
        .unwrap();
    fixture.assert_tested_key(REPLACEMENT_KEY);
    assert_eq!(
        fixture.environment.state.lock().unwrap().successful_writes,
        0
    );
    assert!(
        !fixture
            .environment
            .state
            .lock()
            .unwrap()
            .candidate_had_unrelated_secret
    );

    let confirmed = ConfirmedSecretClear::new(workspace.stepwise.revision);
    let saved = service
        .save_stepwise(SaveStepwiseSettings {
            expected_revision: workspace.stepwise.revision,
            settings: valid_stepwise(),
            secret_change: StepwiseSecretChange::Clear(confirmed),
        })
        .unwrap();
    assert!(!saved.stepwise.settings.api_key_configured);
    for observable in [
        format!("{saved:?}"),
        format!("{:?}", fixture.environment.state.lock().unwrap().payloads),
    ] {
        assert!(!observable.contains(STORED_KEY));
        assert!(!observable.contains(REPLACEMENT_KEY));
    }
}

#[test]
fn validation_and_stepwise_failures_are_stable_and_do_not_write() {
    let fixture = SettingsFixture::with_secret(STORED_KEY);
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();
    let mut invalid_url = valid_stepwise();
    invalid_url.base_url = PrivateUrl::new("file:///private");
    assert_eq!(
        service
            .save_stepwise(SaveStepwiseSettings {
                expected_revision: workspace.stepwise.revision,
                settings: invalid_url,
                secret_change: StepwiseSecretChange::Keep,
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::InvalidUrl
    );
    assert_eq!(
        fixture.environment.state.lock().unwrap().successful_writes,
        0
    );

    assert_eq!(
        service
            .test_stepwise(TestStepwiseSettings {
                expected_revision: workspace.stepwise.revision,
                settings: valid_stepwise(),
                secret_change: StepwiseSecretChange::Replace(SecretReplacement::new("  ")),
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::InvalidSecret
    );

    for (failure, expected) in [
        (
            StepwiseTestFailure::Unauthorized,
            ManagerSettingsErrorKind::StepwiseUnauthorized,
        ),
        (
            StepwiseTestFailure::Timeout,
            ManagerSettingsErrorKind::StepwiseTimeout,
        ),
        (
            StepwiseTestFailure::Rejected,
            ManagerSettingsErrorKind::StepwiseRejected,
        ),
        (
            StepwiseTestFailure::Network,
            ManagerSettingsErrorKind::StepwiseNetwork,
        ),
    ] {
        fixture.environment.state.lock().unwrap().test_failure = Some(failure);
        assert_eq!(
            service
                .test_stepwise(TestStepwiseSettings {
                    expected_revision: workspace.stepwise.revision,
                    settings: valid_stepwise(),
                    secret_change: StepwiseSecretChange::Keep,
                })
                .unwrap_err()
                .kind(),
            expected
        );
    }
}

#[test]
fn mismatched_secret_clear_is_rejected_without_consuming_the_expected_revision() {
    let fixture = SettingsFixture::with_secret(STORED_KEY);
    let service = fixture.service();
    let expected = service.load_workspace().unwrap();
    let other = service.load_workspace().unwrap();

    let mismatch = service
        .save_stepwise(SaveStepwiseSettings {
            expected_revision: expected.stepwise.revision,
            settings: valid_stepwise(),
            secret_change: StepwiseSecretChange::Clear(ConfirmedSecretClear::new(
                other.stepwise.revision,
            )),
        })
        .unwrap_err();
    assert_eq!(
        mismatch.kind(),
        ManagerSettingsErrorKind::ConfirmationMismatch
    );
    assert_eq!(
        fixture.environment.state.lock().unwrap().successful_writes,
        0
    );

    service
        .save_stepwise(SaveStepwiseSettings {
            expected_revision: expected.stepwise.revision,
            settings: valid_stepwise(),
            secret_change: StepwiseSecretChange::Keep,
        })
        .unwrap();
    assert_eq!(
        fixture.environment.state.lock().unwrap().successful_writes,
        1
    );
}

#[test]
fn resets_require_exact_group_and_change_only_the_owned_fields() {
    let fixture = SettingsFixture::with_unknown_root();
    {
        let mut state = fixture.environment.state.lock().unwrap();
        state.settings.provider_sync_enabled = true;
        state.settings.codex_app_image_overlay_enabled = true;
        state.settings.codex_app_image_overlay_path = IMAGE_PATH.to_owned();
        state.settings.codex_app_image_overlay_opacity = 80;
        state.settings.codex_app_image_overlay_fit_mode = "fill".to_owned();
        state.settings.codex_extra_args = vec!["--private-arg".to_owned()];
    }
    let service = fixture.service();
    let first = service.load_workspace().unwrap();

    assert_eq!(
        service
            .reset_image_overlay(ResetImageOverlaySettings {
                expected_revision: first.image_overlay.revision,
                confirmed_group: SafeSettingsGroup::ExtraArgs,
            })
            .unwrap_err()
            .kind(),
        ManagerSettingsErrorKind::ConfirmationMismatch
    );
    let after_image = service
        .reset_image_overlay(ResetImageOverlaySettings {
            expected_revision: first.image_overlay.revision,
            confirmed_group: SafeSettingsGroup::ImageOverlay,
        })
        .unwrap();
    assert!(!after_image.image_overlay.settings.enabled);
    assert_eq!(after_image.image_overlay.settings.opacity, 35);
    assert_eq!(after_image.extra_args.settings.args.len(), 1);

    let after_args = service
        .reset_extra_args(ResetExtraArgs {
            expected_revision: after_image.extra_args.revision,
            confirmed_group: SafeSettingsGroup::ExtraArgs,
        })
        .unwrap();
    assert!(after_args.extra_args.settings.args.is_empty());
    assert!(after_args.stepwise.settings.api_key_configured);

    let after_stepwise = service
        .reset_stepwise(ResetStepwiseSettings {
            expected_revision: after_args.stepwise.revision,
            confirmed_group: SafeSettingsGroup::Stepwise,
        })
        .unwrap();
    assert!(!after_stepwise.stepwise.settings.enabled);
    assert!(!after_stepwise.stepwise.settings.api_key_configured);
    fixture.assert_provider_and_unknown_fields_preserved();
}

#[test]
fn validation_enforces_every_approved_bound_without_consuming_tickets() {
    let fixture = SettingsFixture::with_unknown_root();
    let service = fixture.service();
    let workspace = service.load_workspace().unwrap();

    let mut invalid_inputs = Vec::new();
    let mut invalid_env = valid_stepwise();
    invalid_env.api_key_env = "9INVALID".to_owned();
    invalid_inputs.push((
        invalid_env,
        ManagerSettingsErrorKind::InvalidEnvironmentVariable,
    ));
    let mut invalid_model = valid_stepwise();
    invalid_model.model.clear();
    invalid_inputs.push((invalid_model, ManagerSettingsErrorKind::InvalidModel));
    for mutate in [
        |input: &mut StepwiseSettingsInput| input.max_items = 7,
        |input: &mut StepwiseSettingsInput| input.max_input_chars = 999,
        |input: &mut StepwiseSettingsInput| input.max_output_tokens = 99,
        |input: &mut StepwiseSettingsInput| input.timeout_ms = 999,
    ] {
        let mut input = valid_stepwise();
        mutate(&mut input);
        invalid_inputs.push((input, ManagerSettingsErrorKind::InvalidNumericField));
    }
    for (settings, expected) in invalid_inputs {
        assert_eq!(
            service
                .save_stepwise(SaveStepwiseSettings {
                    expected_revision: workspace.stepwise.revision,
                    settings,
                    secret_change: StepwiseSecretChange::Keep,
                })
                .unwrap_err()
                .kind(),
            expected
        );
    }

    let invalid_images = [
        ImageOverlaySettings {
            opacity: 0,
            ..valid_image_overlay()
        },
        ImageOverlaySettings {
            path: PrivatePath::new("C:/private/image.txt"),
            ..valid_image_overlay()
        },
    ];
    for settings in invalid_images {
        assert!(matches!(
            service
                .save_image_overlay(SaveImageOverlaySettings {
                    expected_revision: workspace.image_overlay.revision,
                    settings,
                })
                .unwrap_err()
                .kind(),
            ManagerSettingsErrorKind::InvalidNumericField | ManagerSettingsErrorKind::InvalidPath
        ));
    }

    for args in [
        (0..129)
            .map(|index| PrivateArgument::new(format!("--arg-{index}")))
            .collect(),
        vec![PrivateArgument::new("x".repeat(4097))],
        vec![PrivateArgument::new("--bad\nargument")],
    ] {
        assert_eq!(
            service
                .save_extra_args(SaveExtraArgs {
                    expected_revision: workspace.extra_args.revision,
                    settings: ExtraArgsSettings { args },
                })
                .unwrap_err()
                .kind(),
            ManagerSettingsErrorKind::InvalidArgument
        );
    }

    service
        .save_image_overlay(SaveImageOverlaySettings {
            expected_revision: workspace.image_overlay.revision,
            settings: valid_image_overlay(),
        })
        .unwrap();
    service
        .save_extra_args(SaveExtraArgs {
            expected_revision: workspace.extra_args.revision,
            settings: ExtraArgsSettings {
                args: vec![
                    PrivateArgument::new("  --valid  "),
                    PrivateArgument::new(""),
                ],
            },
        })
        .unwrap();
    assert_eq!(
        fixture.environment.state.lock().unwrap().successful_writes,
        2
    );
}

fn valid_stepwise() -> StepwiseSettingsInput {
    StepwiseSettingsInput {
        enabled: true,
        direct_send: false,
        base_url: PrivateUrl::new(PRIVATE_URL),
        api_key_env: "CODEX_STEPWISE_API_KEY".to_owned(),
        model: "stepwise-model".to_owned(),
        max_items: 6,
        max_input_chars: 6000,
        max_output_tokens: 500,
        timeout_ms: 8000,
    }
}

fn valid_image_overlay() -> ImageOverlaySettings {
    ImageOverlaySettings {
        enabled: true,
        path: PrivatePath::new(IMAGE_PATH),
        opacity: 35,
        fit_mode: ImageOverlayFitMode::Fit,
    }
}

fn apply_payload(settings: &mut BackendSettings, payload: &serde_json::Value) {
    if let Some(value) = payload
        .get("codexAppStepwiseEnabled")
        .and_then(|value| value.as_bool())
    {
        settings.codex_app_stepwise_enabled = value;
    }
    if let Some(value) = payload
        .get("codexAppStepwiseDirectSend")
        .and_then(|value| value.as_bool())
    {
        settings.codex_app_stepwise_direct_send = value;
    }
    for (key, target) in [
        (
            "codexAppStepwiseBaseUrl",
            &mut settings.codex_app_stepwise_base_url,
        ),
        (
            "codexAppStepwiseApiKey",
            &mut settings.codex_app_stepwise_api_key,
        ),
        (
            "codexAppStepwiseApiKeyEnv",
            &mut settings.codex_app_stepwise_api_key_env,
        ),
        (
            "codexAppStepwiseModel",
            &mut settings.codex_app_stepwise_model,
        ),
        (
            "codexAppImageOverlayPath",
            &mut settings.codex_app_image_overlay_path,
        ),
        (
            "codexAppImageOverlayFitMode",
            &mut settings.codex_app_image_overlay_fit_mode,
        ),
    ] {
        if let Some(value) = payload.get(key).and_then(|value| value.as_str()) {
            *target = value.to_owned();
        }
    }
    if let Some(value) = payload
        .get("codexAppImageOverlayEnabled")
        .and_then(|value| value.as_bool())
    {
        settings.codex_app_image_overlay_enabled = value;
    }
    if let Some(value) = payload
        .get("codexAppImageOverlayOpacity")
        .and_then(|value| value.as_u64())
    {
        settings.codex_app_image_overlay_opacity = value as u8;
    }
    if let Some(value) = payload
        .get("codexAppStepwiseMaxItems")
        .and_then(|value| value.as_u64())
    {
        settings.codex_app_stepwise_max_items = value as u8;
    }
    if let Some(value) = payload
        .get("codexAppStepwiseMaxInputChars")
        .and_then(|value| value.as_u64())
    {
        settings.codex_app_stepwise_max_input_chars = value as u32;
    }
    if let Some(value) = payload
        .get("codexAppStepwiseMaxOutputTokens")
        .and_then(|value| value.as_u64())
    {
        settings.codex_app_stepwise_max_output_tokens = value as u32;
    }
    if let Some(value) = payload
        .get("codexAppStepwiseTimeoutMs")
        .and_then(|value| value.as_u64())
    {
        settings.codex_app_stepwise_timeout_ms = value;
    }
    if let Some(value) = payload
        .get("codexExtraArgs")
        .and_then(|value| value.as_array())
    {
        settings.codex_extra_args = value
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect();
    }
}
