use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_REPOSITORY: &str = "BigPizzaV3/CodexPlusPlus";
pub const DEFAULT_LATEST_JSON_URL: &str =
    "https://github.com/BigPizzaV3/CodexPlusPlus/releases/latest/download/latest.json";
pub const MAX_RELEASE_METADATA_BYTES: usize = 1024 * 1024;
pub const MAX_RELEASE_SUMMARY_BYTES: usize = 16 * 1024;
pub const MAX_UPDATE_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTarget {
    WindowsX64,
    MacosX64,
    MacosArm64,
    Unsupported,
}

pub fn current_update_target() -> UpdateTarget {
    #[cfg(windows)]
    {
        UpdateTarget::WindowsX64
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        UpdateTarget::MacosX64
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        UpdateTarget::MacosArm64
    }
    #[cfg(not(any(
        windows,
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    )))]
    {
        UpdateTarget::Unsupported
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub body: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_summary: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInstall {
    pub release: Release,
    pub installer_path: PathBuf,
    pub launched: bool,
}

pub fn parse_version_tag(value: &str) -> anyhow::Result<Vec<u64>> {
    let normalized = value.trim().trim_start_matches(['v', 'V']);
    let mut digits = String::new();
    for ch in normalized.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            digits.push(ch);
        } else {
            break;
        }
    }
    if digits.is_empty() {
        anyhow::bail!("Invalid version tag: {value}");
    }
    digits
        .split('.')
        .map(|part| part.parse::<u64>().map_err(Into::into))
        .collect()
}

pub fn is_newer_version(candidate: &str, current: &str) -> anyhow::Result<bool> {
    let mut left = parse_version_tag(candidate)?;
    let mut right = parse_version_tag(current)?;
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    Ok(left > right)
}

