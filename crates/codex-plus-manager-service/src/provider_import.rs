use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use codex_plus_core::ccs_import::{
    self, CcsProviderImport, apply_ccs_providers_to_settings, imported_provider_identity,
    provider_identity_from_ccs,
};
use codex_plus_core::provider_import::{
    self, ProviderImportRequest, acquire_pending_provider_import_lock,
    apply_provider_import_to_settings,
};
use codex_plus_core::settings::{BackendSettings, RelayProtocol};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{ProviderEnvironment, ProviderRevision, ProviderService, ProviderWorkspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderImportErrorKind {
    SourceUnavailable,
    SourceChanged,
    PendingUnavailable,
    PendingConflict,
    ProviderConflict,
    ReadFailed,
    SaveFailed,
    InvalidRequest,
}

pub struct ProviderImportError {
    kind: ProviderImportErrorKind,
}

impl ProviderImportError {
    fn new(kind: ProviderImportErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> ProviderImportErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ProviderImportErrorKind::SourceUnavailable => "provider import source unavailable",
            ProviderImportErrorKind::SourceChanged => "provider import source changed",
            ProviderImportErrorKind::PendingUnavailable => "pending provider import unavailable",
            ProviderImportErrorKind::PendingConflict => "pending provider import changed",
            ProviderImportErrorKind::ProviderConflict => "provider workspace changed on disk",
            ProviderImportErrorKind::ReadFailed => "provider import read failed",
            ProviderImportErrorKind::SaveFailed => "provider import save failed",
            ProviderImportErrorKind::InvalidRequest => "provider import request invalid",
        }
    }
}

impl fmt::Debug for ProviderImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderImportError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for ProviderImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ProviderImportError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CcsProviderSummary {
    pub source_id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: RelayProtocol,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CcsDiscovery {
    pub source_path: String,
    pub source_revision: String,
    pub provider_revision: ProviderRevision,
    pub providers: Vec<CcsProviderSummary>,
    pub importable_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCcsProviders {
    pub source_revision: String,
    pub provider_revision: ProviderRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingImportSummary {
    pub name: String,
    pub base_url: String,
    pub wire_api: String,
    pub relay_mode: String,
    pub api_key_present: bool,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingImportSnapshot {
    pub pending: Option<PendingImportSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmPendingImport {
    pub pending_revision: String,
    pub provider_revision: ProviderRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DismissPendingImport {
    pub pending_revision: String,
}

#[derive(Clone)]
pub struct ProviderImportOutcome {
    pub imported: usize,
    pub duplicates: usize,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub workspace: ProviderWorkspace,
}

impl fmt::Debug for ProviderImportOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderImportOutcome")
            .field("imported", &self.imported)
            .field("duplicates", &self.duplicates)
            .field("has_profile", &self.profile_id.is_some())
            .field("workspace", &self.workspace)
            .finish()
    }
}

pub trait ProviderImportEnvironment: ProviderEnvironment {
    fn ccs_db_path(&self) -> &Path;
    fn pending_import_path(&self) -> &Path;
}

pub trait ProviderImportSource: Send + Sync + 'static {
    fn discover_ccs(&self) -> Result<CcsDiscovery, ProviderImportError>;
    fn import_ccs(
        &self,
        request: ImportCcsProviders,
    ) -> Result<ProviderImportOutcome, ProviderImportError>;
    fn load_pending(&self) -> Result<PendingImportSnapshot, ProviderImportError>;
    fn confirm_pending(
        &self,
        request: ConfirmPendingImport,
    ) -> Result<ProviderImportOutcome, ProviderImportError>;
    fn dismiss_pending(
        &self,
        request: DismissPendingImport,
    ) -> Result<PendingImportSnapshot, ProviderImportError>;
}

#[derive(Clone)]
pub struct ProviderImportService<E> {
    environment: E,
}

impl<E> ProviderImportService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E> ProviderImportService<E>
where
    E: ProviderImportEnvironment + Clone,
{
    pub fn load_ccs_records(&self) -> Result<Vec<CcsProviderImport>, ProviderImportError> {
        ccs_import::list_codex_providers_from_db(self.environment.ccs_db_path())
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::SourceUnavailable))
    }

    pub fn ccs_source_path(&self) -> &Path {
        self.environment.ccs_db_path()
    }

    pub fn load_pending_record(
        &self,
    ) -> Result<Option<ProviderImportRequest>, ProviderImportError> {
        provider_import::load_pending_provider_import_at(self.environment.pending_import_path())
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))
    }

    pub fn current_provider_revision(&self) -> Result<ProviderRevision, ProviderImportError> {
        self.load_workspace()
            .map(|(_, workspace)| workspace.revision)
    }

    fn workspace_from_settings(
        &self,
        settings: &BackendSettings,
    ) -> Result<ProviderWorkspace, ProviderImportError> {
        ProviderService::new(self.environment.clone())
            .workspace_from_settings(settings)
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))
    }

    fn load_workspace(&self) -> Result<(BackendSettings, ProviderWorkspace), ProviderImportError> {
        let settings = self
            .environment
            .load_settings()
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?;
        let workspace = self.workspace_from_settings(&settings)?;
        Ok((settings, workspace))
    }

    fn persist_import(
        &self,
        expected_revision: ProviderRevision,
        next: &BackendSettings,
        imported: usize,
        duplicates: usize,
        profile: Option<(String, String)>,
    ) -> Result<ProviderImportOutcome, ProviderImportError> {
        let verifier = ProviderService::new(self.environment.clone());
        let payload = json!({
            "relayProfiles": next.relay_profiles,
            "activeRelayId": next.active_relay_id,
        });
        let updated = self
            .environment
            .update_settings_if(payload, |fresh| {
                verifier
                    .workspace_from_settings(fresh)
                    .is_ok_and(|workspace| workspace.revision == expected_revision)
            })
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::SaveFailed))?
            .ok_or_else(|| ProviderImportError::new(ProviderImportErrorKind::ProviderConflict))?;
        let workspace = self.workspace_from_settings(&updated)?;
        Ok(ProviderImportOutcome {
            imported,
            duplicates,
            profile_id: profile.as_ref().map(|(id, _)| id.clone()),
            profile_name: profile.map(|(_, name)| name),
            workspace,
        })
    }
}

