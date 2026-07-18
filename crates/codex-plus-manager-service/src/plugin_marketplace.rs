use std::fmt;
use std::path::{Path, PathBuf};

use codex_plus_core::plugin_marketplace::PluginMarketplaceInspection;
use sha2::{Digest, Sha256};

pub use codex_plus_core::plugin_marketplace::PluginMarketplaceKind;

#[derive(Clone, PartialEq, Eq)]
pub struct PluginMarketplaceRevision([u8; 32]);

impl PluginMarketplaceRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for PluginMarketplaceRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PluginMarketplaceRevision(..)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginMarketplaceStatus {
    pub available: bool,
    pub config_registered: bool,
    pub needs_repair: bool,
    pub plugin_count: usize,
    pub skill_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginMarketplaceWorkspace {
    pub revision: PluginMarketplaceRevision,
    pub local: PluginMarketplaceStatus,
    pub remote: PluginMarketplaceStatus,
}

impl PluginMarketplaceWorkspace {
    pub fn status(&self, kind: PluginMarketplaceKind) -> &PluginMarketplaceStatus {
        match kind {
            PluginMarketplaceKind::Local => &self.local,
            PluginMarketplaceKind::Remote => &self.remote,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PluginMarketplaceCompatibilityWorkspace {
    pub workspace: PluginMarketplaceWorkspace,
    codex_home: PathBuf,
    local_marketplace_root: Option<PathBuf>,
    remote_marketplace_root: Option<PathBuf>,
}

impl PluginMarketplaceCompatibilityWorkspace {
    pub fn new(
        workspace: PluginMarketplaceWorkspace,
        codex_home: PathBuf,
        local_marketplace_root: Option<PathBuf>,
        remote_marketplace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            workspace,
            codex_home,
            local_marketplace_root,
            remote_marketplace_root,
        }
    }

    pub fn codex_home(&self) -> &Path {
        &self.codex_home
    }

    pub fn marketplace_root(&self, kind: PluginMarketplaceKind) -> Option<&Path> {
        match kind {
            PluginMarketplaceKind::Local => self.local_marketplace_root.as_deref(),
            PluginMarketplaceKind::Remote => self.remote_marketplace_root.as_deref(),
        }
    }
}

impl fmt::Debug for PluginMarketplaceCompatibilityWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginMarketplaceCompatibilityWorkspace")
            .field("workspace", &self.workspace)
            .field("has_codex_home", &true)
            .field(
                "has_local_marketplace_root",
                &self.local_marketplace_root.is_some(),
            )
            .field(
                "has_remote_marketplace_root",
                &self.remote_marketplace_root.is_some(),
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairPluginMarketplace {
    pub expected_revision: PluginMarketplaceRevision,
    pub kind: PluginMarketplaceKind,
    pub confirmed_kind: PluginMarketplaceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginMarketplaceRepairOutcome {
    Initialized,
    Configured,
    AlreadyHealthy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginMarketplaceRepair {
    pub outcome: PluginMarketplaceRepairOutcome,
    pub initialized: bool,
    pub configured: bool,
    pub workspace: PluginMarketplaceWorkspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginMarketplaceErrorKind {
    InspectFailed,
    DownloadFailed,
    DownloadTooLarge,
    ArchiveInvalid,
    WriteFailed,
    Conflict,
    WorkerStopped,
}

pub struct PluginMarketplaceError {
    kind: PluginMarketplaceErrorKind,
    http_status: Option<u16>,
    compatibility_detail: Option<String>,
}

impl PluginMarketplaceError {
    pub fn new(kind: PluginMarketplaceErrorKind) -> Self {
        Self {
            kind,
            http_status: None,
            compatibility_detail: None,
        }
    }

    pub(crate) fn with_compatibility_detail(
        kind: PluginMarketplaceErrorKind,
        http_status: Option<u16>,
        compatibility_detail: String,
    ) -> Self {
        Self {
            kind,
            http_status,
            compatibility_detail: Some(compatibility_detail),
        }
    }

    pub fn kind(&self) -> PluginMarketplaceErrorKind {
        self.kind
    }

    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    pub fn compatibility_detail(&self) -> Option<&str> {
        self.compatibility_detail.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            PluginMarketplaceErrorKind::InspectFailed => "plugin marketplace inspection failed",
            PluginMarketplaceErrorKind::DownloadFailed => "plugin marketplace download failed",
            PluginMarketplaceErrorKind::DownloadTooLarge => {
                "plugin marketplace download is too large"
            }
            PluginMarketplaceErrorKind::ArchiveInvalid => "plugin marketplace archive is invalid",
            PluginMarketplaceErrorKind::WriteFailed => "plugin marketplace write failed",
            PluginMarketplaceErrorKind::Conflict => "plugin marketplace state changed on disk",
            PluginMarketplaceErrorKind::WorkerStopped => "plugin marketplace worker stopped",
        }
    }
}

impl fmt::Debug for PluginMarketplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginMarketplaceError")
            .field("kind", &self.kind)
            .field("http_status", &self.http_status)
            .finish()
    }
}

impl fmt::Display for PluginMarketplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for PluginMarketplaceError {}

pub trait PluginMarketplaceEnvironment: Send + Sync + 'static {
    type Preparation: Send + 'static;

    fn inspect_plugin_marketplaces(
        &self,
    ) -> Result<PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceError>;

    fn prepare_plugin_marketplace(
        &self,
        kind: PluginMarketplaceKind,
    ) -> Result<Self::Preparation, PluginMarketplaceError>;

    fn commit_plugin_marketplace(
        &self,
        expected_revision: PluginMarketplaceRevision,
        kind: PluginMarketplaceKind,
        prepared: Self::Preparation,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError>;
}

pub trait PluginMarketplaceSource: Send + Sync + 'static {
    fn inspect(&self) -> Result<PluginMarketplaceWorkspace, PluginMarketplaceError>;
    fn repair(
        &self,
        request: RepairPluginMarketplace,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError>;
}

#[derive(Clone)]
pub struct PluginMarketplaceService<E> {
    environment: E,
}

impl<E> PluginMarketplaceService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: PluginMarketplaceEnvironment> PluginMarketplaceService<E> {
    pub fn inspect(&self) -> Result<PluginMarketplaceWorkspace, PluginMarketplaceError> {
        self.inspect_compatibility().map(|result| result.workspace)
    }

    pub fn inspect_compatibility(
        &self,
    ) -> Result<PluginMarketplaceCompatibilityWorkspace, PluginMarketplaceError> {
        self.environment.inspect_plugin_marketplaces()
    }

    pub fn repair(
        &self,
        request: RepairPluginMarketplace,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        if request.kind != request.confirmed_kind {
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::Conflict,
            ));
        }
        let current = self.inspect()?;
        if current.revision != request.expected_revision {
            return Err(PluginMarketplaceError::new(
                PluginMarketplaceErrorKind::Conflict,
            ));
        }
        if !current.status(request.kind).needs_repair {
            return Ok(PluginMarketplaceRepair {
                outcome: PluginMarketplaceRepairOutcome::AlreadyHealthy,
                initialized: false,
                configured: false,
                workspace: current,
            });
        }
        let prepared = self.environment.prepare_plugin_marketplace(request.kind)?;
        self.environment.commit_plugin_marketplace(
            request.expected_revision,
            request.kind,
            prepared,
        )
    }
}

impl<E: PluginMarketplaceEnvironment> PluginMarketplaceSource for PluginMarketplaceService<E> {
    fn inspect(&self) -> Result<PluginMarketplaceWorkspace, PluginMarketplaceError> {
        PluginMarketplaceService::inspect(self)
    }

