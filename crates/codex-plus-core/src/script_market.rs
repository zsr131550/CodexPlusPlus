use std::fmt;
use std::net::IpAddr;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::user_scripts::UserScriptManager;

pub const DEFAULT_MARKET_INDEX_URL: &str =
    "https://raw.githubusercontent.com/BigPizzaV3/CodexPlusPlusScriptMarket/main/index.json";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_SCRIPT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketScriptIntegrity {
    Verified,
    Unverified,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMarketErrorKind {
    InvalidUrl,
    InsecureTransport,
    RequestFailed,
    ResponseTooLarge,
    DecodeFailed,
    InvalidIntegrity,
    IntegrityMismatch,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScriptMarketError {
    kind: ScriptMarketErrorKind,
}

impl ScriptMarketError {
    fn new(kind: ScriptMarketErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> ScriptMarketErrorKind {
        self.kind
    }
}

impl fmt::Debug for ScriptMarketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptMarketError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ScriptMarketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            ScriptMarketErrorKind::InvalidUrl => "invalid script market URL",
            ScriptMarketErrorKind::InsecureTransport => "script market transport must use HTTPS",
            ScriptMarketErrorKind::RequestFailed => "script market request failed",
            ScriptMarketErrorKind::ResponseTooLarge => "script market response is too large",
            ScriptMarketErrorKind::DecodeFailed => "script market response is invalid",
            ScriptMarketErrorKind::InvalidIntegrity => "script digest is invalid",
            ScriptMarketErrorKind::IntegrityMismatch => "script digest does not match content",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ScriptMarketError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFetchPolicy {
    allow_loopback_http: bool,
}

impl MarketFetchPolicy {
    pub const fn https_only() -> Self {
        Self {
            allow_loopback_http: false,
        }
    }

    pub const fn loopback_http_for_tests() -> Self {
        Self {
            allow_loopback_http: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptMarketManifest {
    pub version: u64,
    pub updated_at: Option<String>,
    pub scripts: Vec<MarketScript>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketScript {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub homepage: String,
    pub script_url: String,
    #[serde(default)]
    pub sha256: String,
}

pub struct PreparedMarketScript {
    pub(crate) script: MarketScript,
    pub(crate) content: Vec<u8>,
    integrity: MarketScriptIntegrity,
}

impl PreparedMarketScript {
    pub fn id(&self) -> &str {
        &self.script.id
    }

    pub fn version(&self) -> &str {
        &self.script.version
    }

    pub fn integrity(&self) -> MarketScriptIntegrity {
        self.integrity
    }

    pub fn byte_count(&self) -> usize {
        self.content.len()
    }
}

impl fmt::Debug for PreparedMarketScript {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedMarketScript")
            .field("id", &self.script.id)
            .field("version", &self.script.version)
            .field("integrity", &self.integrity)
            .field("byte_count", &self.content.len())
            .finish()
    }
}

pub fn parse_market_manifest(raw: Value) -> anyhow::Result<ScriptMarketManifest> {
    let version = raw.get("version").and_then(Value::as_u64).unwrap_or(1);
    let updated_at = raw
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let scripts = raw
        .get("scripts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_market_script)
        .collect();

    Ok(ScriptMarketManifest {
        version,
        updated_at,
        scripts,
    })
}

pub async fn fetch_market_manifest(url: &str) -> Result<ScriptMarketManifest, ScriptMarketError> {
    fetch_market_manifest_with_policy(url, MarketFetchPolicy::https_only()).await
}

pub async fn fetch_market_manifest_with_policy(
    url: &str,
    policy: MarketFetchPolicy,
) -> Result<ScriptMarketManifest, ScriptMarketError> {
    let bytes = fetch_bounded(url, MAX_MANIFEST_BYTES, policy).await?;
    let raw = serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::DecodeFailed))?;
    parse_market_manifest(raw)
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::DecodeFailed))
}

pub async fn download_script(url: &str) -> Result<Vec<u8>, ScriptMarketError> {
    download_script_with_policy(url, MarketFetchPolicy::https_only()).await
}

pub async fn download_script_with_policy(
    url: &str,
    policy: MarketFetchPolicy,
) -> Result<Vec<u8>, ScriptMarketError> {
    fetch_bounded(url, MAX_SCRIPT_BYTES, policy).await
}

pub fn classify_digest(digest: &str) -> MarketScriptIntegrity {
    let digest = digest.trim();
    if digest.is_empty() {
        MarketScriptIntegrity::Unverified
    } else if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        MarketScriptIntegrity::Verified
    } else {
        MarketScriptIntegrity::Invalid
    }
}

pub fn prepare_market_script_content(
    script: &MarketScript,
    content: &[u8],
) -> Result<PreparedMarketScript, ScriptMarketError> {
    if content.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptMarketError::new(
            ScriptMarketErrorKind::ResponseTooLarge,
        ));
    }
    let integrity = classify_digest(&script.sha256);
    match integrity {
        MarketScriptIntegrity::Invalid => {
            return Err(ScriptMarketError::new(
                ScriptMarketErrorKind::InvalidIntegrity,
            ));
        }
        MarketScriptIntegrity::Verified => {
            let actual = format!("{:x}", Sha256::digest(content));
            if !actual.eq_ignore_ascii_case(script.sha256.trim()) {
                return Err(ScriptMarketError::new(
                    ScriptMarketErrorKind::IntegrityMismatch,
                ));
            }
        }
        MarketScriptIntegrity::Unverified => {}
    }
    Ok(PreparedMarketScript {
        script: script.clone(),
        content: content.to_vec(),
        integrity,
    })
}

