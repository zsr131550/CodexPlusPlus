use std::fmt;
use std::path::Path;
use std::sync::Arc;

use codex_plus_core::settings::{BackendSettings, normalize_codex_extra_args};
use serde::Serialize;
use serde_json::{Map, Value, json};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::revision_ledger::{
    RevisionLedger, RevisionScope, RevisionTicket, scoped_fingerprint, stepwise_fingerprint,
};
use crate::{PathKind, PrivatePath, SafeSettingsGroup};

const IMAGE_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/image-overlay/v1";
const EXTRA_ARGS_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/extra-args/v1";
const MAX_SECRET_BYTES: usize = 8192;
const MAX_ARGUMENTS: usize = 128;
const MAX_ARGUMENT_BYTES: usize = 4096;

macro_rules! opaque_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(RevisionTicket);

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([opaque])"))
            }
        }
    };
}

opaque_revision!(StepwiseRevision);
opaque_revision!(ImageOverlayRevision);
opaque_revision!(ExtraArgsRevision);

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateUrl(String);

impl PrivateUrl {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivateUrl([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateArgument(String);

impl PrivateArgument {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivateArgument([redacted])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOverlayFitMode {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
}

impl ImageOverlayFitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Stretch => "stretch",
            Self::Tile => "tile",
            Self::Center => "center",
        }
    }

    fn from_stored(value: &str) -> Self {
        match value {
            "fill" => Self::Fill,
            "stretch" => Self::Stretch,
            "tile" => Self::Tile,
            "center" => Self::Center,
            _ => Self::Fit,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StepwiseSettings {
    pub enabled: bool,
    pub direct_send: bool,
    pub base_url: PrivateUrl,
    pub api_key_configured: bool,
    pub api_key_env: String,
    pub api_key_env_configured: bool,
    pub model: String,
    pub max_items: u8,
    pub max_input_chars: u32,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
}

impl fmt::Debug for StepwiseSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StepwiseSettings")
            .field("enabled", &self.enabled)
            .field("direct_send", &self.direct_send)
            .field("base_url_configured", &!self.base_url.as_str().is_empty())
            .field("api_key_configured", &self.api_key_configured)
            .field("api_key_env_configured", &self.api_key_env_configured)
            .field("model_configured", &!self.model.is_empty())
            .field("max_items", &self.max_items)
            .field("max_input_chars", &self.max_input_chars)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StepwiseSettingsInput {
    pub enabled: bool,
    pub direct_send: bool,
    pub base_url: PrivateUrl,
    pub api_key_env: String,
    pub model: String,
    pub max_items: u8,
    pub max_input_chars: u32,
    pub max_output_tokens: u32,
    pub timeout_ms: u64,
}

impl fmt::Debug for StepwiseSettingsInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StepwiseSettingsInput")
            .field("enabled", &self.enabled)
            .field("direct_send", &self.direct_send)
            .field("base_url_configured", &!self.base_url.as_str().is_empty())
            .field("environment_name_configured", &!self.api_key_env.is_empty())
            .field("model_configured", &!self.model.is_empty())
            .field("max_items", &self.max_items)
            .field("max_input_chars", &self.max_input_chars)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImageOverlaySettings {
    pub enabled: bool,
    pub path: PrivatePath,
    pub opacity: u8,
    pub fit_mode: ImageOverlayFitMode,
}

impl fmt::Debug for ImageOverlaySettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageOverlaySettings")
            .field("enabled", &self.enabled)
            .field("path_configured", &!self.path.as_str().is_empty())
            .field("opacity", &self.opacity)
            .field("fit_mode", &self.fit_mode)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExtraArgsSettings {
    pub args: Vec<PrivateArgument>,
}

impl fmt::Debug for ExtraArgsSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtraArgsSettings")
            .field("argument_count", &self.args.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RevisionedStepwiseSettings {
    pub revision: StepwiseRevision,
    pub settings: StepwiseSettings,
}

impl fmt::Debug for RevisionedStepwiseSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevisionedStepwiseSettings")
            .field("revision", &self.revision)
            .field("settings", &self.settings)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RevisionedImageOverlaySettings {
    pub revision: ImageOverlayRevision,
    pub settings: ImageOverlaySettings,
}

impl fmt::Debug for RevisionedImageOverlaySettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevisionedImageOverlaySettings")
            .field("revision", &self.revision)
            .field("settings", &self.settings)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RevisionedExtraArgs {
    pub revision: ExtraArgsRevision,
    pub settings: ExtraArgsSettings,
}

impl fmt::Debug for RevisionedExtraArgs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevisionedExtraArgs")
            .field("revision", &self.revision)
            .field("settings", &self.settings)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerSettingsWorkspace {
    pub stepwise: RevisionedStepwiseSettings,
    pub image_overlay: RevisionedImageOverlaySettings,
    pub extra_args: RevisionedExtraArgs,
}

impl fmt::Debug for ManagerSettingsWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerSettingsWorkspace")
            .field("stepwise", &self.stepwise)
            .field("image_overlay", &self.image_overlay)
            .field("extra_args", &self.extra_args)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretReplacement(Zeroizing<String>);

impl SecretReplacement {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose_mut(&mut self) -> &mut String {
        &mut self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretReplacement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretReplacement([redacted])")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConfirmedSecretClear {
    revision: StepwiseRevision,
}

impl ConfirmedSecretClear {
    pub fn new(revision: StepwiseRevision) -> Self {
        Self { revision }
    }
}

impl fmt::Debug for ConfirmedSecretClear {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfirmedSecretClear")
            .field("revision", &self.revision)
            .finish()
    }
}

pub enum StepwiseSecretChange {
    Keep,
    Replace(SecretReplacement),
    Clear(ConfirmedSecretClear),
}

impl fmt::Debug for StepwiseSecretChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => formatter.write_str("Keep"),
            Self::Replace(_) => formatter.write_str("Replace([redacted])"),
            Self::Clear(confirmation) => {
                formatter.debug_tuple("Clear").field(confirmation).finish()
            }
        }
    }
}

pub struct SaveStepwiseSettings {
    pub expected_revision: StepwiseRevision,
    pub settings: StepwiseSettingsInput,
    pub secret_change: StepwiseSecretChange,
}

impl fmt::Debug for SaveStepwiseSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SaveStepwiseSettings")
            .field("expected_revision", &self.expected_revision)
            .field("settings", &self.settings)
            .field("secret_change", &self.secret_change)
            .finish()
    }
}

