use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table};

const OPENAI_CURATED_MARKETPLACE: &str = "openai-curated";
const OPENAI_API_CURATED_MARKETPLACE: &str = "openai-api-curated";
const OPENAI_CURATED_REMOTE_MARKETPLACE: &str = "openai-curated-remote";
const ROLE_SPECIFIC_PLUGINS_MARKETPLACE: &str = "role-specific-plugins";
const OPENAI_PLUGINS_ZIP_URL: &str =
    "https://codeload.github.com/openai/plugins/zip/refs/heads/main";
const OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const OPENAI_CURATED_REMOTE_MARKETPLACE_ZIP: &[u8] =
    include_bytes!("../../../assets/plugin-marketplaces/openai-curated-remote.zip");

pub fn ensure_openai_curated_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_marketplace_root(home)? else {
        return Ok(false);
    };
    let mut changed = ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_MARKETPLACE, OPENAI_API_CURATED_MARKETPLACE],
        &marketplace_root,
    )?;
    if let Some(remote_marketplace_root) = local_openai_curated_remote_marketplace_root(home)? {
        changed |= ensure_marketplace_configs(
            home,
            &[OPENAI_CURATED_REMOTE_MARKETPLACE],
            &remote_marketplace_root,
        )?;
    }
    Ok(changed)
}

pub fn ensure_openai_curated_remote_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_remote_marketplace_root(home)? else {
        return Ok(false);
    };
    ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_REMOTE_MARKETPLACE],
        &marketplace_root,
    )
}

pub fn ensure_role_specific_plugins_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_role_specific_plugins_marketplace_root(home)? else {
        return Ok(false);
    };
    let plugin_ids =
        local_marketplace_plugin_names(&marketplace_root, ROLE_SPECIFIC_PLUGINS_MARKETPLACE)?
            .into_iter()
            .map(|name| format!("{name}@{ROLE_SPECIFIC_PLUGINS_MARKETPLACE}"))
            .collect::<Vec<_>>();
    ensure_marketplace_configs_with_plugins(
        home,
        &[ROLE_SPECIFIC_PLUGINS_MARKETPLACE],
        &marketplace_root,
        &plugin_ids,
    )
}

pub fn ensure_openai_curated_remote_marketplace_available(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let prepared = prepare_remote_plugin_marketplace(home)?;
    commit_prepared_plugin_marketplace(home, prepared)
}

pub fn preserve_openai_curated_remote_marketplace_config(
    home: &Path,
    config_text: &str,
) -> anyhow::Result<String> {
    let Some(marketplace_root) = local_openai_curated_remote_marketplace_root(home)? else {
        return Ok(config_text.to_string());
    };
    merge_marketplace_configs_into_text(
        config_text,
        &[OPENAI_CURATED_REMOTE_MARKETPLACE],
        &marketplace_root,
    )
}

pub fn openai_curated_marketplace_status(home: &Path) -> MarketplaceStatus {
    let marketplace_root = local_openai_curated_marketplace_root(home).ok().flatten();
    let remote_marketplace_root = local_openai_curated_remote_marketplace_root(home)
        .ok()
        .flatten();
    let config_registered = marketplace_root
        .as_deref()
        .map(|root| {
            marketplace_config_points_to_root(home, OPENAI_CURATED_MARKETPLACE, root)
                && marketplace_config_points_to_root(home, OPENAI_API_CURATED_MARKETPLACE, root)
                && remote_marketplace_root
                    .as_deref()
                    .map(|remote_root| {
                        marketplace_config_points_to_root(
                            home,
                            OPENAI_CURATED_REMOTE_MARKETPLACE,
                            remote_root,
                        )
                    })
                    .unwrap_or(true)
        })
        .unwrap_or(false);
    MarketplaceStatus {
        marketplace_root,
        config_registered,
    }
}

