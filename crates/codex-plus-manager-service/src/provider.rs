use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fmt::Write as _;

use codex_plus_core::model_suffix::parse_model_window_token;
use codex_plus_core::relay_config::{
    CodexContextEntries, list_context_entries_from_common_config,
    normalize_relay_profile_for_storage, relay_profile_api_key, relay_profile_base_url,
    relay_profile_model,
};
use codex_plus_core::settings::{AggregateRelayProfile, BackendSettings, RelayMode, RelayProfile};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use crate::{
    DiagnoseProviderProfile, FetchProviderModels, ProviderDoctorReport, ProviderError,
    ProviderErrorKind, ProviderModelsResult, ProviderNetworkEnvironment, ProviderNetworkError,
    ProviderTestResult, TestProviderProfile,
};

const MAX_AGGREGATE_MEMBER_WEIGHT: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Ordinary,
    Aggregate,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ProviderProfile {
    Ordinary(RelayProfile),
    Aggregate {
        shell: RelayProfile,
        routing: AggregateRelayProfile,
    },
}

impl ProviderProfile {
    pub fn id(&self) -> &str {
        match self {
            Self::Ordinary(profile) => &profile.id,
            Self::Aggregate { shell, .. } => &shell.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Ordinary(profile) => &profile.name,
            Self::Aggregate { shell, .. } => &shell.name,
        }
    }

    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Ordinary(_) => ProviderKind::Ordinary,
            Self::Aggregate { .. } => ProviderKind::Aggregate,
        }
    }

    pub fn ordinary(&self) -> Option<&RelayProfile> {
        match self {
            Self::Ordinary(profile) => Some(profile),
            Self::Aggregate { .. } => None,
        }
    }

    pub fn ordinary_mut(&mut self) -> Option<&mut RelayProfile> {
        match self {
            Self::Ordinary(profile) => Some(profile),
            Self::Aggregate { .. } => None,
        }
    }
}

impl fmt::Debug for ProviderProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderProfile")
            .field("id", &self.id())
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderDocument {
    pub profiles: Vec<ProviderProfile>,
    pub common_config_contents: String,
    pub context_config_contents: String,
    pub default_test_model: String,
}