pub fn install_market_script_content(
    manager: &UserScriptManager,
    script: &MarketScript,
    content: &[u8],
) -> anyhow::Result<()> {
    let prepared = prepare_market_script_content(script, content)?;
    let inspection = manager.inspect()?;
    manager.commit_market_script(&inspection.revision, &prepared)?;
    Ok(())
}

pub async fn install_market_script(
    manager: &UserScriptManager,
    script: &MarketScript,
) -> anyhow::Result<()> {
    let content = download_script(&script.script_url).await?;
    install_market_script_content(manager, script, &content)
}

pub fn validate_market_url(
    url: &str,
    policy: MarketFetchPolicy,
) -> Result<reqwest::Url, ScriptMarketError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::InvalidUrl))?;
    if parsed.username() != "" || parsed.password().is_some() {
        return Err(ScriptMarketError::new(ScriptMarketErrorKind::InvalidUrl));
    }
    if url_is_allowed(&parsed, policy) {
        Ok(parsed)
    } else {
        Err(ScriptMarketError::new(
            ScriptMarketErrorKind::InsecureTransport,
        ))
    }
}

async fn fetch_bounded(
    url: &str,
    limit: usize,
    policy: MarketFetchPolicy,
) -> Result<Vec<u8>, ScriptMarketError> {
    let parsed = validate_market_url(url, policy)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            if url_is_allowed(attempt.url(), policy) {
                attempt.follow()
            } else {
                attempt.error("redirect violates transport policy")
            }
        }))
        .build()
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::RequestFailed))?;
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::RequestFailed))?
        .error_for_status()
        .map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::RequestFailed))?;
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(ScriptMarketError::new(
            ScriptMarketErrorKind::ResponseTooLarge,
        ));
    }

    let capacity = response
        .content_length()
        .and_then(|size| usize::try_from(size).ok())
        .unwrap_or(0)
        .min(limit);
    let mut content = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| ScriptMarketError::new(ScriptMarketErrorKind::RequestFailed))?;
        if chunk.len() > limit.saturating_sub(content.len()) {
            return Err(ScriptMarketError::new(
                ScriptMarketErrorKind::ResponseTooLarge,
            ));
        }
        content.extend_from_slice(&chunk);
    }
    Ok(content)
}

fn url_is_allowed(url: &reqwest::Url, policy: MarketFetchPolicy) -> bool {
    match url.scheme() {
        "https" => true,
        "http" if policy.allow_loopback_http => url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("localhost") || is_loopback_ip(host)),
        _ => false,
    }
}

fn is_loopback_ip(host: &str) -> bool {
    host.parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn parse_market_script(raw: Value) -> Option<MarketScript> {
    let id = required_string(&raw, "id")?;
    let name = required_string(&raw, "name")?;
    let version = required_string(&raw, "version")?;
    let script_url = required_string(&raw, "script_url")?;
    Some(MarketScript {
        id,
        name,
        description: optional_string(&raw, "description"),
        version,
        author: optional_string(&raw, "author"),
        tags: raw
            .get("tags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        homepage: optional_string(&raw, "homepage"),
        script_url,
        sha256: optional_string(&raw, "sha256"),
    })
}

fn required_string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_string(raw: &Value, key: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}