pub fn openai_curated_remote_marketplace_status(home: &Path) -> MarketplaceStatus {
    let marketplace_root = local_openai_curated_remote_marketplace_root(home)
        .ok()
        .flatten();
    let config_registered = marketplace_root
        .as_deref()
        .map(|root| {
            marketplace_config_points_to_root(home, OPENAI_CURATED_REMOTE_MARKETPLACE, root)
        })
        .unwrap_or(false);
    MarketplaceStatus {
        marketplace_root,
        config_registered,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplaceStatus {
    pub marketplace_root: Option<PathBuf>,
    pub config_registered: bool,
}

impl MarketplaceStatus {
    pub fn needs_repair(&self) -> bool {
        self.marketplace_root.is_none() || !self.config_registered
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMarketplaceKind {
    Local,
    Remote,
}

#[derive(Clone, PartialEq, Eq)]
pub struct PluginMarketplaceRecord {
    pub kind: PluginMarketplaceKind,
    pub available: bool,
    pub config_registered: bool,
    pub plugin_count: usize,
    pub skill_count: usize,
    pub marketplace_root: Option<PathBuf>,
    directory_identity: [u8; 32],
}

impl PluginMarketplaceRecord {
    pub fn needs_repair(&self) -> bool {
        !self.available || !self.config_registered
    }

    pub fn directory_identity(&self) -> &[u8; 32] {
        &self.directory_identity
    }
}

impl std::fmt::Debug for PluginMarketplaceRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginMarketplaceRecord")
            .field("kind", &self.kind)
            .field("available", &self.available)
            .field("config_registered", &self.config_registered)
            .field("plugin_count", &self.plugin_count)
            .field("skill_count", &self.skill_count)
            .field("has_marketplace_root", &self.marketplace_root.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PluginMarketplaceInspection {
    pub local: PluginMarketplaceRecord,
    pub remote: PluginMarketplaceRecord,
    config_bytes: Vec<u8>,
}

impl PluginMarketplaceInspection {
    pub fn config_bytes(&self) -> &[u8] {
        &self.config_bytes
    }
}

impl std::fmt::Debug for PluginMarketplaceInspection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginMarketplaceInspection")
            .field("local", &self.local)
            .field("remote", &self.remote)
            .finish_non_exhaustive()
    }
}

pub fn inspect_plugin_marketplaces(home: &Path) -> anyhow::Result<PluginMarketplaceInspection> {
    let config_path = home.join("config.toml");
    let config_bytes = match std::fs::read(&config_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let config_text = std::str::from_utf8(&config_bytes)
        .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?;
    let local = inspect_marketplace(
        PluginMarketplaceKind::Local,
        home.join(".tmp").join("plugins"),
        OPENAI_CURATED_MARKETPLACE,
        &[OPENAI_CURATED_MARKETPLACE, OPENAI_API_CURATED_MARKETPLACE],
        config_text,
    )?;
    let remote = inspect_marketplace(
        PluginMarketplaceKind::Remote,
        home.join(".tmp").join("plugins-remote"),
        OPENAI_CURATED_REMOTE_MARKETPLACE,
        &[OPENAI_CURATED_REMOTE_MARKETPLACE],
        config_text,
    )?;
    Ok(PluginMarketplaceInspection {
        local,
        remote,
        config_bytes,
    })
}

fn inspect_marketplace(
    kind: PluginMarketplaceKind,
    root: PathBuf,
    marketplace_name: &str,
    config_names: &[&str],
    config_text: &str,
) -> anyhow::Result<PluginMarketplaceRecord> {
    let directory_identity = marketplace_directory_identity(&root)?;
    let Some(plugin_count) = marketplace_plugin_count(&root, marketplace_name)? else {
        return Ok(PluginMarketplaceRecord {
            kind,
            available: false,
            config_registered: false,
            plugin_count: 0,
            skill_count: 0,
            marketplace_root: None,
            directory_identity,
        });
    };
    let config_registered = config_names
        .iter()
        .all(|name| marketplace_config_text_points_to_root(config_text, name, &root));
    let skill_count = count_marketplace_skill_files(&root.join("plugins"))?;
    Ok(PluginMarketplaceRecord {
        kind,
        available: true,
        config_registered,
        plugin_count,
        skill_count,
        marketplace_root: Some(root),
        directory_identity,
    })
}

fn marketplace_directory_identity(root: &Path) -> anyhow::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-plus-plugin-marketplace-directory-v1\0");
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            hasher.update([1]);
            hash_marketplace_directory(root, root, &mut hasher)?;
        }
        Ok(metadata) if metadata.file_type().is_file() => {
            hasher.update([2]);
            hash_marketplace_file(root, &mut hasher)?;
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            hasher.update([3]);
            let target = std::fs::read_link(root)
                .with_context(|| format!("failed to read link {}", root.display()))?;
            hash_identity_bytes(&mut hasher, target.to_string_lossy().as_bytes());
        }
        Ok(_) => hasher.update([4]),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => hasher.update([0]),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", root.display()));
        }
    }
    Ok(hasher.finalize().into())
}

fn hash_marketplace_directory(
    root: &Path,
    directory: &Path,
    hasher: &mut Sha256,
) -> anyhow::Result<()> {
    let mut entries = std::fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read entry in {}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        let marker = if file_type.is_dir() {
            1
        } else if file_type.is_file() {
            2
        } else if file_type.is_symlink() {
            3
        } else {
            4
        };
        hasher.update([marker]);
        hash_identity_bytes(hasher, relative.as_bytes());
        match marker {
            1 => hash_marketplace_directory(root, &path, hasher)?,
            2 => hash_marketplace_file(&path, hasher)?,
            3 => {
                let target = std::fs::read_link(&path)
                    .with_context(|| format!("failed to read link {}", path.display()))?;
                hash_identity_bytes(hasher, target.to_string_lossy().as_bytes());
            }
            _ => {}
        }
    }
    Ok(())
}

fn hash_marketplace_file(path: &Path, hasher: &mut Sha256) -> anyhow::Result<()> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let length = file
        .metadata()
        .with_context(|| format!("failed to inspect {}", path.display()))?
        .len();
    hasher.update(length.to_le_bytes());
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