impl fmt::Debug for ProviderDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderDocument")
            .field("profile_count", &self.profiles.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderRevision(String);

impl ProviderRevision {
    pub fn parse(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderActivationSummary {
    pub enabled: bool,
    pub active_profile_id: Option<String>,
    pub active_profile_kind: Option<ProviderKind>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderWorkspace {
    pub revision: ProviderRevision,
    pub document: ProviderDocument,
    pub activation: ProviderActivationSummary,
    pub context_options: CodexContextEntries,
}

impl fmt::Debug for ProviderWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderWorkspace")
            .field("revision", &self.revision)
            .field("profile_count", &self.document.profiles.len())
            .field("activation", &self.activation)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct SaveProviderWorkspace {
    pub expected_revision: ProviderRevision,
    pub document: ProviderDocument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderField {
    Document,
    Id,
    Name,
    Model,
    BaseUrl,
    ApiKey,
    TestModel,
    AggregateRouting,
    AggregateMembers,
    AggregateWeight,
    ModelWindows,
    ContextWindow,
    AutoCompactLimit,
    ConfigContents,
    AuthContents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderValidationKind {
    EmptyId,
    DuplicateId,
    NoOrdinaryProfiles,
    ActiveProfileDeleted,
    AggregateMappingMissing,
    AggregateIdMismatch,
    AggregateMemberMissing,
    AggregateMemberDuplicate,
    AggregateMemberSelfReference,
    AggregateMemberIsAggregate,
    AggregateHasNoValidMember,
    AggregateMemberWeightOutOfRange,
    InvalidModelWindowsJson,
    InvalidModelWindowToken,
    InvalidPositiveInteger,
    InvalidConfigToml,
    InvalidAuthJson,
    NormalizationFailed,
    MissingName,
    MissingModel,
    MissingBaseUrl,
    MissingApiKey,
    MissingTestModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderValidationIssue {
    pub profile_id: Option<String>,
    pub field: ProviderField,
    pub severity: ValidationSeverity,
    pub kind: ProviderValidationKind,
}

impl ProviderValidationIssue {
    fn error(profile_id: Option<&str>, field: ProviderField, kind: ProviderValidationKind) -> Self {
        Self::new(profile_id, field, ValidationSeverity::Error, kind)
    }

    fn warning(
        profile_id: Option<&str>,
        field: ProviderField,
        kind: ProviderValidationKind,
    ) -> Self {
        Self::new(profile_id, field, ValidationSeverity::Warning, kind)
    }

    fn new(
        profile_id: Option<&str>,
        field: ProviderField,
        severity: ValidationSeverity,
        kind: ProviderValidationKind,
    ) -> Self {
        Self {
            profile_id: profile_id.map(ToOwned::to_owned),
            field,
            severity,
            kind,
        }
    }
}

pub trait ProviderEnvironment: Send + Sync + 'static {
    fn load_settings(&self) -> anyhow::Result<BackendSettings>;

    fn update_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool;
}

pub trait ProviderSource: Send + Sync + 'static {
    fn load_workspace(&self) -> Result<ProviderWorkspace, ProviderError>;
    fn save_workspace(
        &self,
        request: SaveProviderWorkspace,
    ) -> Result<ProviderWorkspace, ProviderError>;
    fn test_profile(
        &self,
        request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError>;
    fn fetch_models(
        &self,
        request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError>;
    fn diagnose_profile(
        &self,
        request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError>;
}

pub struct ProviderService<E> {
    environment: E,
}

impl<E> ProviderService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }

    pub(crate) fn environment(&self) -> &E {
        &self.environment
    }
}

impl<E: ProviderEnvironment> ProviderService<E> {
    fn workspace_from_settings(
        &self,
        settings: &BackendSettings,
    ) -> Result<ProviderWorkspace, ProviderError> {
        workspace_from_settings(settings)
    }
}

impl<E: ProviderEnvironment + ProviderNetworkEnvironment> ProviderSource for ProviderService<E> {
    fn load_workspace(&self) -> Result<ProviderWorkspace, ProviderError> {
        let settings = self
            .environment
            .load_settings()
            .map_err(|_| ProviderError::load_failed())?;
        self.workspace_from_settings(&settings)
    }

    fn save_workspace(
        &self,
        request: SaveProviderWorkspace,
    ) -> Result<ProviderWorkspace, ProviderError> {
        let current = self
            .environment
            .load_settings()
            .map_err(|_| ProviderError::load_failed())?;
        let activation = activation_from_settings(&current, &document_from_settings(&current));
        let issues = validate_provider_document(&request.document, &activation);
        if issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error)
        {
            return Err(ProviderError::validation(issues));
        }

        let (relay_profiles, aggregate_relay_profiles) =
            document_profiles_for_storage(&request.document)?;
        let payload = json!({
            "relayProfiles": relay_profiles,
            "aggregateRelayProfiles": aggregate_relay_profiles,
            "relayCommonConfigContents": request.document.common_config_contents,
            "relayContextConfigContents": request.document.context_config_contents,
            "relayTestModel": request.document.default_test_model,
        });
        let expected_revision = request.expected_revision;
        let updated = self
            .environment
            .update_settings_if(payload, |fresh| {
                revision_from_settings(fresh).is_ok_and(|revision| revision == expected_revision)
            })
            .map_err(|_| ProviderError::save_failed())?
            .ok_or_else(ProviderError::conflict)?;

        self.workspace_from_settings(&updated)
    }

    fn test_profile(
        &self,
        request: TestProviderProfile,
    ) -> Result<ProviderTestResult, ProviderNetworkError> {
        self.test_profile_network(request)
    }

    fn fetch_models(
        &self,
        request: FetchProviderModels,
    ) -> Result<ProviderModelsResult, ProviderNetworkError> {
        self.fetch_models_network(request)
    }

    fn diagnose_profile(
        &self,
        request: DiagnoseProviderProfile,
    ) -> Result<ProviderDoctorReport, ProviderNetworkError> {
        self.diagnose_profile_network(request)
    }
}

pub fn validate_provider_document(
    document: &ProviderDocument,
    activation: &ProviderActivationSummary,
) -> Vec<ProviderValidationIssue> {
    let mut issues = Vec::new();
    let mut id_counts = HashMap::<&str, usize>::new();
    let ordinary_count = document
        .profiles
        .iter()
        .filter(|profile| profile.kind() == ProviderKind::Ordinary)
        .count();
    if ordinary_count == 0 {
        issues.push(ProviderValidationIssue::error(
            None,
            ProviderField::Document,
            ProviderValidationKind::NoOrdinaryProfiles,
        ));
    }

    for profile in &document.profiles {
        let id = profile.id().trim();
        if id.is_empty() {
            issues.push(ProviderValidationIssue::error(
                None,
                ProviderField::Id,
                ProviderValidationKind::EmptyId,
            ));
        } else {
            *id_counts.entry(id).or_default() += 1;
        }
    }
    for profile in &document.profiles {
        let id = profile.id().trim();
        if !id.is_empty() && id_counts.get(id).copied().unwrap_or_default() > 1 {
            issues.push(ProviderValidationIssue::error(
                Some(id),
                ProviderField::Id,
                ProviderValidationKind::DuplicateId,
            ));
        }
    }

    if let Some(active_id) = activation.active_profile_id.as_deref()
        && !document
            .profiles
            .iter()
            .any(|profile| profile.id() == active_id)
    {
        issues.push(ProviderValidationIssue::error(
            Some(active_id),
            ProviderField::Document,
            ProviderValidationKind::ActiveProfileDeleted,
        ));
    }

    let ordinary_ids = document
        .profiles
        .iter()
        .filter_map(|profile| match profile {
            ProviderProfile::Ordinary(profile)
                if id_counts
                    .get(profile.id.trim())
                    .copied()
                    .unwrap_or_default()
                    == 1 =>
            {
                Some(profile.id.trim())
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let aggregate_ids = document
        .profiles
        .iter()
        .filter(|profile| profile.kind() == ProviderKind::Aggregate)
        .map(ProviderProfile::id)
        .collect::<HashSet<_>>();

    for profile in &document.profiles {
        match profile {
            ProviderProfile::Ordinary(profile) => {
                validate_ordinary_profile(profile, document, &mut issues);
            }
            ProviderProfile::Aggregate { shell, routing } => {
                validate_aggregate_profile(
                    shell,
                    routing,
                    &ordinary_ids,
                    &aggregate_ids,
                    &mut issues,
                );
            }
        }
    }

    issues
}

fn validate_ordinary_profile(
    profile: &RelayProfile,
    document: &ProviderDocument,
    issues: &mut Vec<ProviderValidationIssue>,
) {
    let id = nonempty(profile.id.trim());
    if profile.relay_mode == RelayMode::Aggregate {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::AggregateRouting,
            ProviderValidationKind::AggregateMappingMissing,
        ));
    }
    if profile.name.trim().is_empty() {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::Name,
            ProviderValidationKind::MissingName,
        ));
    }
    if relay_profile_model(profile).trim().is_empty() && profile.model_list.trim().is_empty() {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::Model,
            ProviderValidationKind::MissingModel,
        ));
    }
    let needs_api = profile.relay_mode != RelayMode::Official || profile.official_mix_api_key;
    if needs_api && relay_profile_base_url(profile).trim().is_empty() {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::BaseUrl,
            ProviderValidationKind::MissingBaseUrl,
        ));
    }
    if needs_api && relay_profile_api_key(profile).trim().is_empty() {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::ApiKey,
            ProviderValidationKind::MissingApiKey,
        ));
    }
    if needs_api
        && profile.test_model.trim().is_empty()
        && relay_profile_model(profile).trim().is_empty()
        && document.default_test_model.trim().is_empty()
    {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::TestModel,
            ProviderValidationKind::MissingTestModel,
        ));
    }

    validate_model_windows(profile, issues);
    validate_positive_integer(
        id,
        ProviderField::ContextWindow,
        &profile.context_window,
        issues,
    );
    validate_positive_integer(
        id,
        ProviderField::AutoCompactLimit,
        &profile.auto_compact_limit,
        issues,
    );
    if !profile.config_contents.trim().is_empty()
        && profile.config_contents.parse::<DocumentMut>().is_err()
    {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::ConfigContents,
            ProviderValidationKind::InvalidConfigToml,
        ));
    }
    if !profile.auth_contents.trim().is_empty()
        && serde_json::from_str::<Value>(&profile.auth_contents)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_none()
    {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::AuthContents,
            ProviderValidationKind::InvalidAuthJson,
        ));
    }
    let mut normalized = profile.clone();
    if normalize_relay_profile_for_storage(&mut normalized).is_err() {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::Document,
            ProviderValidationKind::NormalizationFailed,
        ));
    }
}