pub fn release_from_github_payload(payload: &Value) -> anyhow::Result<Release> {
    let version = payload
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("release payload missing tag_name"))?
        .to_string();
    let assets = payload
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            Some((
                asset.get("name")?.as_str()?.to_string(),
                asset.get("browser_download_url")?.as_str()?.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let selected = select_update_asset(&assets);
    Ok(Release {
        version,
        url: payload
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body: normalize_release_summary(
            payload
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        asset_name: selected.as_ref().map(|asset| asset.name.clone()),
        asset_url: selected.map(|asset| asset.browser_download_url),
    })
}

pub fn release_from_latest_json_payload(payload: &Value) -> anyhow::Result<Release> {
    release_from_latest_json_payload_for(payload, current_update_target())
}

pub fn release_from_latest_json_bytes_for(
    payload: &[u8],
    target: UpdateTarget,
) -> anyhow::Result<Release> {
    if payload.len() > MAX_RELEASE_METADATA_BYTES {
        anyhow::bail!("release metadata exceeds {MAX_RELEASE_METADATA_BYTES} bytes");
    }
    let payload = serde_json::from_slice::<Value>(payload)?;
    release_from_latest_json_payload_for(&payload, target)
}

fn release_from_latest_json_payload_for(
    payload: &Value,
    target: UpdateTarget,
) -> anyhow::Result<Release> {
    let version = payload
        .get("version")
        .or_else(|| payload.get("tag_name"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("latest.json missing version"))?
        .to_string();
    let assets = payload
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?.to_string();
            let url = asset
                .get("url")
                .or_else(|| asset.get("browser_download_url"))?
                .as_str()?
                .to_string();
            Some((name, url))
        })
        .collect::<Vec<_>>();
    let selected = select_update_asset_for(&assets, target);
    Ok(Release {
        version,
        url: payload
            .get("url")
            .or_else(|| payload.get("html_url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body: normalize_release_summary(
            payload
                .get("body")
                .or_else(|| payload.get("release_summary"))
                .or_else(|| payload.get("notes"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        asset_name: selected.as_ref().map(|asset| asset.name.clone()),
        asset_url: selected.map(|asset| asset.browser_download_url),
    })
}

pub fn select_update_asset(assets: &[(String, String)]) -> Option<ReleaseAsset> {
    select_update_asset_for(assets, current_update_target())
}

pub fn select_update_asset_for(
    assets: &[(String, String)],
    target: UpdateTarget,
) -> Option<ReleaseAsset> {
    let named = assets
        .iter()
        .filter(|(name, url)| !name.trim().is_empty() && !url.trim().is_empty());
    let mut best: Option<(u8, &str, &str)> = None;
    for (name, url) in named {
        let rank = platform_asset_rank(&name.to_ascii_lowercase(), target);
        if rank >= 2 {
            continue;
        }
        if best.is_none_or(|(r, _, _)| rank < r) {
            best = Some((rank, name.as_str(), url.as_str()));
        }
    }
    best.map(|(_, name, url)| ReleaseAsset {
        name: name.to_string(),
        browser_download_url: url.to_string(),
    })
}

pub fn normalize_release_summary(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(MAX_RELEASE_SUMMARY_BYTES));
    let mut characters = value.chars().peekable();
    while let Some(mut character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            character = '\n';
        } else if character.is_control() && character != '\n' && character != '\t' {
            character = ' ';
        }
        if output.len() + character.len_utf8() > MAX_RELEASE_SUMMARY_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

pub fn validate_update_asset_for(asset: &ReleaseAsset, target: UpdateTarget) -> anyhow::Result<()> {
    safe_asset_name(&asset.name)?;
    validate_update_response_url(&asset.browser_download_url)?;
    if platform_asset_rank(&asset.name.to_ascii_lowercase(), target) >= 2 {
        anyhow::bail!("release asset does not match the update target");
    }
    Ok(())
}

pub fn validate_update_response_url(value: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(value).map_err(|_| anyhow::anyhow!("invalid update URL"))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        anyhow::bail!("update URL must use HTTPS");
    }
    Ok(())
}

pub async fn fetch_latest_release(latest_json_url: &str) -> anyhow::Result<Release> {
    let client =
        crate::http_client::proxied_client(&format!("Codex++/{}", crate::version::VERSION))?;
    let payload = client
        .get(latest_json_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    release_from_latest_json_payload(&payload)
}

pub async fn check_for_update(current_version: &str) -> anyhow::Result<UpdateCheck> {
    let release = fetch_latest_release(DEFAULT_LATEST_JSON_URL).await?;
    let update_available = is_newer_version(&release.version, current_version)?;
    Ok(UpdateCheck {
        current_version: current_version.to_string(),
        latest_version: Some(release.version),
        release_summary: release.body,
        asset_name: release.asset_name,
        asset_url: release.asset_url,
        update_available,
    })
}

pub async fn perform_update(
    release: &Release,
    download_dir: &Path,
) -> anyhow::Result<UpdateInstall> {
    let url = release
        .asset_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("没有可下载的 Release asset"))?;
    let bytes =
        crate::http_client::proxied_client(&format!("Codex++/{}", crate::version::VERSION))?
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
    let installer_path = download_asset_to(release, &bytes, download_dir)?;
    launch_installer(&installer_path)?;
    Ok(UpdateInstall {
        release: release.clone(),
        installer_path,
        launched: true,
    })
}

pub fn download_asset_to(
    release: &Release,
    bytes: &[u8],
    download_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let name = release
        .asset_name
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("没有可下载的 Release asset"))?;
    let safe = safe_asset_name(name)?;
    std::fs::create_dir_all(download_dir)?;
    let path = download_dir.join(safe);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

pub fn safe_asset_name(name: &str) -> anyhow::Result<String> {
    if name.is_empty()
        || name.len() > 255
        || name.trim() != name
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        anyhow::bail!("invalid release asset filename");
    }
    let path = Path::new(name);
    if path.components().count() != 1 {
        anyhow::bail!("invalid release asset filename");
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid release asset filename"))?;
    if file_name == "." || file_name == ".." {
        anyhow::bail!("invalid release asset filename");
    }
    let device = file_name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || is_numbered_windows_device(&device, "COM")
        || is_numbered_windows_device(&device, "LPT")
    {
        anyhow::bail!("invalid release asset filename");
    }
    Ok(file_name.to_string())
}

fn is_numbered_windows_device(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

fn platform_asset_rank(name: &str, target: UpdateTarget) -> u8 {
    // 0 = exact match (current OS + native arch)
    // 1 = same OS, other arch (acceptable fallback, e.g. x86_64 on arm64 or vice versa)
    // 2 = wrong platform
    if matches!(target, UpdateTarget::MacosX64 | UpdateTarget::MacosArm64) {
        if !is_macos_installer_asset(name) {
            return 2;
        }
        if is_macos_native_arch_asset(name, target) {
            return 0;
        }
        return 1;
    }
    if target == UpdateTarget::WindowsX64 && is_windows_installer_asset(name) {
        return 0;
    }
    2
}

fn is_macos_native_arch_asset(name: &str, target: UpdateTarget) -> bool {
    let lower = name.to_ascii_lowercase();
    let native_arch_token = match target {
        UpdateTarget::MacosX64 => "x64",
        UpdateTarget::MacosArm64 => "arm64",
        _ => return false,
    };
    // Modern filename shape: `...-macos-x64.dmg` or `...-macos-arm64.dmg`
    if lower.contains(&format!("-{native_arch_token}.")) {
        return true;
    }
    // Old filename shape: `CodexPlusPlus_1.0.9_x64.dmg`
    if lower.contains(&format!("_{native_arch_token}.")) {
        return true;
    }
    // Newer but alternative shape: `..._x64.dmg` (no `macos-` token)
    let other_token = if native_arch_token == "x64" {
        "arm64"
    } else {
        "x64"
    };
    if lower.contains(&format!("_{other_token}.")) || lower.contains(&format!("-{other_token}.")) {
        return false;
    }
    // No arch token at all — assume it matches the current arch.
    true
}

fn is_windows_installer_asset(name: &str) -> bool {
    name.contains("codex")
        && name.contains("plus")
        && (name.ends_with(".msi")
            || name.ends_with("-setup.exe")
            || name.ends_with("_setup.exe")
            || name.ends_with("setup.exe")
            || name.ends_with("installer.exe"))
}

fn is_macos_installer_asset(name: &str) -> bool {
    // Loose shape check; arch preference is handled by platform_asset_rank
    // via is_macos_native_arch_asset.
    name.contains("codex") && name.contains("plus") && name.ends_with(".dmg")
}

pub fn launch_installer(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(path)
            .creation_flags(crate::windows_integration::CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("启动安装包失败：{error}"))
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("打开 DMG 失败：{error}"))
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let _ = path;
        anyhow::bail!("当前平台不支持启动安装包")
    }
}