fn hash_identity_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn marketplace_plugin_count(root: &Path, marketplace_name: &str) -> anyhow::Result<Option<usize>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let Ok(marketplace) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(None);
    };
    if marketplace.get("name").and_then(serde_json::Value::as_str) != Some(marketplace_name)
        || !root.join("plugins").is_dir()
    {
        return Ok(None);
    }
    let Some(plugins) = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .filter(|plugins| !plugins.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(plugins.len()))
}

fn count_marketplace_skill_files(root: &Path) -> anyhow::Result<usize> {
    if !root.is_dir() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in
        std::fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if file_type.is_dir() {
            total += count_marketplace_skill_files(&path)?;
        } else if file_type.is_file()
            && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
        {
            total += 1;
        }
    }
    Ok(total)
}

pub async fn initialize_openai_curated_marketplace_and_configure(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let prepared = prepare_local_plugin_marketplace(home).await?;
    commit_prepared_plugin_marketplace(home, prepared)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketplaceEnsureResult {
    pub initialized: bool,
    pub configured: bool,
}

pub struct PreparedPluginMarketplace {
    kind: PluginMarketplaceKind,
    staging: Option<PathBuf>,
}

impl PreparedPluginMarketplace {
    pub fn kind(&self) -> PluginMarketplaceKind {
        self.kind
    }
}

impl std::fmt::Debug for PreparedPluginMarketplace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPluginMarketplace")
            .field("kind", &self.kind)
            .field("has_staging", &self.staging.is_some())
            .finish()
    }
}

impl Drop for PreparedPluginMarketplace {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take() {
            let _ = std::fs::remove_dir_all(staging);
        }
    }
}

pub async fn prepare_local_plugin_marketplace(
    home: &Path,
) -> anyhow::Result<PreparedPluginMarketplace> {
    let root = home.join(".tmp").join("plugins");
    if marketplace_plugin_count(&root, OPENAI_CURATED_MARKETPLACE)?.is_some() {
        return Ok(PreparedPluginMarketplace {
            kind: PluginMarketplaceKind::Local,
            staging: None,
        });
    }
    let bytes = download_openai_plugins_zip().await?;
    prepare_local_plugin_marketplace_from_bytes(home, &bytes)
}

pub fn prepare_remote_plugin_marketplace(home: &Path) -> anyhow::Result<PreparedPluginMarketplace> {
    prepare_remote_plugin_marketplace_from_bytes(home, OPENAI_CURATED_REMOTE_MARKETPLACE_ZIP)
}

pub fn commit_prepared_plugin_marketplace(
    home: &Path,
    mut prepared: PreparedPluginMarketplace,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let kind = prepared.kind;
    let initialized = commit_prepared_marketplace_directory(home, &mut prepared)?;
    let configured = match kind {
        PluginMarketplaceKind::Local => ensure_openai_curated_marketplace_config(home)?,
        PluginMarketplaceKind::Remote => ensure_openai_curated_remote_marketplace_config(home)?,
    };
    Ok(MarketplaceEnsureResult {
        initialized,
        configured,
    })
}

fn local_openai_curated_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins");
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root))
}

fn local_role_specific_plugins_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home
        .join(".tmp")
        .join("marketplaces")
        .join(ROLE_SPECIFIC_PLUGINS_MARKETPLACE);
    local_marketplace_root_from_root(&root, ROLE_SPECIFIC_PLUGINS_MARKETPLACE)
}

fn local_marketplace_root_from_root(
    root: &Path,
    marketplace_name: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str) != Some(marketplace_name) {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn local_marketplace_plugin_names(
    root: &Path,
    marketplace_name: &str,
) -> anyhow::Result<Vec<String>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str) != Some(marketplace_name) {
        return Ok(Vec::new());
    }
    Ok(marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| {
            plugin
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .collect())
}

fn local_openai_curated_remote_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins-remote");
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_REMOTE_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root))
}