fn validate_aggregate_profile<'a>(
    shell: &'a RelayProfile,
    routing: &'a AggregateRelayProfile,
    ordinary_ids: &HashSet<&'a str>,
    aggregate_ids: &HashSet<&'a str>,
    issues: &mut Vec<ProviderValidationIssue>,
) {
    let id = nonempty(shell.id.trim());
    if shell.relay_mode != RelayMode::Aggregate || routing.id.trim().is_empty() {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::AggregateRouting,
            ProviderValidationKind::AggregateMappingMissing,
        ));
    }
    if shell.id != routing.id {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::AggregateRouting,
            ProviderValidationKind::AggregateIdMismatch,
        ));
    }
    if shell.name.trim().is_empty() {
        issues.push(ProviderValidationIssue::warning(
            id,
            ProviderField::Name,
            ProviderValidationKind::MissingName,
        ));
    }

    let mut seen = HashSet::new();
    let mut valid_members = 0usize;
    for member in &routing.members {
        let member_id = member.relay_id.trim();
        if !seen.insert(member_id) {
            issues.push(ProviderValidationIssue::error(
                id,
                ProviderField::AggregateMembers,
                ProviderValidationKind::AggregateMemberDuplicate,
            ));
        }
        if member_id == shell.id {
            issues.push(ProviderValidationIssue::error(
                id,
                ProviderField::AggregateMembers,
                ProviderValidationKind::AggregateMemberSelfReference,
            ));
        } else if aggregate_ids.contains(member_id) {
            issues.push(ProviderValidationIssue::error(
                id,
                ProviderField::AggregateMembers,
                ProviderValidationKind::AggregateMemberIsAggregate,
            ));
        } else if !ordinary_ids.contains(member_id) {
            issues.push(ProviderValidationIssue::error(
                id,
                ProviderField::AggregateMembers,
                ProviderValidationKind::AggregateMemberMissing,
            ));
        } else if member.weight > 0
            && member.weight <= MAX_AGGREGATE_MEMBER_WEIGHT
            && seen.contains(member_id)
        {
            valid_members += 1;
        }
        if member.weight == 0 || member.weight > MAX_AGGREGATE_MEMBER_WEIGHT {
            issues.push(ProviderValidationIssue::error(
                id,
                ProviderField::AggregateWeight,
                ProviderValidationKind::AggregateMemberWeightOutOfRange,
            ));
        }
    }
    if valid_members == 0 {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::AggregateMembers,
            ProviderValidationKind::AggregateHasNoValidMember,
        ));
    }
}

