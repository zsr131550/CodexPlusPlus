use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use codex_plus_core::update::{
    MAX_RELEASE_METADATA_BYTES, MAX_UPDATE_DOWNLOAD_BYTES, ReleaseAsset, UpdateTarget,
    is_newer_version, release_from_latest_json_bytes_for, safe_asset_name,
    validate_update_asset_for, validate_update_response_url,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLimits {
    metadata_bytes: usize,
    download_bytes: u64,
}

impl UpdateLimits {
    pub fn new(metadata_bytes: usize, download_bytes: u64) -> Self {
        Self {
            metadata_bytes: metadata_bytes.clamp(1, MAX_RELEASE_METADATA_BYTES),
            download_bytes: download_bytes.clamp(1, MAX_UPDATE_DOWNLOAD_BYTES),
        }
    }
}

impl Default for UpdateLimits {
    fn default() -> Self {
        Self::new(MAX_RELEASE_METADATA_BYTES, MAX_UPDATE_DOWNLOAD_BYTES)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UpdateRevision(Uuid);

impl fmt::Debug for UpdateRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UpdateRevision([opaque])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCandidate {
    pub revision: UpdateRevision,
    pub version: String,
    pub asset_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateAvailability {
    Current,
    Available(UpdateCandidate),
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub installed_version: String,
    pub latest_version: String,
    pub summary: String,
    pub availability: UpdateAvailability,
}

impl fmt::Debug for UpdateCheckResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateCheckResult")
            .field("installed_version", &self.installed_version)
            .field("latest_version", &self.latest_version)
            .field("summary_bytes", &self.summary.len())
            .field("availability", &self.availability)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InstallUpdate {
    pub revision: UpdateRevision,
    pub confirmed_version: String,
}

impl fmt::Debug for InstallUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstallUpdate")
            .field("revision", &self.revision)
            .field("confirmed_version", &self.confirmed_version)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallStarted {
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

pub type UpdateProgressSink = Arc<dyn Fn(UpdateProgress) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateEnvironmentErrorKind {
    Transport,
    Filesystem,
    Launcher,
}

#[derive(Clone)]
pub struct UpdateEnvironmentError {
    kind: UpdateEnvironmentErrorKind,
    _detail: String,
}

impl UpdateEnvironmentError {
    pub fn new(kind: UpdateEnvironmentErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            _detail: bounded_detail(detail.into()),
        }
    }

    pub fn kind(&self) -> UpdateEnvironmentErrorKind {
        self.kind
    }
}

impl fmt::Debug for UpdateEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateEnvironmentError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for UpdateEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            UpdateEnvironmentErrorKind::Transport => "update transport failed",
            UpdateEnvironmentErrorKind::Filesystem => "update filesystem operation failed",
            UpdateEnvironmentErrorKind::Launcher => "update installer launch failed",
        })
    }
}

impl std::error::Error for UpdateEnvironmentError {}

pub struct UpdateDownload {
    final_url: String,
    content_length: Option<u64>,
    chunks: Box<dyn Iterator<Item = Result<Vec<u8>, UpdateEnvironmentError>> + Send>,
}

impl UpdateDownload {
    pub fn new<I>(final_url: String, content_length: Option<u64>, chunks: I) -> Self
    where
        I: Iterator<Item = Result<Vec<u8>, UpdateEnvironmentError>> + Send + 'static,
    {
        Self {
            final_url,
            content_length,
            chunks: Box::new(chunks),
        }
    }
}

impl fmt::Debug for UpdateDownload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateDownload")
            .field("has_final_url", &!self.final_url.is_empty())
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