async fn download_openai_plugins_zip() -> anyhow::Result<Vec<u8>> {
    let client =
        crate::http_client::proxied_client(&format!("Codex++/{}", crate::version::VERSION))?;
    let mut response = client
        .get(OPENAI_PLUGINS_ZIP_URL)
        .header(reqwest::header::ACCEPT, "application/zip")
        .send()
        .await
        .context("failed to download openai/plugins marketplace")?
        .error_for_status()
        .context("openai/plugins marketplace download returned an error status")?;
    if let Some(size) = response.content_length()
        && size > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES as u64
    {
        anyhow::bail!("openai/plugins marketplace download is too large: {size} bytes");
    }
    let capacity = response
        .content_length()
        .unwrap_or_default()
        .min(OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES as u64) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read openai/plugins marketplace download body")?
    {
        checked_openai_plugins_download_size(bytes.len(), chunk.len())?;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_openai_plugins_download_size(size: usize) -> anyhow::Result<()> {
    if size > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES {
        anyhow::bail!("openai/plugins marketplace download is too large: {size} bytes");
    }
    Ok(())
}

fn checked_openai_plugins_download_size(current: usize, added: usize) -> anyhow::Result<usize> {
    let size = current
        .checked_add(added)
        .ok_or_else(|| anyhow::anyhow!("openai/plugins marketplace download is too large"))?;
    validate_openai_plugins_download_size(size)?;
    Ok(size)
}

fn prepare_local_plugin_marketplace_from_bytes(
    home: &Path,
    bytes: &[u8],
) -> anyhow::Result<PreparedPluginMarketplace> {
    let root = home.join(".tmp").join("plugins");
    if marketplace_plugin_count(&root, OPENAI_CURATED_MARKETPLACE)?.is_some() {
        return Ok(PreparedPluginMarketplace {
            kind: PluginMarketplaceKind::Local,
            staging: None,
        });
    }
    validate_openai_plugins_download_size(bytes.len())?;
    let staging = create_marketplace_staging(home, "plugins-download")?;
    let result = extract_openai_plugins_zip(bytes, &staging)
        .and_then(|_| validate_openai_plugins_marketplace_root(&staging));
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(PreparedPluginMarketplace {
        kind: PluginMarketplaceKind::Local,
        staging: Some(staging),
    })
}

fn prepare_remote_plugin_marketplace_from_bytes(
    home: &Path,
    bytes: &[u8],
) -> anyhow::Result<PreparedPluginMarketplace> {
    let root = home.join(".tmp").join("plugins-remote");
    if marketplace_plugin_count(&root, OPENAI_CURATED_REMOTE_MARKETPLACE)?.is_some() {
        return Ok(PreparedPluginMarketplace {
            kind: PluginMarketplaceKind::Remote,
            staging: None,
        });
    }
    let staging = create_marketplace_staging(home, "plugins-remote-embedded")?;
    let result = extract_zip_exact(bytes, &staging)
        .and_then(|_| validate_openai_curated_remote_marketplace_root(&staging));
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(PreparedPluginMarketplace {
        kind: PluginMarketplaceKind::Remote,
        staging: Some(staging),
    })
}

fn create_marketplace_staging(home: &Path, prefix: &str) -> anyhow::Result<PathBuf> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let parent = home.join(".tmp");
    std::fs::create_dir_all(&parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..32 {
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let staging = parent.join(format!(
            "{prefix}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        match std::fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create {}", staging.display()));
            }
        }
    }
    anyhow::bail!("failed to allocate plugin marketplace staging directory")
}

fn commit_prepared_marketplace_directory(
    home: &Path,
    prepared: &mut PreparedPluginMarketplace,
) -> anyhow::Result<bool> {
    let Some(staging) = prepared.staging.as_deref() else {
        return Ok(false);
    };
    match prepared.kind {
        PluginMarketplaceKind::Local => {
            validate_openai_plugins_marketplace_root(staging)?;
            replace_directory(staging, &home.join(".tmp").join("plugins"))?;
        }
        PluginMarketplaceKind::Remote => {
            validate_openai_curated_remote_marketplace_root(staging)?;
            replace_directory_with_backup_name(
                staging,
                &home.join(".tmp").join("plugins-remote"),
                "plugins-remote.previous-codex-plus",
            )?;
        }
    }
    prepared.staging = None;
    Ok(true)
}

fn extract_openai_plugins_zip(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to read openai/plugins zip")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;
        let Some(relative_path) = zip_entry_relative_path(file.name())? else {
            continue;
        };
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .with_context(|| format!("failed to read zip entry {}", file.name()))?;
        std::fs::write(&output_path, contents)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
    }
    Ok(())
}

fn extract_zip_exact(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to read embedded plugin zip")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;
        let relative_path = safe_zip_path(file.name())?;
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .with_context(|| format!("failed to read zip entry {}", file.name()))?;
        std::fs::write(&output_path, contents)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
    }
    Ok(())
}

fn safe_zip_path(name: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(name);
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => anyhow::bail!("zip entry escapes destination: {name}"),
        }
    }
    if relative.as_os_str().is_empty() {
        anyhow::bail!("zip entry has empty path");
    }
    Ok(relative)
}

fn zip_entry_relative_path(name: &str) -> anyhow::Result<Option<PathBuf>> {
    let path = Path::new(name);
    let mut components = path.components();
    let Some(first) = components.next() else {
        anyhow::bail!("zip entry has empty path");
    };
    match first {
        Component::Normal(_) => {}
        _ => anyhow::bail!("zip entry escapes destination: {name}"),
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => anyhow::bail!("zip entry escapes destination: {name}"),
        }
    }
    Ok((!relative.as_os_str().is_empty()).then_some(relative))
}

