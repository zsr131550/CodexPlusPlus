use std::collections::BTreeMap;
use std::fmt;

use codex_plus_core::script_market::{
    MarketScript, MarketScriptIntegrity, ScriptMarketManifest, classify_digest,
};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UserScriptRevision([u8; 32]);

impl UserScriptRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for UserScriptRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserScriptRevision(..)")
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ScriptMarketRevision([u8; 32]);

impl ScriptMarketRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for ScriptMarketRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ScriptMarketRevision(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptOrigin {
    Builtin,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptStatus {
    Disabled,
    NotLoaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptIntegrity {
    Verified,
    Unverified,
    Invalid,
}

impl From<MarketScriptIntegrity> for ScriptIntegrity {
    fn from(value: MarketScriptIntegrity) -> Self {
        match value {
            MarketScriptIntegrity::Verified => Self::Verified,
            MarketScriptIntegrity::Unverified => Self::Unverified,
            MarketScriptIntegrity::Invalid => Self::Invalid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserScriptSummary {
    pub key: String,
    pub name: String,
    pub origin: UserScriptOrigin,
    pub enabled: bool,
    pub status: UserScriptStatus,
    pub market_id: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptWorkspace {
    pub revision: UserScriptRevision,
    pub globally_enabled: bool,
    pub scripts: Vec<UserScriptSummary>,
}

impl fmt::Debug for UserScriptWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptWorkspace")
            .field("revision", &self.revision)
            .field("globally_enabled", &self.globally_enabled)
            .field("script_count", &self.scripts.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptMarketSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub tags: Vec<String>,
    pub source_host: String,
    pub integrity: ScriptIntegrity,
    pub installed_version: Option<String>,
    pub update_available: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScriptMarketWorkspace {
    pub revision: ScriptMarketRevision,
    pub updated_at: Option<String>,
    pub entries: Vec<ScriptMarketSummary>,
}

impl fmt::Debug for ScriptMarketWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptMarketWorkspace")
            .field("revision", &self.revision)
            .field("has_updated_at", &self.updated_at.is_some())
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScriptMarketCompatibilityWorkspace {
    pub workspace: ScriptMarketWorkspace,
    scripts: Vec<MarketScript>,
}

impl ScriptMarketCompatibilityWorkspace {
    pub fn from_manifest(manifest: ScriptMarketManifest) -> Self {
        let revision = market_revision(&manifest);
        let mut scripts = manifest.scripts;
        scripts.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.version.cmp(&right.version))
        });
        let entries = scripts.iter().map(market_summary).collect();
        Self {
            workspace: ScriptMarketWorkspace {
                revision,
                updated_at: manifest.updated_at,
                entries,
            },
            scripts,
        }
    }

    fn script(&self, id: &str) -> Result<&MarketScript, UserScriptError> {
        let mut matches = self.scripts.iter().filter(|script| script.id == id);
        let Some(script) = matches.next() else {
            return Err(UserScriptError::new(UserScriptErrorKind::InvalidTarget));
        };
        if matches.next().is_some() {
            return Err(UserScriptError::new(UserScriptErrorKind::InvalidTarget));
        }
        Ok(script)
    }

    pub fn scripts(&self) -> &[MarketScript] {
        &self.scripts
    }
}

impl fmt::Debug for ScriptMarketCompatibilityWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptMarketCompatibilityWorkspace")
            .field("workspace", &self.workspace)
            .field("script_count", &self.scripts.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallMarketScript {
    pub expected_local_revision: UserScriptRevision,
    pub expected_market_revision: ScriptMarketRevision,
    pub script_id: String,
    pub confirmed_script_id: String,
    pub confirmed_version: String,
    pub acknowledge_unverified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetUserScriptsEnabled {
    pub expected_revision: UserScriptRevision,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetUserScriptEnabled {
    pub expected_revision: UserScriptRevision,
    pub key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteUserScript {
    pub expected_revision: UserScriptRevision,
    pub key: String,
    pub confirmed_key: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptBackupEvidence {
    pub id: String,
    pub created: bool,
}

impl UserScriptBackupEvidence {
    pub fn none() -> Self {
        Self {
            id: String::new(),
            created: false,
        }
    }
}

impl fmt::Debug for UserScriptBackupEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptBackupEvidence")
            .field("id", &self.id)
            .field("created", &self.created)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptMutationOutcome {
    pub workspace: UserScriptWorkspace,
    pub backup: UserScriptBackupEvidence,
}

impl fmt::Debug for UserScriptMutationOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptMutationOutcome")
            .field("workspace", &self.workspace)
            .field("backup", &self.backup)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptErrorKind {
    InspectFailed,
    MarketRefreshFailed,
    DownloadFailed,
    DownloadTooLarge,
    InvalidIntegrity,
    IntegrityMismatch,
    UnverifiedNotAcknowledged,
    ConfirmationMismatch,
    InvalidTarget,
    Conflict,
    BackupFailed,
    WriteFailed,
    RollbackFailed,
    WorkerStopped,
}

pub struct UserScriptError {
    kind: UserScriptErrorKind,
    compatibility_detail: Option<String>,
}

impl UserScriptError {
    pub fn new(kind: UserScriptErrorKind) -> Self {
        Self {
            kind,
            compatibility_detail: None,
        }
    }

    pub fn with_compatibility_detail(kind: UserScriptErrorKind, detail: String) -> Self {
        Self {
            kind,
            compatibility_detail: Some(detail),
        }
    }

    pub fn kind(&self) -> UserScriptErrorKind {
        self.kind
    }

    pub fn compatibility_detail(&self) -> Option<&str> {
        self.compatibility_detail.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            UserScriptErrorKind::InspectFailed => "user script inspection failed",
            UserScriptErrorKind::MarketRefreshFailed => "script market refresh failed",
            UserScriptErrorKind::DownloadFailed => "script download failed",
            UserScriptErrorKind::DownloadTooLarge => "script download is too large",
            UserScriptErrorKind::InvalidIntegrity => "script digest is invalid",
            UserScriptErrorKind::IntegrityMismatch => "script digest does not match content",
            UserScriptErrorKind::UnverifiedNotAcknowledged => {
                "unverified script was not acknowledged"
            }
            UserScriptErrorKind::ConfirmationMismatch => "script confirmation does not match",
            UserScriptErrorKind::InvalidTarget => "invalid user script target",
            UserScriptErrorKind::Conflict => "user script state changed on disk",
            UserScriptErrorKind::BackupFailed => "user script backup failed",
            UserScriptErrorKind::WriteFailed => "user script write failed",
            UserScriptErrorKind::RollbackFailed => "user script rollback failed",
            UserScriptErrorKind::WorkerStopped => "user script worker stopped",
        }
    }
}

impl fmt::Debug for UserScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for UserScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for UserScriptError {}

pub trait UserScriptEnvironment: Send + Sync + 'static {
    type Prepared: Send + 'static;

    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError>;
    fn refresh_market(&self) -> Result<ScriptMarketCompatibilityWorkspace, UserScriptError>;
    fn prepare_market_script(
        &self,
        script: &MarketScript,
    ) -> Result<Self::Prepared, UserScriptError>;
    fn commit_market_script(
        &self,
        expected_revision: UserScriptRevision,
        prepared: Self::Prepared,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn set_global_enabled(
        &self,
        expected_revision: UserScriptRevision,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn set_script_enabled(
        &self,
        expected_revision: UserScriptRevision,
        key: &str,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn delete_user_script(
        &self,
        expected_revision: UserScriptRevision,
        key: &str,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
}

pub trait UserScriptSource: Send + Sync + 'static {
    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError>;
    fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError>;
    fn install(
        &self,
        request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn set_global_enabled(
        &self,
        request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn set_script_enabled(
        &self,
        request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
    fn delete(
        &self,
        request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError>;
}

#[derive(Clone)]
pub struct UserScriptService<E> {
    environment: E,
}

impl<E> UserScriptService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: UserScriptEnvironment> UserScriptService<E> {
    pub fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        self.environment.inspect_local()
    }

    pub fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError> {
        let (compatibility, local) = self.refresh_market_compatibility()?;
        Ok(decorate_market_workspace(compatibility.workspace, &local))
    }

    pub fn refresh_market_compatibility(
        &self,
    ) -> Result<(ScriptMarketCompatibilityWorkspace, UserScriptWorkspace), UserScriptError> {
        let compatibility = self.environment.refresh_market()?;
        let local = self.inspect_local()?;
        Ok((compatibility, local))
    }

    pub fn install(
        &self,
        request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        if request.script_id.trim().is_empty()
            || request.script_id != request.confirmed_script_id
            || request.confirmed_version.trim().is_empty()
        {
            return Err(UserScriptError::new(
                UserScriptErrorKind::ConfirmationMismatch,
            ));
        }

        let market = self.environment.refresh_market()?;
        if market.workspace.revision != request.expected_market_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        let script = market.script(&request.script_id)?;
        if script.version != request.confirmed_version {
            return Err(UserScriptError::new(
                UserScriptErrorKind::ConfirmationMismatch,
            ));
        }
        match classify_digest(&script.sha256) {
            MarketScriptIntegrity::Invalid => {
                return Err(UserScriptError::new(UserScriptErrorKind::InvalidIntegrity));
            }
            MarketScriptIntegrity::Unverified if !request.acknowledge_unverified => {
                return Err(UserScriptError::new(
                    UserScriptErrorKind::UnverifiedNotAcknowledged,
                ));
            }
            MarketScriptIntegrity::Verified | MarketScriptIntegrity::Unverified => {}
        }

        let local = self.inspect_local()?;
        if local.revision != request.expected_local_revision {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        let prepared = self.environment.prepare_market_script(script)?;
        self.environment
            .commit_market_script(request.expected_local_revision, prepared)
    }

    pub fn set_global_enabled(
        &self,
        request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        self.require_current_revision(&request.expected_revision)?;
        self.environment
            .set_global_enabled(request.expected_revision, request.enabled)
    }

    pub fn set_script_enabled(
        &self,
        request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        let current = self.require_current_revision(&request.expected_revision)?;
        if !current
            .scripts
            .iter()
            .any(|script| script.key == request.key)
        {
            return Err(UserScriptError::new(UserScriptErrorKind::InvalidTarget));
        }
        self.environment.set_script_enabled(
            request.expected_revision,
            &request.key,
            request.enabled,
        )
    }

    pub fn delete(
        &self,
        request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        if request.key.trim().is_empty() || request.key != request.confirmed_key {
            return Err(UserScriptError::new(
                UserScriptErrorKind::ConfirmationMismatch,
            ));
        }
        let current = self.require_current_revision(&request.expected_revision)?;
        let Some(script) = current
            .scripts
            .iter()
            .find(|script| script.key == request.key)
        else {
            return Err(UserScriptError::new(UserScriptErrorKind::InvalidTarget));
        };
        if script.origin != UserScriptOrigin::User {
            return Err(UserScriptError::new(UserScriptErrorKind::InvalidTarget));
        }
        self.environment
            .delete_user_script(request.expected_revision, &request.key)
    }

    fn require_current_revision(
        &self,
        expected: &UserScriptRevision,
    ) -> Result<UserScriptWorkspace, UserScriptError> {
        let current = self.inspect_local()?;
        if current.revision != *expected {
            return Err(UserScriptError::new(UserScriptErrorKind::Conflict));
        }
        Ok(current)
    }
}

impl<E: UserScriptEnvironment> UserScriptSource for UserScriptService<E> {
    fn inspect_local(&self) -> Result<UserScriptWorkspace, UserScriptError> {
        UserScriptService::inspect_local(self)
    }

    fn refresh_market(&self) -> Result<ScriptMarketWorkspace, UserScriptError> {
        UserScriptService::refresh_market(self)
    }

    fn install(
        &self,
        request: InstallMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        UserScriptService::install(self, request)
    }

    fn set_global_enabled(
        &self,
        request: SetUserScriptsEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        UserScriptService::set_global_enabled(self, request)
    }

    fn set_script_enabled(
        &self,
        request: SetUserScriptEnabled,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        UserScriptService::set_script_enabled(self, request)
    }

    fn delete(
        &self,
        request: DeleteUserScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptError> {
        UserScriptService::delete(self, request)
    }
}

pub(crate) fn workspace_from_core(
    inspection: codex_plus_core::user_scripts::UserScriptInspection,
) -> UserScriptWorkspace {
    let mut scripts = inspection
        .inventory
        .scripts
        .into_iter()
        .map(|script| UserScriptSummary {
            key: script.key,
            name: script.name,
            origin: match script.source {
                codex_plus_core::user_scripts::UserScriptOrigin::Builtin => {
                    UserScriptOrigin::Builtin
                }
                codex_plus_core::user_scripts::UserScriptOrigin::User => UserScriptOrigin::User,
            },
            enabled: script.enabled,
            status: match script.status {
                codex_plus_core::user_scripts::UserScriptStatus::Disabled => {
                    UserScriptStatus::Disabled
                }
                codex_plus_core::user_scripts::UserScriptStatus::NotLoaded => {
                    UserScriptStatus::NotLoaded
                }
            },
            market_id: script.market.as_ref().map(|market| market.id.clone()),
            version: script.market.map(|market| market.version),
        })
        .collect::<Vec<_>>();
    scripts.sort_by(|left, right| {
        origin_rank(left.origin)
            .cmp(&origin_rank(right.origin))
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then_with(|| left.key.cmp(&right.key))
    });
    UserScriptWorkspace {
        revision: UserScriptRevision::from_digest(inspection.revision.digest()),
        globally_enabled: inspection.inventory.enabled,
        scripts,
    }
}

fn market_summary(script: &MarketScript) -> ScriptMarketSummary {
    let source_host = url::Url::parse(&script.script_url)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_default();
    ScriptMarketSummary {
        id: script.id.clone(),
        name: script.name.clone(),
        description: script.description.clone(),
        version: script.version.clone(),
        author: script.author.clone(),
        tags: script.tags.clone(),
        source_host,
        integrity: classify_digest(&script.sha256).into(),
        installed_version: None,
        update_available: false,
    }
}

fn decorate_market_workspace(
    mut market: ScriptMarketWorkspace,
    local: &UserScriptWorkspace,
) -> ScriptMarketWorkspace {
    let installed = local
        .scripts
        .iter()
        .filter_map(|script| Some((script.market_id.as_ref()?, script.version.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    for entry in &mut market.entries {
        entry.installed_version = installed.get(&entry.id).map(|version| (*version).clone());
        entry.update_available = entry
            .installed_version
            .as_ref()
            .is_some_and(|version| version != &entry.version);
    }
    market
}

fn market_revision(manifest: &ScriptMarketManifest) -> ScriptMarketRevision {
    let mut scripts = manifest.scripts.iter().collect::<Vec<_>>();
    scripts.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.script_url.cmp(&right.script_url))
    });
    let mut digest = Sha256::new();
    hash_field(&mut digest, &manifest.version.to_le_bytes());
    hash_field(
        &mut digest,
        manifest
            .updated_at
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    for script in scripts {
        for value in [
            &script.id,
            &script.name,
            &script.description,
            &script.version,
            &script.author,
            &script.homepage,
            &script.script_url,
            &script.sha256,
        ] {
            hash_field(&mut digest, value.as_bytes());
        }
        for tag in &script.tags {
            hash_field(&mut digest, tag.as_bytes());
        }
    }
    ScriptMarketRevision::from_digest(digest.finalize().into())
}

fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

fn origin_rank(origin: UserScriptOrigin) -> u8 {
    match origin {
        UserScriptOrigin::Builtin => 0,
        UserScriptOrigin::User => 1,
    }
}