impl<E> ProviderImportSource for ProviderImportService<E>
where
    E: ProviderImportEnvironment + Clone,
{
    fn discover_ccs(&self) -> Result<CcsDiscovery, ProviderImportError> {
        let providers = self.load_ccs_records()?;
        let (_, workspace) = self.load_workspace()?;
        let mut identities = workspace
            .document
            .profiles
            .iter()
            .filter_map(|profile| profile.ordinary())
            .map(imported_provider_identity)
            .collect::<HashSet<_>>();
        let mut duplicate_count = 0usize;
        let summaries = providers
            .iter()
            .map(|provider| {
                let duplicate = !identities.insert(provider_identity_from_ccs(provider));
                duplicate_count += usize::from(duplicate);
                CcsProviderSummary {
                    source_id: provider.source_id.clone(),
                    name: provider.name.clone(),
                    base_url: provider.base_url.clone(),
                    protocol: provider.protocol,
                    duplicate,
                }
            })
            .collect::<Vec<_>>();
        Ok(CcsDiscovery {
            source_path: self.environment.ccs_db_path().to_string_lossy().to_string(),
            source_revision: revision_of(&providers)?,
            provider_revision: workspace.revision,
            importable_count: summaries.len() - duplicate_count,
            duplicate_count,
            providers: summaries,
        })
    }

    fn import_ccs(
        &self,
        request: ImportCcsProviders,
    ) -> Result<ProviderImportOutcome, ProviderImportError> {
        let providers = self.load_ccs_records()?;
        if revision_of(&providers)? != request.source_revision {
            return Err(ProviderImportError::new(
                ProviderImportErrorKind::SourceChanged,
            ));
        }
        let (settings, workspace) = self.load_workspace()?;
        if workspace.revision != request.provider_revision {
            return Err(ProviderImportError::new(
                ProviderImportErrorKind::ProviderConflict,
            ));
        }
        let (next, summary) = apply_ccs_providers_to_settings(&settings, &providers)
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::InvalidRequest))?;
        self.persist_import(
            request.provider_revision,
            &next,
            summary.imported,
            summary.duplicates,
            None,
        )
    }

    fn load_pending(&self) -> Result<PendingImportSnapshot, ProviderImportError> {
        let pending = self
            .load_pending_record()?
            .map(|request| pending_summary(&request))
            .transpose()?;
        Ok(PendingImportSnapshot { pending })
    }

    fn confirm_pending(
        &self,
        request: ConfirmPendingImport,
    ) -> Result<ProviderImportOutcome, ProviderImportError> {
        let pending_lock =
            acquire_pending_provider_import_lock(self.environment.pending_import_path())
                .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?;
        let pending = pending_lock
            .load()
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?
            .ok_or_else(|| ProviderImportError::new(ProviderImportErrorKind::PendingUnavailable))?;
        if revision_of(&pending)? != request.pending_revision {
            return Err(ProviderImportError::new(
                ProviderImportErrorKind::PendingConflict,
            ));
        }
        let (settings, workspace) = self.load_workspace()?;
        if workspace.revision != request.provider_revision {
            return Err(ProviderImportError::new(
                ProviderImportErrorKind::ProviderConflict,
            ));
        }
        let (next, result) = apply_provider_import_to_settings(&settings, &pending)
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::InvalidRequest))?;
        let outcome = self.persist_import(
            request.provider_revision,
            &next,
            usize::from(result.imported),
            usize::from(!result.imported),
            Some((result.profile_id, result.profile_name)),
        )?;
        pending_lock
            .clear()
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::SaveFailed))?;
        Ok(outcome)
    }

    fn dismiss_pending(
        &self,
        request: DismissPendingImport,
    ) -> Result<PendingImportSnapshot, ProviderImportError> {
        let pending_lock =
            acquire_pending_provider_import_lock(self.environment.pending_import_path())
                .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?;
        let pending = pending_lock
            .load()
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?;
        let Some(pending) = pending else {
            return Ok(PendingImportSnapshot { pending: None });
        };
        if revision_of(&pending)? != request.pending_revision {
            return Err(ProviderImportError::new(
                ProviderImportErrorKind::PendingConflict,
            ));
        }
        pending_lock
            .clear()
            .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::SaveFailed))?;
        Ok(PendingImportSnapshot { pending: None })
    }
}

fn pending_summary(
    request: &ProviderImportRequest,
) -> Result<PendingImportSummary, ProviderImportError> {
    Ok(PendingImportSummary {
        name: request.name.clone(),
        base_url: request.base_url.clone(),
        wire_api: request.wire_api.clone(),
        relay_mode: request.relay_mode.clone(),
        api_key_present: !request.api_key.trim().is_empty(),
        revision: revision_of(request)?,
    })
}

fn revision_of<T: Serialize>(value: &T) -> Result<String, ProviderImportError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| ProviderImportError::new(ProviderImportErrorKind::ReadFailed))?;
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