fn validate_openai_plugins_marketplace_root(root: &Path) -> anyhow::Result<()> {
    let marketplace = local_openai_curated_marketplace_root_from_root(root)?
        .ok_or_else(|| anyhow::anyhow!("downloaded openai/plugins marketplace is invalid"))?;
    if marketplace != root {
        anyhow::bail!("downloaded openai/plugins marketplace root mismatch");
    }
    Ok(())
}

fn validate_openai_curated_remote_marketplace_root(root: &Path) -> anyhow::Result<()> {
    let marketplace = local_openai_curated_remote_marketplace_root_from_root(root)?
        .ok_or_else(|| anyhow::anyhow!("embedded official remote plugin marketplace is invalid"))?;
    if marketplace != root {
        anyhow::bail!("embedded official remote plugin marketplace root mismatch");
    }
    Ok(())
}

fn local_openai_curated_marketplace_root_from_root(root: &Path) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn local_openai_curated_remote_marketplace_root_from_root(
    root: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_REMOTE_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn replace_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    replace_directory_with_backup_name(source, destination, "plugins.previous-codex-plus")
}

fn replace_directory_with_backup_name(
    source: &Path,
    destination: &Path,
    backup_name: &str,
) -> anyhow::Result<()> {
    let backup = destination.with_file_name(backup_name);
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove {}", backup.display()))?;
    }
    if destination.exists() {
        std::fs::rename(destination, &backup).with_context(|| {
            format!(
                "failed to move {} to {}",
                destination.display(),
                backup.display()
            )
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => {
            if backup.exists() {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, destination);
            }
            Err(error).with_context(|| {
                format!(
                    "failed to move {} to {}",
                    source.display(),
                    destination.display()
                )
            })
        }
    }
}

fn ensure_marketplace_configs(
    home: &Path,
    marketplace_names: &[&str],
    marketplace_root: &Path,
) -> anyhow::Result<bool> {
    ensure_marketplace_configs_with_plugins(home, marketplace_names, marketplace_root, &[])
}

fn ensure_marketplace_configs_with_plugins(
    home: &Path,
    marketplace_names: &[&str],
    marketplace_root: &Path,
    plugin_ids: &[String],
) -> anyhow::Result<bool> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read(&config_path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let without_bom = existing.trim_start_matches('\u{feff}');
    let updated = merge_marketplace_configs_and_plugins_into_text(
        without_bom,
        marketplace_names,
        marketplace_root,
        plugin_ids,
    )?;
    if updated.as_bytes() == without_bom.as_bytes() {
        return Ok(false);
    }
    crate::settings::atomic_write(&config_path, updated.as_bytes())?;
    Ok(true)
}

fn merge_marketplace_configs_into_text(
    config_text: &str,
    marketplace_names: &[&str],
    marketplace_root: &Path,
) -> anyhow::Result<String> {
    merge_marketplace_configs_and_plugins_into_text(
        config_text,
        marketplace_names,
        marketplace_root,
        &[],
    )
}

fn merge_marketplace_configs_and_plugins_into_text(
    config_text: &str,
    marketplace_names: &[&str],
    marketplace_root: &Path,
    plugin_ids: &[String],
) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    let marketplaces = table_mut_or_insert(&mut doc, "marketplaces")?;
    for marketplace_name in marketplace_names {
        if marketplaces
            .get(marketplace_name)
            .and_then(Item::as_table)
            .is_none()
        {
            marketplaces[marketplace_name] = toml_edit::table();
        }
        marketplaces[marketplace_name]["source_type"] = toml_edit::value("local");
        marketplaces[marketplace_name]["source"] =
            toml_edit::value(windows_extended_path(marketplace_root));
    }
    if !plugin_ids.is_empty() {
        let plugins = table_mut_or_insert(&mut doc, "plugins")?;
        for plugin_id in plugin_ids {
            let existing_enabled = plugins
                .get(plugin_id)
                .and_then(Item::as_table)
                .and_then(|table| table.get("enabled"))
                .and_then(Item::as_bool);
            if plugins.get(plugin_id).and_then(Item::as_table).is_none() {
                plugins[plugin_id] = toml_edit::table();
            }
            if existing_enabled.is_none() {
                plugins[plugin_id]["enabled"] = toml_edit::value(true);
            }
        }
    }
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn marketplace_config_points_to_root(home: &Path, marketplace_name: &str, root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(home.join("config.toml")) else {
        return false;
    };
    marketplace_config_text_points_to_root(&text, marketplace_name, root)
}

fn marketplace_config_text_points_to_root(
    config_text: &str,
    marketplace_name: &str,
    root: &Path,
) -> bool {
    let Ok(doc) = config_text
        .trim_start_matches('\u{feff}')
        .parse::<DocumentMut>()
    else {
        return false;
    };
    let Some(table) = doc
        .get("marketplaces")
        .and_then(Item::as_table)
        .and_then(|marketplaces| marketplaces.get(marketplace_name))
        .and_then(Item::as_table)
    else {
        return false;
    };
    let source_type = table
        .get("source_type")
        .and_then(Item::as_str)
        .unwrap_or_default();
    let source = table
        .get("source")
        .and_then(Item::as_str)
        .unwrap_or_default();
    source_type == "local" && normalize_windows_extended_path(source) == root.to_string_lossy()
}

