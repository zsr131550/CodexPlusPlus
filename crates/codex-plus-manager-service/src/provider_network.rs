use std::collections::HashSet;
use std::fmt;

use codex_plus_core::relay_config::{
    relay_profile_api_key, relay_profile_base_url, relay_profile_model,
};
use codex_plus_core::settings::{RelayMode, RelayProfile};
use serde_json::Value;
use url::Url;

use crate::{ProviderProfile, ProviderService};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderNetworkFailureKind {
    MissingConfiguration,
    InvalidEndpoint,
    Unauthorized,
    NotFound,
    RateLimited,
    UpstreamFailure,
    Timeout,
    Network,
    InvalidResponse,
    AggregateUnsupported,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SafeEndpoint(String);

impl SafeEndpoint {
    pub fn parse(value: &str) -> Option<Self> {
        let mut url = Url::parse(value).ok()?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return None;
        }
        url.set_username("").ok()?;
        url.set_password(None).ok()?;
        url.set_query(None);
        url.set_fragment(None);
        Some(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SafeEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SafeEndpoint")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for SafeEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone)]
pub struct NetworkTestResponse {
    pub http_status: u16,
    pub endpoint: String,
}

#[derive(Clone)]
pub struct NetworkModelsResponse {
    pub models: Vec<String>,
    pub endpoint: String,
}

#[derive(Clone)]
pub struct ProviderEnvironmentNetworkError {
    kind: ProviderNetworkFailureKind,
    http_status: Option<u16>,
    endpoint: Option<String>,
    _technical_detail: String,
}

impl ProviderEnvironmentNetworkError {
    pub fn new(
        kind: ProviderNetworkFailureKind,
        http_status: Option<u16>,
        endpoint: Option<String>,
        technical_detail: String,
    ) -> Self {
        Self {
            kind,
            http_status,
            endpoint,
            _technical_detail: technical_detail,
        }
    }
}

impl fmt::Debug for ProviderEnvironmentNetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderEnvironmentNetworkError")
            .field("kind", &self.kind)
            .field("http_status", &self.http_status)
            .finish_non_exhaustive()
    }
}

pub trait ProviderNetworkEnvironment: Send + Sync + 'static {
    fn test_provider(
        &self,
        profile: &RelayProfile,
        model: &str,
    ) -> Result<NetworkTestResponse, ProviderEnvironmentNetworkError>;

    fn fetch_provider_models(
        &self,
        profile: &RelayProfile,
    ) -> Result<NetworkModelsResponse, ProviderEnvironmentNetworkError>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderNetworkError {
    kind: ProviderNetworkFailureKind,
    http_status: Option<u16>,
    endpoint: Option<SafeEndpoint>,
}

impl ProviderNetworkError {
    fn new(
        kind: ProviderNetworkFailureKind,
        http_status: Option<u16>,
        endpoint: Option<SafeEndpoint>,
    ) -> Self {
        Self {
            kind,
            http_status,
            endpoint,
        }
    }

    pub fn for_failure(
        kind: ProviderNetworkFailureKind,
        http_status: Option<u16>,
        endpoint: Option<SafeEndpoint>,
    ) -> Self {
        Self::new(kind, http_status, endpoint)
    }

    pub fn kind(&self) -> ProviderNetworkFailureKind {
        self.kind
    }

    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    pub fn endpoint(&self) -> Option<&SafeEndpoint> {
        self.endpoint.as_ref()
    }
}

impl fmt::Debug for ProviderNetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderNetworkError")
            .field("kind", &self.kind)
            .field("http_status", &self.http_status)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl fmt::Display for ProviderNetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            ProviderNetworkFailureKind::MissingConfiguration => "provider configuration is missing",
            ProviderNetworkFailureKind::InvalidEndpoint => "provider endpoint is invalid",
            ProviderNetworkFailureKind::Unauthorized => "provider authorization failed",
            ProviderNetworkFailureKind::NotFound => "provider endpoint was not found",
            ProviderNetworkFailureKind::RateLimited => "provider rate limit was reached",
            ProviderNetworkFailureKind::UpstreamFailure => "provider upstream failed",
            ProviderNetworkFailureKind::Timeout => "provider request timed out",
            ProviderNetworkFailureKind::Network => "provider network request failed",
            ProviderNetworkFailureKind::InvalidResponse => "provider returned an invalid response",
            ProviderNetworkFailureKind::AggregateUnsupported => {
                "aggregate providers must be tested through their members"
            }
        })
    }
}

impl std::error::Error for ProviderNetworkError {}

#[derive(Clone)]
pub struct TestProviderProfile {
    pub profile: ProviderProfile,
    pub default_test_model: String,
}