pub trait UpdateEnvironment: Send + Sync + 'static {
    type Artifact: Write + Send + 'static;

    fn current_version(&self) -> String;
    fn target(&self) -> UpdateTarget;
    fn fetch_release_metadata(
        &self,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError>;
    fn open_asset_download(&self, url: &str) -> Result<UpdateDownload, UpdateEnvironmentError>;
    fn create_update_artifact(
        &self,
        safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError>;
    fn publish_update_artifact(
        &self,
        artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError>;
    fn cleanup_update_artifact(&self, artifact: &mut Self::Artifact);
    fn launch_update_artifact(
        &self,
        artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateErrorKind {
    MetadataFetchFailed,
    MetadataTooLarge,
    MetadataInvalid,
    InvalidAsset,
    InsecureAsset,
    InvalidRevision,
    ConfirmationRequired,
    DownloadFailed,
    DownloadTooLarge,
    WriteFailed,
    PublishFailed,
    LaunchFailed,
    WorkerStopped,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UpdateError {
    kind: UpdateErrorKind,
}

impl UpdateError {
    pub fn new(kind: UpdateErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> UpdateErrorKind {
        self.kind
    }

    fn detail(&self) -> &'static str {
        match self.kind {
            UpdateErrorKind::MetadataFetchFailed => "update metadata fetch failed",
            UpdateErrorKind::MetadataTooLarge => "update metadata is too large",
            UpdateErrorKind::MetadataInvalid => "update metadata is invalid",
            UpdateErrorKind::InvalidAsset => "update asset is invalid",
            UpdateErrorKind::InsecureAsset => "update asset URL is insecure",
            UpdateErrorKind::InvalidRevision => "update candidate is stale or consumed",
            UpdateErrorKind::ConfirmationRequired => "current update version confirmation required",
            UpdateErrorKind::DownloadFailed => "update download failed",
            UpdateErrorKind::DownloadTooLarge => "update download is too large",
            UpdateErrorKind::WriteFailed => "update write failed",
            UpdateErrorKind::PublishFailed => "update publish failed",
            UpdateErrorKind::LaunchFailed => "update installer launch failed",
            UpdateErrorKind::WorkerStopped => "update worker stopped",
        }
    }
}

impl fmt::Debug for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for UpdateError {}

#[derive(Clone)]
struct CandidateRecord {
    candidate: UpdateCandidate,
    asset_url: String,
    installed_version: String,
    target: UpdateTarget,
}

pub struct UpdateService<E> {
    environment: E,
    limits: UpdateLimits,
    candidate: Mutex<Option<CandidateRecord>>,
}

impl<E> UpdateService<E> {
    pub fn new(environment: E) -> Self {
        Self::with_limits(environment, UpdateLimits::default())
    }

    pub fn with_limits(environment: E, limits: UpdateLimits) -> Self {
        Self {
            environment,
            limits,
            candidate: Mutex::new(None),
        }
    }

    fn lock_candidate(&self) -> MutexGuard<'_, Option<CandidateRecord>> {
        self.candidate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl<E: UpdateEnvironment> UpdateService<E> {
    pub fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        *self.lock_candidate() = None;
        let installed_version = safe_version(&self.environment.current_version())?;
        let metadata = self
            .environment
            .fetch_release_metadata(self.limits.metadata_bytes)
            .map_err(|_| UpdateError::new(UpdateErrorKind::MetadataFetchFailed))?;
        if metadata.len() > self.limits.metadata_bytes {
            return Err(UpdateError::new(UpdateErrorKind::MetadataTooLarge));
        }
        let target = self.environment.target();
        let release = release_from_latest_json_bytes_for(&metadata, target)
            .map_err(|_| UpdateError::new(UpdateErrorKind::MetadataInvalid))?;
        let latest_version = safe_version(&release.version)?;
        let update_available = is_newer_version(&latest_version, &installed_version)
            .map_err(|_| UpdateError::new(UpdateErrorKind::MetadataInvalid))?;

        let availability = if !update_available {
            *self.lock_candidate() = None;
            UpdateAvailability::Current
        } else if let (Some(asset_name), Some(asset_url)) = (release.asset_name, release.asset_url)
        {
            validate_update_response_url(&asset_url)
                .map_err(|_| UpdateError::new(UpdateErrorKind::InsecureAsset))?;
            let asset = ReleaseAsset {
                name: asset_name,
                browser_download_url: asset_url.clone(),
            };
            validate_update_asset_for(&asset, target)
                .map_err(|_| UpdateError::new(UpdateErrorKind::InvalidAsset))?;
            let candidate = UpdateCandidate {
                revision: UpdateRevision(Uuid::new_v4()),
                version: latest_version.clone(),
                asset_name: asset.name,
            };
            *self.lock_candidate() = Some(CandidateRecord {
                candidate: candidate.clone(),
                asset_url,
                installed_version: installed_version.clone(),
                target,
            });
            UpdateAvailability::Available(candidate)
        } else {
            *self.lock_candidate() = None;
            UpdateAvailability::Unavailable
        };

        Ok(UpdateCheckResult {
            installed_version,
            latest_version,
            summary: release.body,
            availability,
        })
    }

    pub fn install(
        &self,
        request: InstallUpdate,
        progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        let record = {
            let mut current = self.lock_candidate();
            let Some(record) = current.take() else {
                return Err(UpdateError::new(UpdateErrorKind::InvalidRevision));
            };
            if record.candidate.revision != request.revision
                || record.installed_version != self.environment.current_version()
            {
                return Err(UpdateError::new(UpdateErrorKind::InvalidRevision));
            }
            if record.candidate.version != request.confirmed_version {
                return Err(UpdateError::new(UpdateErrorKind::ConfirmationRequired));
            }
            record
        };

        let asset = ReleaseAsset {
            name: record.candidate.asset_name.clone(),
            browser_download_url: record.asset_url.clone(),
        };
        validate_update_response_url(&record.asset_url)
            .map_err(|_| UpdateError::new(UpdateErrorKind::InsecureAsset))?;
        validate_update_asset_for(&asset, record.target)
            .map_err(|_| UpdateError::new(UpdateErrorKind::InvalidAsset))?;
        let safe_name = safe_asset_name(&asset.name)
            .map_err(|_| UpdateError::new(UpdateErrorKind::InvalidAsset))?;
        let mut download = self
            .environment
            .open_asset_download(&record.asset_url)
            .map_err(|_| UpdateError::new(UpdateErrorKind::DownloadFailed))?;
        validate_update_response_url(&download.final_url)
            .map_err(|_| UpdateError::new(UpdateErrorKind::InsecureAsset))?;
        if download
            .content_length
            .is_some_and(|length| length > self.limits.download_bytes)
        {
            return Err(UpdateError::new(UpdateErrorKind::DownloadTooLarge));
        }

        let mut artifact = self
            .environment
            .create_update_artifact(&safe_name)
            .map_err(|_| UpdateError::new(UpdateErrorKind::WriteFailed))?;
        let result = self.write_publish_launch(&mut download, &mut artifact, &record, progress);
        if result.is_err() {
            self.environment.cleanup_update_artifact(&mut artifact);
        }
        result
    }

    fn write_publish_launch(
        &self,
        download: &mut UpdateDownload,
        artifact: &mut E::Artifact,
        record: &CandidateRecord,
        progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        let mut downloaded_bytes = 0_u64;
        for chunk in download.chunks.by_ref() {
            let chunk = chunk.map_err(|_| UpdateError::new(UpdateErrorKind::DownloadFailed))?;
            downloaded_bytes = downloaded_bytes
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| UpdateError::new(UpdateErrorKind::DownloadTooLarge))?;
            if downloaded_bytes > self.limits.download_bytes {
                return Err(UpdateError::new(UpdateErrorKind::DownloadTooLarge));
            }
            artifact
                .write_all(&chunk)
                .map_err(|_| UpdateError::new(UpdateErrorKind::WriteFailed))?;
            progress(UpdateProgress {
                downloaded_bytes,
                total_bytes: download.content_length,
            });
        }
        if download
            .content_length
            .is_some_and(|expected| expected != downloaded_bytes)
        {
            return Err(UpdateError::new(UpdateErrorKind::DownloadFailed));
        }
        artifact
            .flush()
            .map_err(|_| UpdateError::new(UpdateErrorKind::WriteFailed))?;
        self.environment
            .publish_update_artifact(artifact)
            .map_err(|_| UpdateError::new(UpdateErrorKind::PublishFailed))?;
        self.environment
            .launch_update_artifact(artifact)
            .map_err(|_| UpdateError::new(UpdateErrorKind::LaunchFailed))?;
        Ok(InstallStarted {
            version: record.candidate.version.clone(),
        })
    }
}

pub trait UpdateSource: Send + Sync + 'static {
    fn check(&self) -> Result<UpdateCheckResult, UpdateError>;
    fn install(
        &self,
        request: InstallUpdate,
        progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError>;
}

impl<E: UpdateEnvironment> UpdateSource for UpdateService<E> {
    fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        UpdateService::check(self)
    }

    fn install(
        &self,
        request: InstallUpdate,
        progress: UpdateProgressSink,
    ) -> Result<InstallStarted, UpdateError> {
        UpdateService::install(self, request, progress)
    }
}

pub struct SystemUpdateEnvironment {
    update_root: PathBuf,
    runtime: tokio::runtime::Runtime,
}

impl SystemUpdateEnvironment {
    pub fn new(update_root: PathBuf) -> Result<Self, UpdateEnvironmentError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("manager-update-io")
            .build()
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?;
        Ok(Self {
            update_root,
            runtime,
        })
    }

    fn client(&self) -> Result<reqwest::Client, UpdateEnvironmentError> {
        codex_plus_core::http_client::proxied_client(&format!(
            "Codex++/{}",
            codex_plus_core::version::VERSION
        ))
        .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))
    }
}