fn validate_model_windows(profile: &RelayProfile, issues: &mut Vec<ProviderValidationIssue>) {
    if profile.model_windows.trim().is_empty() {
        return;
    }
    let id = nonempty(profile.id.trim());
    let Ok(windows) = serde_json::from_str::<BTreeMap<String, String>>(&profile.model_windows)
    else {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::ModelWindows,
            ProviderValidationKind::InvalidModelWindowsJson,
        ));
        return;
    };
    if windows
        .values()
        .any(|token| parse_model_window_token(token).is_none())
    {
        issues.push(ProviderValidationIssue::error(
            id,
            ProviderField::ModelWindows,
            ProviderValidationKind::InvalidModelWindowToken,
        ));
    }
}

fn validate_positive_integer(
    profile_id: Option<&str>,
    field: ProviderField,
    value: &str,
    issues: &mut Vec<ProviderValidationIssue>,
) {
    let value = value.trim();
    if !value.is_empty()
        && value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
    {
        issues.push(ProviderValidationIssue::error(
            profile_id,
            field,
            ProviderValidationKind::InvalidPositiveInteger,
        ));
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn document_from_settings(settings: &BackendSettings) -> ProviderDocument {
    let mut routings = settings.aggregate_relay_profiles.clone();
    let profiles = settings
        .relay_profiles
        .iter()
        .cloned()
        .map(|shell| {
            if shell.relay_mode != RelayMode::Aggregate {
                return ProviderProfile::Ordinary(shell);
            }
            let routing = routings
                .iter()
                .position(|routing| routing.id == shell.id)
                .map(|index| routings.remove(index))
                .unwrap_or_else(|| AggregateRelayProfile {
                    id: String::new(),
                    name: shell.name.clone(),
                    strategy: Default::default(),
                    members: Vec::new(),
                });
            ProviderProfile::Aggregate { shell, routing }
        })
        .collect();
    ProviderDocument {
        profiles,
        common_config_contents: settings.relay_common_config_contents.clone(),
        context_config_contents: settings.relay_context_config_contents.clone(),
        default_test_model: settings.relay_test_model.clone(),
    }
}

fn activation_from_settings(
    settings: &BackendSettings,
    document: &ProviderDocument,
) -> ProviderActivationSummary {
    let active = document
        .profiles
        .iter()
        .find(|profile| profile.id() == settings.active_relay_id);
    ProviderActivationSummary {
        enabled: settings.relay_profiles_enabled,
        active_profile_id: active.map(|profile| profile.id().to_string()),
        active_profile_kind: active.map(ProviderProfile::kind),
    }
}

fn workspace_from_settings(settings: &BackendSettings) -> Result<ProviderWorkspace, ProviderError> {
    let document = document_from_settings(settings);
    let activation = activation_from_settings(settings, &document);
    let context = [
        settings.relay_common_config_contents.trim(),
        settings.relay_context_config_contents.trim(),
    ]
    .into_iter()
    .filter(|section| !section.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");
    let context_options =
        list_context_entries_from_common_config(&context).unwrap_or_else(|_| CodexContextEntries {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        });
    Ok(ProviderWorkspace {
        revision: revision_from_settings(settings)?,
        document,
        activation,
        context_options,
    })
}

fn document_profiles_for_storage(
    document: &ProviderDocument,
) -> Result<(Vec<RelayProfile>, Vec<AggregateRelayProfile>), ProviderError> {
    let mut relay_profiles = Vec::with_capacity(document.profiles.len());
    let mut aggregate_profiles = Vec::new();
    for profile in &document.profiles {
        match profile {
            ProviderProfile::Ordinary(profile) => {
                relay_profiles.push(normalize_profile(profile.clone())?);
            }
            ProviderProfile::Aggregate { shell, routing } => {
                relay_profiles.push(normalize_profile(shell.clone())?);
                aggregate_profiles.push(routing.clone());
            }
        }
    }
    Ok((relay_profiles, aggregate_profiles))
}

fn normalize_profile(mut profile: RelayProfile) -> Result<RelayProfile, ProviderError> {
    normalize_relay_profile_for_storage(&mut profile).map_err(|_| {
        ProviderError::validation(vec![ProviderValidationIssue::error(
            nonempty(profile.id.trim()),
            ProviderField::Document,
            ProviderValidationKind::NormalizationFailed,
        )])
    })?;
    canonicalize_model_windows(&mut profile);
    Ok(profile)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalProviderSettings {
    relay_profiles: Vec<RelayProfile>,
    aggregate_relay_profiles: Vec<AggregateRelayProfile>,
    relay_common_config_contents: String,
    relay_context_config_contents: String,
    relay_test_model: String,
}

fn revision_from_settings(settings: &BackendSettings) -> Result<ProviderRevision, ProviderError> {
    let relay_profiles = settings
        .relay_profiles
        .iter()
        .cloned()
        .map(|profile| {
            let original = profile.clone();
            normalize_profile(profile).unwrap_or_else(|_| {
                let mut original = original;
                canonicalize_model_windows(&mut original);
                original
            })
        })
        .collect();
    let canonical = CanonicalProviderSettings {
        relay_profiles,
        aggregate_relay_profiles: settings.aggregate_relay_profiles.clone(),
        relay_common_config_contents: settings.relay_common_config_contents.clone(),
        relay_context_config_contents: settings.relay_context_config_contents.clone(),
        relay_test_model: settings.relay_test_model.clone(),
    };
    let bytes = serde_json::to_vec(&canonical).map_err(|_| ProviderError::load_failed())?;
    let digest = Sha256::digest(bytes);
    let mut revision = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(revision, "{byte:02x}");
    }
    Ok(ProviderRevision(revision))
}

fn canonicalize_model_windows(profile: &mut RelayProfile) {
    let Ok(windows) = serde_json::from_str::<BTreeMap<String, String>>(&profile.model_windows)
    else {
        return;
    };
    let windows = windows
        .into_iter()
        .map(|(model, token)| {
            let value = parse_model_window_token(&token)
                .map(|value| value.to_string())
                .unwrap_or_else(|| token.trim().to_string());
            (model, value)
        })
        .collect::<BTreeMap<_, _>>();
    profile.model_windows = serde_json::to_string(&windows).unwrap_or_default();
}

impl From<ProviderErrorKind> for ProviderError {
    fn from(kind: ProviderErrorKind) -> Self {
        match kind {
            ProviderErrorKind::LoadFailed => ProviderError::load_failed(),
            ProviderErrorKind::SaveFailed => ProviderError::save_failed(),
            ProviderErrorKind::Conflict => ProviderError::conflict(),
            ProviderErrorKind::Validation => ProviderError::validation(Vec::new()),
        }
    }
}