#[derive(Clone)]
pub struct FetchProviderModels {
    pub profile: ProviderProfile,
}

#[derive(Clone)]
pub struct DiagnoseProviderProfile {
    pub profile: ProviderProfile,
    pub default_test_model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTestOutcome {
    Success,
    OfficialNoApiRequired,
    Failure(ProviderNetworkFailureKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTestResult {
    pub http_status: Option<u16>,
    pub endpoint: Option<SafeEndpoint>,
    pub outcome: ProviderTestOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelsResult {
    pub models: Vec<String>,
    pub endpoint: SafeEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderDoctorCheckId {
    Config,
    Models,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorCheckStatus {
    Passed,
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorDetailKind {
    ConfigReady,
    MissingConfiguration,
    InvalidEndpoint,
    OfficialNoApiRequired,
    AggregateUnsupported,
    ModelsAvailable,
    ModelsUnavailable,
    TestModelMissing,
    RequestSucceeded,
    RequestFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDoctorCheck {
    pub id: ProviderDoctorCheckId,
    pub status: DoctorCheckStatus,
    pub detail: DoctorDetailKind,
    pub failure: Option<ProviderNetworkFailureKind>,
    pub http_status: Option<u16>,
    pub endpoint: Option<SafeEndpoint>,
    pub model_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorOutcome {
    Passed,
    Warning,
    Failed,
    OfficialNoApiRequired,
    AggregateUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorRecommendation {
    Ready,
    CompleteConfiguration,
    CheckModelsEndpoint,
    UseDiscoveredModel,
    CheckCredentialsOrProtocol,
    UseOfficialLogin,
    TestAggregateMembers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDoctorReport {
    pub profile_name: String,
    pub model: String,
    pub outcome: DoctorOutcome,
    pub recommendation: DoctorRecommendation,
    pub checks: Vec<ProviderDoctorCheck>,
}

impl<E: ProviderNetworkEnvironment> ProviderService<E> {
    pub(crate) fn test_profile_network(
        &self,
        request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError> {
        let ProviderProfile::Ordinary(profile) = request.profile else {
            return Ok(ProviderTestResult {
                http_status: None,
                endpoint: None,
                outcome: ProviderTestOutcome::Failure(
                    ProviderNetworkFailureKind::AggregateUnsupported,
                ),
            });
        };
        if is_official_login(&profile) {
            return Ok(ProviderTestResult {
                http_status: None,
                endpoint: None,
                outcome: ProviderTestOutcome::OfficialNoApiRequired,
            });
        }
        let model = resolved_test_model(&profile, &request.default_test_model);
        let base_url = relay_profile_base_url(&profile);
        if base_url.trim().is_empty()
            || relay_profile_api_key(&profile).trim().is_empty()
            || model.is_empty()
        {
            return Ok(ProviderTestResult {
                http_status: None,
                endpoint: SafeEndpoint::parse(&base_url),
                outcome: ProviderTestOutcome::Failure(
                    ProviderNetworkFailureKind::MissingConfiguration,
                ),
            });
        }
        if SafeEndpoint::parse(&base_url).is_none() {
            return Ok(ProviderTestResult {
                http_status: None,
                endpoint: None,
                outcome: ProviderTestOutcome::Failure(ProviderNetworkFailureKind::InvalidEndpoint),
            });
        }
        let response = self
            .environment()
            .test_provider(&profile, &model)
            .map_err(public_network_error)?;
        let endpoint = SafeEndpoint::parse(&response.endpoint).ok_or_else(|| {
            ProviderNetworkError::new(ProviderNetworkFailureKind::InvalidEndpoint, None, None)
        })?;
        Ok(ProviderTestResult {
            http_status: Some(response.http_status),
            endpoint: Some(endpoint),
            outcome: outcome_for_status(response.http_status),
        })
    }

    pub(crate) fn fetch_models_network(
        &self,
        request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        let ProviderProfile::Ordinary(profile) = request.profile else {
            return Err(ProviderNetworkError::new(
                ProviderNetworkFailureKind::AggregateUnsupported,
                None,
                None,
            ));
        };
        self.fetch_models_for_profile(&profile)
    }

    pub(crate) fn diagnose_profile_network(
        &self,
        request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError> {
        let profile_name = request.profile.name().trim().to_string();
        let ProviderProfile::Ordinary(profile) = request.profile else {
            return Ok(ProviderDoctorReport {
                profile_name,
                model: String::new(),
                outcome: DoctorOutcome::AggregateUnsupported,
                recommendation: DoctorRecommendation::TestAggregateMembers,
                checks: vec![doctor_check(
                    ProviderDoctorCheckId::Config,
                    DoctorCheckStatus::Skipped,
                    DoctorDetailKind::AggregateUnsupported,
                )],
            });
        };
        let model = resolved_test_model(&profile, &request.default_test_model);
        if is_official_login(&profile) {
            return Ok(ProviderDoctorReport {
                profile_name,
                model,
                outcome: DoctorOutcome::OfficialNoApiRequired,
                recommendation: DoctorRecommendation::UseOfficialLogin,
                checks: vec![doctor_check(
                    ProviderDoctorCheckId::Config,
                    DoctorCheckStatus::Passed,
                    DoctorDetailKind::OfficialNoApiRequired,
                )],
            });
        }

        let base_url = relay_profile_base_url(&profile);
        let missing = base_url.trim().is_empty()
            || relay_profile_api_key(&profile).trim().is_empty()
            || model.trim().is_empty();
        if missing {
            return Ok(ProviderDoctorReport {
                profile_name,
                model,
                outcome: DoctorOutcome::Failed,
                recommendation: DoctorRecommendation::CompleteConfiguration,
                checks: vec![doctor_check(
                    ProviderDoctorCheckId::Config,
                    DoctorCheckStatus::Failed,
                    DoctorDetailKind::MissingConfiguration,
                )],
            });
        }
        let Some(config_endpoint) = SafeEndpoint::parse(&base_url) else {
            return Ok(ProviderDoctorReport {
                profile_name,
                model,
                outcome: DoctorOutcome::Failed,
                recommendation: DoctorRecommendation::CompleteConfiguration,
                checks: vec![doctor_check(
                    ProviderDoctorCheckId::Config,
                    DoctorCheckStatus::Failed,
                    DoctorDetailKind::InvalidEndpoint,
                )],
            });
        };

        let mut config_check = doctor_check(
            ProviderDoctorCheckId::Config,
            DoctorCheckStatus::Passed,
            DoctorDetailKind::ConfigReady,
        );
        config_check.endpoint = Some(config_endpoint);
        let mut checks = vec![config_check];
        let mut model_failed = false;
        let mut model_missing = false;
        match self.fetch_models_for_profile(&profile) {
            Ok(result) => {
                model_missing =
                    !model.is_empty() && !result.models.iter().any(|item| item == &model);
                let mut check = doctor_check(
                    ProviderDoctorCheckId::Models,
                    if model_missing {
                        DoctorCheckStatus::Warning
                    } else {
                        DoctorCheckStatus::Passed
                    },
                    if model_missing {
                        DoctorDetailKind::TestModelMissing
                    } else {
                        DoctorDetailKind::ModelsAvailable
                    },
                );
                check.endpoint = Some(result.endpoint);
                check.model_count = Some(result.models.len());
                checks.push(check);
            }
            Err(error) => {
                model_failed = true;
                checks.push(doctor_network_failure(
                    ProviderDoctorCheckId::Models,
                    DoctorDetailKind::ModelsUnavailable,
                    &error,
                ));
            }
        }

        let request_failed = match self.environment().test_provider(&profile, &model) {
            Ok(response) => {
                let outcome = outcome_for_status(response.http_status);
                let request_failed = outcome != ProviderTestOutcome::Success;
                let mut check = doctor_check(
                    ProviderDoctorCheckId::Request,
                    if request_failed {
                        DoctorCheckStatus::Failed
                    } else {
                        DoctorCheckStatus::Passed
                    },
                    if request_failed {
                        DoctorDetailKind::RequestFailed
                    } else {
                        DoctorDetailKind::RequestSucceeded
                    },
                );
                check.http_status = Some(response.http_status);
                check.endpoint = SafeEndpoint::parse(&response.endpoint);
                if let ProviderTestOutcome::Failure(failure) = outcome {
                    check.failure = Some(failure);
                }
                checks.push(check);
                request_failed
            }
            Err(error) => {
                let error = public_network_error(error);
                checks.push(doctor_network_failure(
                    ProviderDoctorCheckId::Request,
                    DoctorDetailKind::RequestFailed,
                    &error,
                ));
                true
            }
        };

        let (outcome, recommendation) = if request_failed {
            (
                DoctorOutcome::Failed,
                DoctorRecommendation::CheckCredentialsOrProtocol,
            )
        } else if model_failed {
            (
                DoctorOutcome::Failed,
                DoctorRecommendation::CheckModelsEndpoint,
            )
        } else if model_missing {
            (
                DoctorOutcome::Warning,
                DoctorRecommendation::UseDiscoveredModel,
            )
        } else {
            (DoctorOutcome::Passed, DoctorRecommendation::Ready)
        };
        Ok(ProviderDoctorReport {
            profile_name,
            model,
            outcome,
            recommendation,
            checks,
        })
    }

    fn fetch_models_for_profile(
        &self,
        profile: &RelayProfile,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        let base_url = relay_profile_base_url(profile);
        if base_url.trim().is_empty() || relay_profile_api_key(profile).trim().is_empty() {
            return Err(ProviderNetworkError::new(
                ProviderNetworkFailureKind::MissingConfiguration,
                None,
                SafeEndpoint::parse(&base_url),
            ));
        }
        if SafeEndpoint::parse(&base_url).is_none() {
            return Err(ProviderNetworkError::new(
                ProviderNetworkFailureKind::InvalidEndpoint,
                None,
                None,
            ));
        }
        let response = self
            .environment()
            .fetch_provider_models(profile)
            .map_err(public_network_error)?;
        let endpoint = SafeEndpoint::parse(&response.endpoint).ok_or_else(|| {
            ProviderNetworkError::new(ProviderNetworkFailureKind::InvalidEndpoint, None, None)
        })?;
        let mut seen = HashSet::new();
        let models = response
            .models
            .into_iter()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty() && seen.insert(model.clone()))
            .collect::<Vec<_>>();
        if models.is_empty() {
            return Err(ProviderNetworkError::new(
                ProviderNetworkFailureKind::InvalidResponse,
                None,
                Some(endpoint),
            ));
        }
        Ok(ProviderModelsResult { models, endpoint })
    }
}

fn is_official_login(profile: &RelayProfile) -> bool {
    profile.relay_mode == RelayMode::Official && !profile.official_mix_api_key
}

fn resolved_test_model(profile: &RelayProfile, default_model: &str) -> String {
    if !profile.test_model.trim().is_empty() {
        profile.test_model.trim().to_string()
    } else {
        let profile_model = relay_profile_model(profile);
        if profile_model.trim().is_empty() {
            default_model.trim().to_string()
        } else {
            profile_model.trim().to_string()
        }
    }
}

fn outcome_for_status(status: u16) -> ProviderTestOutcome {
    if status < 400 {
        return ProviderTestOutcome::Success;
    }
    ProviderTestOutcome::Failure(match status {
        401 | 403 => ProviderNetworkFailureKind::Unauthorized,
        404 => ProviderNetworkFailureKind::NotFound,
        429 => ProviderNetworkFailureKind::RateLimited,
        500..=599 => ProviderNetworkFailureKind::UpstreamFailure,
        _ => ProviderNetworkFailureKind::UpstreamFailure,
    })
}

fn public_network_error(error: ProviderEnvironmentNetworkError) -> ProviderNetworkError {
    ProviderNetworkError::new(
        error.kind,
        error.http_status,
        error.endpoint.as_deref().and_then(SafeEndpoint::parse),
    )
}

fn doctor_check(
    id: ProviderDoctorCheckId,
    status: DoctorCheckStatus,
    detail: DoctorDetailKind,
) -> ProviderDoctorCheck {
    ProviderDoctorCheck {
        id,
        status,
        detail,
        failure: None,
        http_status: None,
        endpoint: None,
        model_count: None,
    }
}

fn doctor_network_failure(
    id: ProviderDoctorCheckId,
    detail: DoctorDetailKind,
    error: &ProviderNetworkError,
) -> ProviderDoctorCheck {
    ProviderDoctorCheck {
        id,
        status: DoctorCheckStatus::Failed,
        detail,
        failure: Some(error.kind),
        http_status: error.http_status,
        endpoint: error.endpoint.clone(),
        model_count: None,
    }
}

pub fn masked_auth_preview(contents: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(contents) else {
        return "<invalid auth JSON>".to_string();
    };
    mask_json_scalars(&mut value);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "<invalid auth JSON>".to_string())
}

fn mask_json_scalars(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(mask_json_scalars),
        Value::Object(values) => values.values_mut().for_each(mask_json_scalars),
        _ => *value = Value::String("***".to_string()),
    }
}

pub fn masked_config_preview(contents: &str) -> String {
    let Ok(mut value) = contents.parse::<toml::Value>() else {
        return "<invalid config TOML>".to_string();
    };
    mask_toml_value(&mut value, false);
    toml::to_string_pretty(&value).unwrap_or_else(|_| "<invalid config TOML>".to_string())
}

fn mask_toml_value(value: &mut toml::Value, force_mask: bool) {
    if force_mask {
        *value = toml::Value::String("***".to_string());
        return;
    }
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                mask_toml_value(value, sensitive_key(key));
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                mask_toml_value(value, false);
            }
        }
        _ => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "authorization",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|marker| key.contains(marker))
        || key == "key"
        || key.ends_with("_key")
}