pub struct TestStepwiseSettings {
    pub expected_revision: StepwiseRevision,
    pub settings: StepwiseSettingsInput,
    pub secret_change: StepwiseSecretChange,
}

impl fmt::Debug for TestStepwiseSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TestStepwiseSettings")
            .field("expected_revision", &self.expected_revision)
            .field("settings", &self.settings)
            .field("secret_change", &self.secret_change)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetStepwiseSettings {
    pub expected_revision: StepwiseRevision,
    pub confirmed_group: SafeSettingsGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveImageOverlaySettings {
    pub expected_revision: ImageOverlayRevision,
    pub settings: ImageOverlaySettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetImageOverlaySettings {
    pub expected_revision: ImageOverlayRevision,
    pub confirmed_group: SafeSettingsGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveExtraArgs {
    pub expected_revision: ExtraArgsRevision,
    pub settings: ExtraArgsSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetExtraArgs {
    pub expected_revision: ExtraArgsRevision,
    pub confirmed_group: SafeSettingsGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepwiseTestOutcome {
    pub item_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepwiseTestFailure {
    Unauthorized,
    Timeout,
    Rejected,
    Network,
}

pub trait StepwiseConnectionTester: Send + Sync + 'static {
    fn test(&self, settings: &BackendSettings) -> Result<usize, StepwiseTestFailure>;
}

pub trait ManagerSettingsEnvironment: Send + Sync + 'static {
    fn load_manager_settings(&self) -> anyhow::Result<BackendSettings>;
    fn update_manager_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool;
    fn inspect_path(&self, path: &Path) -> anyhow::Result<PathKind>;
    fn environment_value_present(&self, name: &str) -> bool;
    fn test_stepwise_candidate(
        &self,
        settings: &BackendSettings,
    ) -> Result<usize, StepwiseTestFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerSettingsErrorKind {
    SettingsReadFailed,
    SettingsWriteFailed,
    SettingsConflict,
    InvalidRevision,
    InvalidUrl,
    InvalidEnvironmentVariable,
    InvalidModel,
    InvalidNumericField,
    InvalidPath,
    InvalidFitMode,
    InvalidArgument,
    InvalidSecret,
    ConfirmationMismatch,
    StepwiseUnauthorized,
    StepwiseTimeout,
    StepwiseRejected,
    StepwiseNetwork,
    WorkerStopped,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerSettingsError {
    kind: ManagerSettingsErrorKind,
    group: Option<SafeSettingsGroup>,
    refreshed_workspace: Option<Box<ManagerSettingsWorkspace>>,
}

impl ManagerSettingsError {
    pub fn new(kind: ManagerSettingsErrorKind, group: Option<SafeSettingsGroup>) -> Self {
        Self {
            kind,
            group,
            refreshed_workspace: None,
        }
    }

    pub fn kind(&self) -> ManagerSettingsErrorKind {
        self.kind
    }

    pub fn group(&self) -> Option<SafeSettingsGroup> {
        self.group
    }

    pub fn refreshed_workspace(&self) -> Option<&ManagerSettingsWorkspace> {
        self.refreshed_workspace.as_deref()
    }

    fn with_refreshed_workspace(mut self, workspace: Option<ManagerSettingsWorkspace>) -> Self {
        self.refreshed_workspace = workspace.map(Box::new);
        self
    }

    fn detail(&self) -> &'static str {
        match self.kind {
            ManagerSettingsErrorKind::SettingsReadFailed => "manager settings read failed",
            ManagerSettingsErrorKind::SettingsWriteFailed => "manager settings write failed",
            ManagerSettingsErrorKind::SettingsConflict => "manager settings changed on disk",
            ManagerSettingsErrorKind::InvalidRevision => "manager settings revision is invalid",
            ManagerSettingsErrorKind::InvalidUrl => "Stepwise URL is invalid",
            ManagerSettingsErrorKind::InvalidEnvironmentVariable => {
                "Stepwise environment variable is invalid"
            }
            ManagerSettingsErrorKind::InvalidModel => "Stepwise model is invalid",
            ManagerSettingsErrorKind::InvalidNumericField => {
                "manager settings numeric field is invalid"
            }
            ManagerSettingsErrorKind::InvalidPath => "image overlay path is invalid",
            ManagerSettingsErrorKind::InvalidFitMode => "image overlay fit mode is invalid",
            ManagerSettingsErrorKind::InvalidArgument => "Codex launch argument is invalid",
            ManagerSettingsErrorKind::InvalidSecret => "Stepwise secret replacement is invalid",
            ManagerSettingsErrorKind::ConfirmationMismatch => {
                "manager settings confirmation does not match"
            }
            ManagerSettingsErrorKind::StepwiseUnauthorized => "Stepwise request was unauthorized",
            ManagerSettingsErrorKind::StepwiseTimeout => "Stepwise request timed out",
            ManagerSettingsErrorKind::StepwiseRejected => "Stepwise request was rejected",
            ManagerSettingsErrorKind::StepwiseNetwork => "Stepwise network request failed",
            ManagerSettingsErrorKind::WorkerStopped => "manager settings worker stopped",
        }
    }
}

impl fmt::Debug for ManagerSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagerSettingsError")
            .field("kind", &self.kind)
            .field("group", &self.group)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

impl fmt::Display for ManagerSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ManagerSettingsError {}

#[derive(Clone)]
pub struct ManagerSettingsService<E> {
    environment: E,
    revisions: Arc<RevisionLedger>,
}

impl<E> ManagerSettingsService<E> {
    pub fn new(environment: E) -> Self {
        Self {
            environment,
            revisions: Arc::new(RevisionLedger::default()),
        }
    }
}

impl<E: ManagerSettingsEnvironment> ManagerSettingsService<E> {
    pub fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        let settings = self.environment.load_manager_settings().map_err(|_| {
            ManagerSettingsError::new(ManagerSettingsErrorKind::SettingsReadFailed, None)
        })?;
        Ok(self.workspace_from_settings(&settings))
    }

    pub fn save_stepwise(
        &self,
        request: SaveStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        let normalized = normalize_stepwise(
            &request.settings,
            &request.secret_change,
            request.expected_revision,
        )?;
        let expected = self
            .revisions
            .take(request.expected_revision.0, RevisionScope::Stepwise)
            .ok_or_else(|| {
                settings_error(
                    ManagerSettingsErrorKind::InvalidRevision,
                    SafeSettingsGroup::Stepwise,
                )
            })?;
        let payload = stepwise_payload(&normalized);
        let updated = self
            .environment
            .update_manager_settings_if(payload, move |current| {
                stepwise_fingerprint(current) == expected
            })
            .map_err(|_| {
                settings_error(
                    ManagerSettingsErrorKind::SettingsWriteFailed,
                    SafeSettingsGroup::Stepwise,
                )
            })?;
        if updated.is_none() {
            return Err(self.conflict_error(SafeSettingsGroup::Stepwise));
        }
        self.load_workspace()
    }

    pub fn reset_stepwise(
        &self,
        request: ResetStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        if request.confirmed_group != SafeSettingsGroup::Stepwise {
            return Err(settings_error(
                ManagerSettingsErrorKind::ConfirmationMismatch,
                SafeSettingsGroup::Stepwise,
            ));
        }
        let defaults = BackendSettings::default();
        self.save_stepwise(SaveStepwiseSettings {
            expected_revision: request.expected_revision,
            settings: StepwiseSettingsInput::from_backend(&defaults),
            secret_change: StepwiseSecretChange::Clear(ConfirmedSecretClear::new(
                request.expected_revision,
            )),
        })
    }

    pub fn test_stepwise(
        &self,
        request: TestStepwiseSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        let normalized = normalize_stepwise(
            &request.settings,
            &request.secret_change,
            request.expected_revision,
        )?;
        let expected = self
            .revisions
            .peek(request.expected_revision.0, RevisionScope::Stepwise)
            .ok_or_else(|| {
                settings_error(
                    ManagerSettingsErrorKind::InvalidRevision,
                    SafeSettingsGroup::Stepwise,
                )
            })?;
        let current = self.environment.load_manager_settings().map_err(|_| {
            settings_error(
                ManagerSettingsErrorKind::SettingsReadFailed,
                SafeSettingsGroup::Stepwise,
            )
        })?;
        if stepwise_fingerprint(&current) != expected {
            return Err(self.conflict_error(SafeSettingsGroup::Stepwise));
        }
        let candidate = StepwiseCandidate::new(&current, &normalized);
        let item_count = self
            .environment
            .test_stepwise_candidate(&candidate.settings)
            .map_err(stepwise_test_error)?;
        Ok(StepwiseTestOutcome { item_count })
    }

    pub fn save_image_overlay(
        &self,
        request: SaveImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        let normalized = self.normalize_image_overlay(&request.settings)?;
        let expected = self
            .revisions
            .take(request.expected_revision.0, RevisionScope::ImageOverlay)
            .ok_or_else(|| {
                settings_error(
                    ManagerSettingsErrorKind::InvalidRevision,
                    SafeSettingsGroup::ImageOverlay,
                )
            })?;
        let updated = self
            .environment
            .update_manager_settings_if(
                json!({
                    "codexAppImageOverlayEnabled": normalized.enabled,
                    "codexAppImageOverlayPath": normalized.path,
                    "codexAppImageOverlayOpacity": normalized.opacity,
                    "codexAppImageOverlayFitMode": normalized.fit_mode.as_str(),
                }),
                move |current| image_overlay_fingerprint(current) == expected,
            )
            .map_err(|_| {
                settings_error(
                    ManagerSettingsErrorKind::SettingsWriteFailed,
                    SafeSettingsGroup::ImageOverlay,
                )
            })?;
        if updated.is_none() {
            return Err(self.conflict_error(SafeSettingsGroup::ImageOverlay));
        }
        self.load_workspace()
    }

    pub fn reset_image_overlay(
        &self,
        request: ResetImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        if request.confirmed_group != SafeSettingsGroup::ImageOverlay {
            return Err(settings_error(
                ManagerSettingsErrorKind::ConfirmationMismatch,
                SafeSettingsGroup::ImageOverlay,
            ));
        }
        let defaults = BackendSettings::default();
        self.save_image_overlay(SaveImageOverlaySettings {
            expected_revision: request.expected_revision,
            settings: ImageOverlaySettings::from_backend(&defaults),
        })
    }

    pub fn save_extra_args(
        &self,
        request: SaveExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        let args = normalize_extra_args(&request.settings)?;
        let expected = self
            .revisions
            .take(request.expected_revision.0, RevisionScope::ExtraArgs)
            .ok_or_else(|| {
                settings_error(
                    ManagerSettingsErrorKind::InvalidRevision,
                    SafeSettingsGroup::ExtraArgs,
                )
            })?;
        let updated = self
            .environment
            .update_manager_settings_if(json!({ "codexExtraArgs": args }), move |current| {
                extra_args_fingerprint(current) == expected
            })
            .map_err(|_| {
                settings_error(
                    ManagerSettingsErrorKind::SettingsWriteFailed,
                    SafeSettingsGroup::ExtraArgs,
                )
            })?;
        if updated.is_none() {
            return Err(self.conflict_error(SafeSettingsGroup::ExtraArgs));
        }
        self.load_workspace()
    }

    pub fn reset_extra_args(
        &self,
        request: ResetExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        if request.confirmed_group != SafeSettingsGroup::ExtraArgs {
            return Err(settings_error(
                ManagerSettingsErrorKind::ConfirmationMismatch,
                SafeSettingsGroup::ExtraArgs,
            ));
        }
        let defaults = BackendSettings::default();
        self.save_extra_args(SaveExtraArgs {
            expected_revision: request.expected_revision,
            settings: ExtraArgsSettings::from_backend(&defaults),
        })
    }

    pub fn test_compatibility_settings(
        &self,
        mut settings: BackendSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        let input = StepwiseSettingsInput::from_backend(&settings);
        let normalized = normalize_stepwise_fields(&input)?;
        let direct_key = Zeroizing::new(std::mem::take(&mut settings.codex_app_stepwise_api_key));
        if direct_key.len() > MAX_SECRET_BYTES {
            return Err(settings_error(
                ManagerSettingsErrorKind::InvalidSecret,
                SafeSettingsGroup::Stepwise,
            ));
        }
        let candidate = StepwiseCandidate::with_direct_key(&normalized, direct_key.as_str());
        let item_count = self
            .environment
            .test_stepwise_candidate(&candidate.settings)
            .map_err(stepwise_test_error)?;
        Ok(StepwiseTestOutcome { item_count })
    }

    fn workspace_from_settings(&self, settings: &BackendSettings) -> ManagerSettingsWorkspace {
        let stepwise_revision = StepwiseRevision(
            self.revisions
                .issue(RevisionScope::Stepwise, stepwise_fingerprint(settings)),
        );
        let image_revision = ImageOverlayRevision(self.revisions.issue(
            RevisionScope::ImageOverlay,
            image_overlay_fingerprint(settings),
        ));
        let args_revision = ExtraArgsRevision(
            self.revisions
                .issue(RevisionScope::ExtraArgs, extra_args_fingerprint(settings)),
        );
        ManagerSettingsWorkspace {
            stepwise: RevisionedStepwiseSettings {
                revision: stepwise_revision,
                settings: StepwiseSettings {
                    enabled: settings.codex_app_stepwise_enabled,
                    direct_send: settings.codex_app_stepwise_direct_send,
                    base_url: PrivateUrl::new(settings.codex_app_stepwise_base_url.clone()),
                    api_key_configured: !settings.codex_app_stepwise_api_key.trim().is_empty(),
                    api_key_env: settings.codex_app_stepwise_api_key_env.clone(),
                    api_key_env_configured: self
                        .environment
                        .environment_value_present(&settings.codex_app_stepwise_api_key_env),
                    model: settings.codex_app_stepwise_model.clone(),
                    max_items: settings.codex_app_stepwise_max_items,
                    max_input_chars: settings.codex_app_stepwise_max_input_chars,
                    max_output_tokens: settings.codex_app_stepwise_max_output_tokens,
                    timeout_ms: settings.codex_app_stepwise_timeout_ms,
                },
            },
            image_overlay: RevisionedImageOverlaySettings {
                revision: image_revision,
                settings: ImageOverlaySettings::from_backend(settings),
            },
            extra_args: RevisionedExtraArgs {
                revision: args_revision,
                settings: ExtraArgsSettings::from_backend(settings),
            },
        }
    }

    fn normalize_image_overlay(
        &self,
        settings: &ImageOverlaySettings,
    ) -> Result<NormalizedImageOverlay, ManagerSettingsError> {
        if !(1..=100).contains(&settings.opacity) {
            return Err(settings_error(
                ManagerSettingsErrorKind::InvalidNumericField,
                SafeSettingsGroup::ImageOverlay,
            ));
        }
        let path = settings.path.as_str().trim().to_owned();
        if settings.enabled && path.is_empty() {
            return Err(settings_error(
                ManagerSettingsErrorKind::InvalidPath,
                SafeSettingsGroup::ImageOverlay,
            ));
        }
        if !path.is_empty() {
            let extension = Path::new(&path)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            if !extension.as_deref().is_some_and(|extension| {
                matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
            }) {
                return Err(settings_error(
                    ManagerSettingsErrorKind::InvalidPath,
                    SafeSettingsGroup::ImageOverlay,
                ));
            }
            let kind = self
                .environment
                .inspect_path(Path::new(&path))
                .map_err(|_| {
                    settings_error(
                        ManagerSettingsErrorKind::InvalidPath,
                        SafeSettingsGroup::ImageOverlay,
                    )
                })?;
            if kind != PathKind::File {
                return Err(settings_error(
                    ManagerSettingsErrorKind::InvalidPath,
                    SafeSettingsGroup::ImageOverlay,
                ));
            }
        }
        Ok(NormalizedImageOverlay {
            enabled: settings.enabled,
            path,
            opacity: settings.opacity,
            fit_mode: settings.fit_mode,
        })
    }

    fn conflict_error(&self, group: SafeSettingsGroup) -> ManagerSettingsError {
        settings_error(ManagerSettingsErrorKind::SettingsConflict, group)
            .with_refreshed_workspace(self.load_workspace().ok())
    }
}

pub trait ManagerSettingsSource: Send + Sync + 'static {
    fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn save_stepwise(
        &self,
        request: SaveStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn reset_stepwise(
        &self,
        request: ResetStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn test_stepwise(
        &self,
        request: TestStepwiseSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError>;
    fn save_image_overlay(
        &self,
        request: SaveImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn reset_image_overlay(
        &self,
        request: ResetImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn save_extra_args(
        &self,
        request: SaveExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn reset_extra_args(
        &self,
        request: ResetExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError>;
    fn test_compatibility_settings(
        &self,
        settings: BackendSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError>;
}

impl<E: ManagerSettingsEnvironment> ManagerSettingsSource for ManagerSettingsService<E> {
    fn load_workspace(&self) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::load_workspace(self)
    }

    fn save_stepwise(
        &self,
        request: SaveStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::save_stepwise(self, request)
    }

    fn reset_stepwise(
        &self,
        request: ResetStepwiseSettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::reset_stepwise(self, request)
    }

    fn test_stepwise(
        &self,
        request: TestStepwiseSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        ManagerSettingsService::test_stepwise(self, request)
    }

    fn save_image_overlay(
        &self,
        request: SaveImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::save_image_overlay(self, request)
    }

    fn reset_image_overlay(
        &self,
        request: ResetImageOverlaySettings,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::reset_image_overlay(self, request)
    }

    fn save_extra_args(
        &self,
        request: SaveExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::save_extra_args(self, request)
    }

    fn reset_extra_args(
        &self,
        request: ResetExtraArgs,
    ) -> Result<ManagerSettingsWorkspace, ManagerSettingsError> {
        ManagerSettingsService::reset_extra_args(self, request)
    }

    fn test_compatibility_settings(
        &self,
        settings: BackendSettings,
    ) -> Result<StepwiseTestOutcome, ManagerSettingsError> {
        ManagerSettingsService::test_compatibility_settings(self, settings)
    }
}

impl StepwiseSettingsInput {
    fn from_backend(settings: &BackendSettings) -> Self {
        Self {
            enabled: settings.codex_app_stepwise_enabled,
            direct_send: settings.codex_app_stepwise_direct_send,
            base_url: PrivateUrl::new(settings.codex_app_stepwise_base_url.clone()),
            api_key_env: settings.codex_app_stepwise_api_key_env.clone(),
            model: settings.codex_app_stepwise_model.clone(),
            max_items: settings.codex_app_stepwise_max_items,
            max_input_chars: settings.codex_app_stepwise_max_input_chars,
            max_output_tokens: settings.codex_app_stepwise_max_output_tokens,
            timeout_ms: settings.codex_app_stepwise_timeout_ms,
        }
    }
}

impl ImageOverlaySettings {
    fn from_backend(settings: &BackendSettings) -> Self {
        Self {
            enabled: settings.codex_app_image_overlay_enabled,
            path: PrivatePath::new(settings.codex_app_image_overlay_path.clone()),
            opacity: settings.codex_app_image_overlay_opacity,
            fit_mode: ImageOverlayFitMode::from_stored(&settings.codex_app_image_overlay_fit_mode),
        }
    }
}

impl ExtraArgsSettings {
    fn from_backend(settings: &BackendSettings) -> Self {
        Self {
            args: normalize_codex_extra_args(&settings.codex_extra_args)
                .into_iter()
                .map(PrivateArgument::new)
                .collect(),
        }
    }
}

struct NormalizedStepwise {
    enabled: bool,
    direct_send: bool,
    base_url: String,
    api_key_env: String,
    model: String,
    max_items: u8,
    max_input_chars: u32,
    max_output_tokens: u32,
    timeout_ms: u64,
    secret: NormalizedSecretChange,
}

enum NormalizedSecretChange {
    Keep,
    Set(Zeroizing<String>),
}

fn normalize_stepwise(
    input: &StepwiseSettingsInput,
    secret: &StepwiseSecretChange,
    expected_revision: StepwiseRevision,
) -> Result<NormalizedStepwise, ManagerSettingsError> {
    let fields = normalize_stepwise_fields(input)?;
    let secret = match secret {
        StepwiseSecretChange::Keep => NormalizedSecretChange::Keep,
        StepwiseSecretChange::Replace(value) => {
            let value = value.as_str().trim();
            if value.is_empty() || value.len() > MAX_SECRET_BYTES {
                return Err(settings_error(
                    ManagerSettingsErrorKind::InvalidSecret,
                    SafeSettingsGroup::Stepwise,
                ));
            }
            NormalizedSecretChange::Set(Zeroizing::new(value.to_owned()))
        }
        StepwiseSecretChange::Clear(confirmation) => {
            if confirmation.revision != expected_revision {
                return Err(settings_error(
                    ManagerSettingsErrorKind::ConfirmationMismatch,
                    SafeSettingsGroup::Stepwise,
                ));
            }
            NormalizedSecretChange::Set(Zeroizing::new(String::new()))
        }
    };
    Ok(NormalizedStepwise {
        enabled: fields.enabled,
        direct_send: fields.direct_send,
        base_url: fields.base_url,
        api_key_env: fields.api_key_env,
        model: fields.model,
        max_items: fields.max_items,
        max_input_chars: fields.max_input_chars,
        max_output_tokens: fields.max_output_tokens,
        timeout_ms: fields.timeout_ms,
        secret,
    })
}

struct NormalizedStepwiseFields {
    enabled: bool,
    direct_send: bool,
    base_url: String,
    api_key_env: String,
    model: String,
    max_items: u8,
    max_input_chars: u32,
    max_output_tokens: u32,
    timeout_ms: u64,
}

fn normalize_stepwise_fields(
    input: &StepwiseSettingsInput,
) -> Result<NormalizedStepwiseFields, ManagerSettingsError> {
    let base_url = input
        .base_url
        .as_str()
        .trim()
        .trim_end_matches('/')
        .to_owned();
    if input.enabled && base_url.is_empty() {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidUrl,
            SafeSettingsGroup::Stepwise,
        ));
    }
    if !base_url.is_empty() {
        let parsed = Url::parse(&base_url).map_err(|_| {
            settings_error(
                ManagerSettingsErrorKind::InvalidUrl,
                SafeSettingsGroup::Stepwise,
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(settings_error(
                ManagerSettingsErrorKind::InvalidUrl,
                SafeSettingsGroup::Stepwise,
            ));
        }
    }
    let api_key_env = input.api_key_env.trim().to_owned();
    if !valid_environment_name(&api_key_env) {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidEnvironmentVariable,
            SafeSettingsGroup::Stepwise,
        ));
    }
    let model = input.model.trim().to_owned();
    if input.enabled && model.is_empty() {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidModel,
            SafeSettingsGroup::Stepwise,
        ));
    }
    if input.max_items > 6
        || !(1000..=24000).contains(&input.max_input_chars)
        || !(100..=4000).contains(&input.max_output_tokens)
        || !(1000..=60000).contains(&input.timeout_ms)
    {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidNumericField,
            SafeSettingsGroup::Stepwise,
        ));
    }
    Ok(NormalizedStepwiseFields {
        enabled: input.enabled,
        direct_send: input.direct_send,
        base_url,
        api_key_env,
        model,
        max_items: input.max_items,
        max_input_chars: input.max_input_chars,
        max_output_tokens: input.max_output_tokens,
        timeout_ms: input.timeout_ms,
    })
}

fn valid_environment_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn stepwise_payload(settings: &NormalizedStepwise) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "codexAppStepwiseEnabled".to_owned(),
        Value::Bool(settings.enabled),
    );
    payload.insert(
        "codexAppStepwiseDirectSend".to_owned(),
        Value::Bool(settings.direct_send),
    );
    payload.insert(
        "codexAppStepwiseBaseUrl".to_owned(),
        Value::String(settings.base_url.clone()),
    );
    payload.insert(
        "codexAppStepwiseApiKeyEnv".to_owned(),
        Value::String(settings.api_key_env.clone()),
    );
    payload.insert(
        "codexAppStepwiseModel".to_owned(),
        Value::String(settings.model.clone()),
    );
    payload.insert(
        "codexAppStepwiseMaxItems".to_owned(),
        Value::from(settings.max_items),
    );
    payload.insert(
        "codexAppStepwiseMaxInputChars".to_owned(),
        Value::from(settings.max_input_chars),
    );
    payload.insert(
        "codexAppStepwiseMaxOutputTokens".to_owned(),
        Value::from(settings.max_output_tokens),
    );
    payload.insert(
        "codexAppStepwiseTimeoutMs".to_owned(),
        Value::from(settings.timeout_ms),
    );
    if let NormalizedSecretChange::Set(value) = &settings.secret {
        payload.insert(
            "codexAppStepwiseApiKey".to_owned(),
            Value::String(value.to_string()),
        );
    }
    Value::Object(payload)
}

struct StepwiseCandidate {
    settings: BackendSettings,
}

impl StepwiseCandidate {
    fn new(current: &BackendSettings, normalized: &NormalizedStepwise) -> Self {
        let direct_key = match &normalized.secret {
            NormalizedSecretChange::Keep => current.codex_app_stepwise_api_key.as_str(),
            NormalizedSecretChange::Set(value) => value.as_str(),
        };
        Self::from_normalized(normalized, direct_key)
    }

    fn with_direct_key(fields: &NormalizedStepwiseFields, direct_key: &str) -> Self {
        let normalized = NormalizedStepwise {
            enabled: fields.enabled,
            direct_send: fields.direct_send,
            base_url: fields.base_url.clone(),
            api_key_env: fields.api_key_env.clone(),
            model: fields.model.clone(),
            max_items: fields.max_items,
            max_input_chars: fields.max_input_chars,
            max_output_tokens: fields.max_output_tokens,
            timeout_ms: fields.timeout_ms,
            secret: NormalizedSecretChange::Keep,
        };
        Self::from_normalized(&normalized, direct_key)
    }

    fn from_normalized(normalized: &NormalizedStepwise, direct_key: &str) -> Self {
        let settings = BackendSettings {
            codex_app_stepwise_enabled: normalized.enabled,
            codex_app_stepwise_direct_send: normalized.direct_send,
            codex_app_stepwise_base_url: normalized.base_url.clone(),
            codex_app_stepwise_api_key: direct_key.to_owned(),
            codex_app_stepwise_api_key_env: normalized.api_key_env.clone(),
            codex_app_stepwise_model: normalized.model.clone(),
            codex_app_stepwise_max_items: normalized.max_items,
            codex_app_stepwise_max_input_chars: normalized.max_input_chars,
            codex_app_stepwise_max_output_tokens: normalized.max_output_tokens,
            codex_app_stepwise_timeout_ms: normalized.timeout_ms,
            ..BackendSettings::default()
        };
        Self { settings }
    }
}

impl Drop for StepwiseCandidate {
    fn drop(&mut self) {
        self.settings.codex_app_stepwise_api_key.zeroize();
    }
}

struct NormalizedImageOverlay {
    enabled: bool,
    path: String,
    opacity: u8,
    fit_mode: ImageOverlayFitMode,
}

fn normalize_extra_args(settings: &ExtraArgsSettings) -> Result<Vec<String>, ManagerSettingsError> {
    let raw = settings
        .args
        .iter()
        .map(|argument| argument.as_str().to_owned())
        .collect::<Vec<_>>();
    if raw.iter().any(|argument| {
        argument.len() > MAX_ARGUMENT_BYTES || argument.contains(['\0', '\r', '\n'])
    }) {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidArgument,
            SafeSettingsGroup::ExtraArgs,
        ));
    }
    let normalized = normalize_codex_extra_args(&raw);
    if normalized.len() > MAX_ARGUMENTS {
        return Err(settings_error(
            ManagerSettingsErrorKind::InvalidArgument,
            SafeSettingsGroup::ExtraArgs,
        ));
    }
    Ok(normalized)
}