impl fmt::Debug for SystemUpdateEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemUpdateEnvironment")
            .field("has_update_root", &true)
            .finish_non_exhaustive()
    }
}

pub struct SystemUpdateArtifact {
    partial_path: PathBuf,
    final_path: PathBuf,
    file: Option<File>,
    published: bool,
}

impl fmt::Debug for SystemUpdateArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemUpdateArtifact")
            .field("open", &self.file.is_some())
            .field("published", &self.published)
            .finish()
    }
}

impl Write for SystemUpdateArtifact {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.file_mut()?.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file_mut()?.flush()
    }
}

impl SystemUpdateArtifact {
    fn file_mut(&mut self) -> std::io::Result<&mut File> {
        self.file.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "update artifact is closed")
        })
    }
}

impl UpdateEnvironment for SystemUpdateEnvironment {
    type Artifact = SystemUpdateArtifact;

    fn current_version(&self) -> String {
        codex_plus_core::version::VERSION.to_owned()
    }

    fn target(&self) -> UpdateTarget {
        codex_plus_core::update::current_update_target()
    }

    fn fetch_release_metadata(
        &self,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, UpdateEnvironmentError> {
        let client = self.client()?;
        self.runtime.block_on(async move {
            let mut response = client
                .get(codex_plus_core::update::DEFAULT_LATEST_JSON_URL)
                .header("accept", "application/json")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?;
            validate_update_response_url(response.url().as_str())
                .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?;
            if response
                .content_length()
                .is_some_and(|length| length > maximum_bytes as u64)
            {
                return Err(UpdateEnvironmentError::new(
                    UpdateEnvironmentErrorKind::Transport,
                    "release metadata content length exceeds limit",
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?
            {
                if bytes.len() + chunk.len() > maximum_bytes {
                    return Err(UpdateEnvironmentError::new(
                        UpdateEnvironmentErrorKind::Transport,
                        "release metadata stream exceeds limit",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
    }

    fn open_asset_download(&self, url: &str) -> Result<UpdateDownload, UpdateEnvironmentError> {
        validate_update_response_url(url)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?;
        let client = self.client()?;
        let mut response = self.runtime.block_on(async move {
            client
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))
        })?;
        let final_url = response.url().to_string();
        validate_update_response_url(&final_url)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Transport, error))?;
        let content_length = response.content_length();
        let (chunk_tx, chunk_rx) = mpsc::sync_channel(2);
        self.runtime.spawn(async move {
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if chunk_tx.send(Ok(chunk.to_vec())).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = chunk_tx.send(Err(environment_error(
                            UpdateEnvironmentErrorKind::Transport,
                            error,
                        )));
                        break;
                    }
                }
            }
        });
        Ok(UpdateDownload::new(
            final_url,
            content_length,
            chunk_rx.into_iter(),
        ))
    }

    fn create_update_artifact(
        &self,
        safe_name: &str,
    ) -> Result<Self::Artifact, UpdateEnvironmentError> {
        let safe_name = safe_asset_name(safe_name)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        std::fs::create_dir_all(&self.update_root)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        for _ in 0..16 {
            let token = Uuid::new_v4().simple().to_string();
            let partial_path = self.update_root.join(format!(".{token}-{safe_name}.part"));
            let final_path = self.update_root.join(format!("{token}-{safe_name}"));
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&partial_path)
            {
                Ok(file) => {
                    return Ok(SystemUpdateArtifact {
                        partial_path,
                        final_path,
                        file: Some(file),
                        published: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(environment_error(
                        UpdateEnvironmentErrorKind::Filesystem,
                        error,
                    ));
                }
            }
        }
        Err(UpdateEnvironmentError::new(
            UpdateEnvironmentErrorKind::Filesystem,
            "could not allocate a unique update artifact",
        ))
    }

    fn publish_update_artifact(
        &self,
        artifact: &mut Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        if artifact.published {
            return Err(UpdateEnvironmentError::new(
                UpdateEnvironmentErrorKind::Filesystem,
                "update artifact is already published",
            ));
        }
        let file = artifact.file.take().ok_or_else(|| {
            UpdateEnvironmentError::new(
                UpdateEnvironmentErrorKind::Filesystem,
                "update artifact is closed",
            )
        })?;
        file.sync_all()
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        drop(file);
        std::fs::hard_link(&artifact.partial_path, &artifact.final_path)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        std::fs::remove_file(&artifact.partial_path)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Filesystem, error))?;
        artifact.published = true;
        Ok(())
    }

    fn cleanup_update_artifact(&self, artifact: &mut Self::Artifact) {
        artifact.file.take();
        remove_owned_file(&artifact.partial_path);
        remove_owned_file(&artifact.final_path);
        artifact.published = false;
    }

    fn launch_update_artifact(
        &self,
        artifact: &Self::Artifact,
    ) -> Result<(), UpdateEnvironmentError> {
        if !artifact.published {
            return Err(UpdateEnvironmentError::new(
                UpdateEnvironmentErrorKind::Launcher,
                "update artifact is not published",
            ));
        }
        codex_plus_core::update::launch_installer(&artifact.final_path)
            .map_err(|error| environment_error(UpdateEnvironmentErrorKind::Launcher, error))
    }
}

fn safe_version(value: &str) -> Result<String, UpdateError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(char::is_control)
        || codex_plus_core::update::parse_version_tag(value).is_err()
    {
        return Err(UpdateError::new(UpdateErrorKind::MetadataInvalid));
    }
    Ok(value.to_owned())
}

fn bounded_detail(value: String) -> String {
    value.chars().take(512).collect()
}

fn environment_error(
    kind: UpdateEnvironmentErrorKind,
    error: impl fmt::Display,
) -> UpdateEnvironmentError {
    UpdateEnvironmentError::new(kind, error.to_string())
}

fn remove_owned_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        let _ = error;
    }
}
