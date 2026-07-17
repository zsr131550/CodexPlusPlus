use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

use codex_plus_core::settings::{RelayMode, RelayProfile, RelayProtocol};
use serde::Deserialize;
use url::Url;

const PROVIDER_PRESETS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/provider-presets.json"
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPresetCategory {
    Official,
    Aggregator,
    ThirdParty,
    ChineseOfficial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub website_url: Option<String>,
    pub api_key_url: Option<String>,
    pub category: ProviderPresetCategory,
    pub base_url: String,
    pub protocol: RelayProtocol,
    pub model: String,
    pub model_list: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPresetCatalogErrorKind {
    InvalidJson,
    EmptyId,
    DuplicateId,
    InvalidCategory,
    InvalidProtocol,
    InvalidUrl,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderPresetCatalogError {
    kind: ProviderPresetCatalogErrorKind,
}

impl ProviderPresetCatalogError {
    fn new(kind: ProviderPresetCatalogErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> ProviderPresetCatalogErrorKind {
        self.kind
    }
}

impl fmt::Debug for ProviderPresetCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderPresetCatalogError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ProviderPresetCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            ProviderPresetCatalogErrorKind::InvalidJson => "invalid provider preset catalog",
            ProviderPresetCatalogErrorKind::EmptyId => "provider preset id is empty",
            ProviderPresetCatalogErrorKind::DuplicateId => "provider preset id is duplicated",
            ProviderPresetCatalogErrorKind::InvalidCategory => {
                "provider preset category is invalid"
            }
            ProviderPresetCatalogErrorKind::InvalidProtocol => {
                "provider preset protocol is invalid"
            }
            ProviderPresetCatalogErrorKind::InvalidUrl => "provider preset URL is invalid",
        })
    }
}

impl std::error::Error for ProviderPresetCatalogError {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProviderPreset {
    id: String,
    name: String,
    website_url: Option<String>,
    api_key_url: Option<String>,
    category: String,
    base_url: String,
    protocol: String,
    model: String,
    #[serde(default)]
    model_list: Vec<String>,
}

pub fn provider_presets() -> Result<&'static [ProviderPreset], ProviderPresetCatalogError> {
    static PRESETS: OnceLock<Result<Vec<ProviderPreset>, ProviderPresetCatalogError>> =
        OnceLock::new();
    match PRESETS.get_or_init(|| parse_provider_presets(PROVIDER_PRESETS_JSON)) {
        Ok(presets) => Ok(presets),
        Err(error) => Err(error.clone()),
    }
}

pub fn parse_provider_presets(
    contents: &str,
) -> Result<Vec<ProviderPreset>, ProviderPresetCatalogError> {
    let raw = serde_json::from_str::<Vec<RawProviderPreset>>(contents).map_err(|_| {
        ProviderPresetCatalogError::new(ProviderPresetCatalogErrorKind::InvalidJson)
    })?;
    let mut seen = HashSet::new();
    raw.into_iter()
        .map(|preset| {
            let id = preset.id.trim().to_string();
            if id.is_empty() {
                return Err(ProviderPresetCatalogError::new(
                    ProviderPresetCatalogErrorKind::EmptyId,
                ));
            }
            if !seen.insert(id.clone()) {
                return Err(ProviderPresetCatalogError::new(
                    ProviderPresetCatalogErrorKind::DuplicateId,
                ));
            }
            let category = match preset.category.as_str() {
                "official" => ProviderPresetCategory::Official,
                "aggregator" => ProviderPresetCategory::Aggregator,
                "third_party" => ProviderPresetCategory::ThirdParty,
                "cn_official" => ProviderPresetCategory::ChineseOfficial,
                _ => {
                    return Err(ProviderPresetCatalogError::new(
                        ProviderPresetCatalogErrorKind::InvalidCategory,
                    ));
                }
            };
            let protocol = match preset.protocol.as_str() {
                "responses" => RelayProtocol::Responses,
                "chatCompletions" => RelayProtocol::ChatCompletions,
                _ => {
                    return Err(ProviderPresetCatalogError::new(
                        ProviderPresetCatalogErrorKind::InvalidProtocol,
                    ));
                }
            };
            validate_url(&preset.base_url)?;
            if let Some(url) = preset.website_url.as_deref() {
                validate_url(url)?;
            }
            if let Some(url) = preset.api_key_url.as_deref() {
                validate_url(url)?;
            }
            Ok(ProviderPreset {
                id,
                name: preset.name,
                website_url: preset.website_url,
                api_key_url: preset.api_key_url,
                category,
                base_url: preset.base_url,
                protocol,
                model: preset.model,
                model_list: preset.model_list,
            })
        })
        .collect()
}

pub fn apply_provider_preset(profile: &mut RelayProfile, preset: &ProviderPreset) {
    profile.name = preset.name.clone();
    profile.base_url = preset.base_url.clone();
    profile.upstream_base_url = preset.base_url.clone();
    profile.protocol = preset.protocol;
    profile.model = preset.model.clone();
    profile.test_model = preset.model.clone();
    profile.model_list = preset.model_list.join("\n");
    profile.relay_mode = if preset.category == ProviderPresetCategory::Official {
        RelayMode::Official
    } else {
        RelayMode::PureApi
    };
    profile.official_mix_api_key = false;
}

fn validate_url(value: &str) -> Result<(), ProviderPresetCatalogError> {
    let url = Url::parse(value)
        .map_err(|_| ProviderPresetCatalogError::new(ProviderPresetCatalogErrorKind::InvalidUrl))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ProviderPresetCatalogError::new(
            ProviderPresetCatalogErrorKind::InvalidUrl,
        ));
    }
    Ok(())
}