#[derive(Serialize)]
struct CanonicalImageOverlay<'a> {
    enabled: bool,
    path: &'a str,
    opacity: u8,
    fit_mode: &'a str,
}

fn image_overlay_fingerprint(settings: &BackendSettings) -> [u8; 32] {
    scoped_fingerprint(
        IMAGE_FINGERPRINT_DOMAIN,
        &CanonicalImageOverlay {
            enabled: settings.codex_app_image_overlay_enabled,
            path: &settings.codex_app_image_overlay_path,
            opacity: settings.codex_app_image_overlay_opacity,
            fit_mode: &settings.codex_app_image_overlay_fit_mode,
        },
    )
}

#[derive(Serialize)]
struct CanonicalExtraArgs<'a> {
    args: &'a [String],
}

fn extra_args_fingerprint(settings: &BackendSettings) -> [u8; 32] {
    let args = normalize_codex_extra_args(&settings.codex_extra_args);
    scoped_fingerprint(
        EXTRA_ARGS_FINGERPRINT_DOMAIN,
        &CanonicalExtraArgs { args: &args },
    )
}

fn settings_error(
    kind: ManagerSettingsErrorKind,
    group: SafeSettingsGroup,
) -> ManagerSettingsError {
    ManagerSettingsError::new(kind, Some(group))
}

fn stepwise_test_error(failure: StepwiseTestFailure) -> ManagerSettingsError {
    let kind = match failure {
        StepwiseTestFailure::Unauthorized => ManagerSettingsErrorKind::StepwiseUnauthorized,
        StepwiseTestFailure::Timeout => ManagerSettingsErrorKind::StepwiseTimeout,
        StepwiseTestFailure::Rejected => ManagerSettingsErrorKind::StepwiseRejected,
        StepwiseTestFailure::Network => ManagerSettingsErrorKind::StepwiseNetwork,
    };
    settings_error(kind, SafeSettingsGroup::Stepwise)
}