    fn repair(
        &self,
        request: RepairPluginMarketplace,
    ) -> Result<PluginMarketplaceRepair, PluginMarketplaceError> {
        PluginMarketplaceService::repair(self, request)
    }
}

pub(crate) fn compatibility_workspace_from_core(
    codex_home: &Path,
    inspection: PluginMarketplaceInspection,
) -> PluginMarketplaceCompatibilityWorkspace {
    let mut hasher = Sha256::new();
    hasher.update(inspection.config_bytes());
    for record in [&inspection.local, &inspection.remote] {
        hasher.update([match record.kind {
            PluginMarketplaceKind::Local => 0,
            PluginMarketplaceKind::Remote => 1,
        }]);
        hasher.update([u8::from(record.available)]);
        hasher.update([u8::from(record.config_registered)]);
        hasher.update(record.plugin_count.to_le_bytes());
        hasher.update(record.skill_count.to_le_bytes());
        hasher.update(record.directory_identity());
    }
    let revision = PluginMarketplaceRevision::from_digest(hasher.finalize().into());
    let local_marketplace_root = inspection.local.marketplace_root.clone();
    let remote_marketplace_root = inspection.remote.marketplace_root.clone();
    PluginMarketplaceCompatibilityWorkspace::new(
        PluginMarketplaceWorkspace {
            revision,
            local: status_from_core(&inspection.local),
            remote: status_from_core(&inspection.remote),
        },
        codex_home.to_path_buf(),
        local_marketplace_root,
        remote_marketplace_root,
    )
}

fn status_from_core(
    record: &codex_plus_core::plugin_marketplace::PluginMarketplaceRecord,
) -> PluginMarketplaceStatus {
    PluginMarketplaceStatus {
        available: record.available,
        config_registered: record.config_registered,
        needs_repair: record.needs_repair(),
        plugin_count: record.plugin_count,
        skill_count: record.skill_count,
    }
}