fn normalize_windows_extended_path(value: &str) -> String {
    value.strip_prefix(r"\\?\").unwrap_or(value).to_string()
}

fn windows_extended_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value.starts_with(r"\\?\") {
        value.into_owned()
    } else {
        format!(r"\\?\{value}")
    }
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    let contents = contents.trim_start_matches('\u{feff}');
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .map_err(|error| anyhow::anyhow!("config.toml TOML parse failed: {error}"))
    }
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} must be a TOML table"))
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_marketplace(home: &Path) {
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins").join("gmail")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
        )
        .unwrap();
    }

    fn write_remote_marketplace(home: &Path) {
        let root = home.join(".tmp").join("plugins-remote");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins").join("product-design")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design","path":"./plugins/product-design"}]}"#,
        )
        .unwrap();
    }

    fn write_role_specific_marketplace(home: &Path) {
        let root = home
            .join(".tmp")
            .join("marketplaces")
            .join("role-specific-plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        for plugin in [
            "sales",
            "data-analytics",
            "product-design",
            "financial-markets",
            "customer-support",
        ] {
            std::fs::create_dir_all(root.join("plugins").join(plugin)).unwrap();
        }
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"role-specific-plugins","plugins":[{"name":"sales"},{"name":"data-analytics"},{"name":"product-design"},{"name":"financial-markets"},{"name":"customer-support"}]}"#,
        )
        .unwrap();
    }

    fn local_marketplace_zip() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("plugins-main/.agents/plugins/marketplace.json", options)
                .unwrap();
            std::io::Write::write_all(
                &mut writer,
                br#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
            )
            .unwrap();
            writer
                .start_file(
                    "plugins-main/plugins/gmail/.codex-plugin/plugin.json",
                    options,
                )
                .unwrap();
            std::io::Write::write_all(&mut writer, br#"{"name":"gmail"}"#).unwrap();
            writer.finish().unwrap();
        }
        bytes.into_inner()
    }

    #[test]
    fn ensure_openai_curated_marketplace_config_registers_local_marketplace() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_marketplace(home);
        write_remote_marketplace(home);

        let changed = ensure_openai_curated_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-curated"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated"]["source"].as_str(),
            Some(format!(r"\\?\{}", home.join(".tmp").join("plugins").display()).as_str())
        );
        assert_eq!(
            parsed["marketplaces"]["openai-api-curated"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-api-curated"]["source"].as_str(),
            Some(format!(r"\\?\{}", home.join(".tmp").join("plugins").display()).as_str())
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source"].as_str(),
            Some(
                format!(
                    r"\\?\{}",
                    home.join(".tmp").join("plugins-remote").display()
                )
                .as_str()
            )
        );
    }

    #[test]
    fn ensure_openai_curated_marketplace_config_skips_when_snapshot_missing() {
        let temp = tempfile::tempdir().unwrap();

        let changed = ensure_openai_curated_marketplace_config(temp.path()).unwrap();

        assert!(!changed);
        assert!(!temp.path().join("config.toml").exists());
    }

    #[test]
    fn ensure_role_specific_plugins_marketplace_config_repairs_installed_plugin_entries() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_role_specific_marketplace(home);
        std::fs::write(
            home.join("config.toml"),
            "model_provider = \"custom\"\nexperimental_bearer_token = \"sk-redacted\"\n",
        )
        .unwrap();

        let changed = ensure_role_specific_plugins_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["role-specific-plugins"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["role-specific-plugins"]["source"].as_str(),
            Some(
                format!(
                    r"\\?\{}",
                    home.join(".tmp")
                        .join("marketplaces")
                        .join("role-specific-plugins")
                        .display()
                )
                .as_str()
            )
        );
        for plugin in [
            "sales@role-specific-plugins",
            "data-analytics@role-specific-plugins",
            "product-design@role-specific-plugins",
            "financial-markets@role-specific-plugins",
            "customer-support@role-specific-plugins",
        ] {
            assert_eq!(parsed["plugins"][plugin]["enabled"].as_bool(), Some(true));
        }
        assert_eq!(
            parsed["experimental_bearer_token"].as_str(),
            Some("sk-redacted")
        );
    }

    #[test]
    fn ensure_role_specific_plugins_marketplace_config_preserves_disabled_plugin_choice() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_role_specific_marketplace(home);
        std::fs::write(
            home.join("config.toml"),
            "[plugins.\"sales@role-specific-plugins\"]\nenabled = false\n",
        )
        .unwrap();

        let changed = ensure_role_specific_plugins_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["plugins"]["sales@role-specific-plugins"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            parsed["plugins"]["customer-support@role-specific-plugins"]["enabled"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn openai_curated_marketplace_status_detects_missing_config() {
        let temp = tempfile::tempdir().unwrap();
        write_marketplace(temp.path());

        let status = openai_curated_marketplace_status(temp.path());

        assert!(status.marketplace_root.is_some());
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn openai_curated_marketplace_status_requires_api_marketplace_config() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        write_marketplace(home);
        write_remote_marketplace(home);
        ensure_marketplace_configs(home, &[OPENAI_CURATED_MARKETPLACE], &root).unwrap();

        let status = openai_curated_marketplace_status(home);

        assert!(status.marketplace_root.is_some());
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn openai_curated_remote_marketplace_status_detects_cached_marketplace() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_remote_marketplace(home);

        let status = openai_curated_remote_marketplace_status(home);

        assert_eq!(
            status.marketplace_root,
            Some(home.join(".tmp").join("plugins-remote"))
        );
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn inspect_plugin_marketplaces_reports_both_marketplaces_and_recursive_counts() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_marketplace(home);
        write_remote_marketplace(home);
        let local_root = home.join(".tmp").join("plugins");
        let remote_root = home.join(".tmp").join("plugins-remote");
        std::fs::create_dir_all(local_root.join("plugins").join("calendar")).unwrap();
        std::fs::write(
            local_root
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"},{"name":"calendar","path":"./plugins/calendar"}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(local_root.join("plugins/gmail/nested")).unwrap();
        std::fs::write(local_root.join("plugins/gmail/SKILL.md"), "gmail").unwrap();
        std::fs::write(local_root.join("plugins/gmail/nested/SKILL.md"), "nested").unwrap();
        std::fs::write(
            remote_root.join("plugins/product-design/SKILL.md"),
            "product design",
        )
        .unwrap();
        ensure_openai_curated_marketplace_config(home).unwrap();

        let inspection = inspect_plugin_marketplaces(home).unwrap();

        assert_eq!(inspection.local.kind, PluginMarketplaceKind::Local);
        assert!(inspection.local.available);
        assert!(inspection.local.config_registered);
        assert_eq!(inspection.local.plugin_count, 2);
        assert_eq!(inspection.local.skill_count, 2);
        assert_eq!(inspection.remote.kind, PluginMarketplaceKind::Remote);
        assert!(inspection.remote.available);
        assert!(inspection.remote.config_registered);
        assert_eq!(inspection.remote.plugin_count, 1);
        assert_eq!(inspection.remote.skill_count, 1);
    }

    #[test]
    fn inspect_plugin_marketplaces_marks_missing_marketplaces_for_repair() {
        let temp = tempfile::tempdir().unwrap();

        let inspection = inspect_plugin_marketplaces(temp.path()).unwrap();

        assert!(!inspection.local.available);
        assert!(inspection.local.needs_repair());
        assert_eq!(inspection.local.plugin_count, 0);
        assert_eq!(inspection.local.skill_count, 0);
        assert!(!inspection.remote.available);
        assert!(inspection.remote.needs_repair());
        assert_eq!(inspection.remote.plugin_count, 0);
        assert_eq!(inspection.remote.skill_count, 0);
    }

    #[test]
    fn inspect_plugin_marketplaces_marks_corrupt_marketplace_for_repair() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            "{not-json",
        )
        .unwrap();

        let inspection = inspect_plugin_marketplaces(home).unwrap();

        assert!(!inspection.local.available);
        assert!(inspection.local.needs_repair());
        assert_eq!(inspection.local.plugin_count, 0);
        assert_eq!(inspection.local.skill_count, 0);
    }

    #[test]
    fn local_marketplace_preparation_does_not_change_live_directory_or_config() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::write(home.join("config.toml"), "unknown_key = true\n").unwrap();

        let prepared =
            prepare_local_plugin_marketplace_from_bytes(home, &local_marketplace_zip()).unwrap();

        assert!(!home.join(".tmp").join("plugins").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("config.toml")).unwrap(),
            "unknown_key = true\n"
        );
        assert!(prepared.staging.as_deref().is_some_and(Path::is_dir));
    }

    #[test]
    fn dropping_marketplace_preparation_removes_private_staging() {
        let temp = tempfile::tempdir().unwrap();
        let prepared = prepare_remote_plugin_marketplace(temp.path()).unwrap();
        let staging = prepared.staging.clone().unwrap();

        drop(prepared);

        assert!(!staging.exists());
    }

    #[test]
    fn commit_prepared_marketplace_preserves_unknown_toml_and_registers_remote() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::write(home.join("config.toml"), "unknown_key = true\n").unwrap();
        let prepared = prepare_remote_plugin_marketplace(home).unwrap();

        let result = commit_prepared_plugin_marketplace(home, prepared).unwrap();

        assert!(result.initialized);
        assert!(result.configured);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(config.contains("unknown_key = true"));
        assert!(config.contains("openai-curated-remote"));
        let inspection = inspect_plugin_marketplaces(home).unwrap();
        assert!(!inspection.remote.needs_repair());
    }

    #[test]
    fn valid_existing_marketplace_prepares_registration_without_staging() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_marketplace(home);

        let prepared = prepare_local_plugin_marketplace_from_bytes(home, b"not-a-zip").unwrap();

        assert!(prepared.staging.is_none());
        let result = commit_prepared_plugin_marketplace(home, prepared).unwrap();
        assert!(!result.initialized);
        assert!(result.configured);
        assert!(
            !inspect_plugin_marketplaces(home)
                .unwrap()
                .local
                .needs_repair()
        );
    }

    #[test]
    fn invalid_marketplace_preparation_cleans_staging_directory() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        let error = prepare_local_plugin_marketplace_from_bytes(home, b"not-a-zip")
            .expect_err("invalid archive should fail");

        assert!(error.to_string().contains("openai/plugins zip"));
        let staging_entries = std::fs::read_dir(home.join(".tmp"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("plugins-download-")
            })
            .count();
        assert_eq!(staging_entries, 0);
    }

    #[test]
    fn download_limit_accepts_the_boundary_and_rejects_overflow_without_allocating_it() {
        assert_eq!(
            checked_openai_plugins_download_size(OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES - 1, 1)
                .unwrap(),
            OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES
        );
        assert!(
            checked_openai_plugins_download_size(OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES, 1).is_err()
        );
        assert!(checked_openai_plugins_download_size(usize::MAX, 1).is_err());
    }

    #[test]
    fn local_marketplace_preparation_rejects_archive_escape_entries() {
        let temp = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("plugins-main/.agents/plugins/marketplace.json", options)
                .unwrap();
            std::io::Write::write_all(
                &mut writer,
                br#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
            )
            .unwrap();
            writer
                .start_file("plugins-main/plugins/gmail/SKILL.md", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, b"safe").unwrap();
            writer
                .start_file("plugins-main/../outside.txt", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, b"escape").unwrap();
            writer.finish().unwrap();
        }

        let error = prepare_local_plugin_marketplace_from_bytes(temp.path(), bytes.get_ref())
            .expect_err("archive escape should fail");

        assert!(error.to_string().contains("escapes destination"));
        assert!(!temp.path().join("outside.txt").exists());
    }

    #[test]
    fn ensure_openai_curated_remote_marketplace_config_registers_remote_only() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_remote_marketplace(home);

        let changed = ensure_openai_curated_remote_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert!(
            parsed
                .get("marketplaces")
                .and_then(Item::as_table)
                .and_then(|marketplaces| marketplaces.get("openai-curated"))
                .is_none()
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source"].as_str(),
            Some(
                format!(
                    r"\\?\{}",
                    home.join(".tmp").join("plugins-remote").display()
                )
                .as_str()
            )
        );
    }

    #[test]
    fn ensure_openai_curated_remote_marketplace_available_installs_embedded_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        let result = ensure_openai_curated_remote_marketplace_available(home).unwrap();

        assert!(result.initialized);
        assert!(result.configured);
        let root = home.join(".tmp").join("plugins-remote");
        assert!(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json")
                .is_file()
        );
        assert!(
            root.join("plugins")
                .join("product-design")
                .join(".codex-plugin")
                .join("plugin.json")
                .is_file()
        );
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source"].as_str(),
            Some(
                format!(
                    r"\\?\{}",
                    home.join(".tmp").join("plugins-remote").display()
                )
                .as_str()
            )
        );
    }

    #[test]
    fn zip_entry_relative_path_strips_archive_root_and_rejects_escape() {
        assert_eq!(
            zip_entry_relative_path("plugins-main/plugins/gmail/file.txt").unwrap(),
            Some(PathBuf::from("plugins").join("gmail").join("file.txt"))
        );
        assert!(zip_entry_relative_path("plugins-main/../evil.txt").is_err());
        assert!(zip_entry_relative_path("../evil.txt").is_err());
    }

    #[test]
    fn install_openai_plugins_zip_installs_valid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("plugins-main/.agents/plugins/marketplace.json", options)
                .unwrap();
            std::io::Write::write_all(
                &mut writer,
                br#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
            )
            .unwrap();
            writer
                .start_file(
                    "plugins-main/plugins/gmail/.codex-plugin/plugin.json",
                    options,
                )
                .unwrap();
            std::io::Write::write_all(&mut writer, br#"{"name":"gmail"}"#).unwrap();
            writer.finish().unwrap();
        }

        let mut prepared =
            prepare_local_plugin_marketplace_from_bytes(temp.path(), bytes.get_ref()).unwrap();
        commit_prepared_marketplace_directory(temp.path(), &mut prepared).unwrap();
        let changed = ensure_openai_curated_marketplace_config(temp.path()).unwrap();

        assert!(changed);
        assert!(
            temp.path()
                .join(".tmp/plugins/.agents/plugins/marketplace.json")
                .is_file()
        );
        assert!(
            temp.path()
                .join(".tmp/plugins/plugins/gmail/.codex-plugin/plugin.json")
                .is_file()
        );
    }
}
