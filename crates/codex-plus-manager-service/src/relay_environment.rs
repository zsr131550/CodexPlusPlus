use std::fmt;
use std::path::Path;

use codex_plus_core::env_conflicts::{
    self, EnvConflict, EnvConflictRemoval, EnvConflictRemovalFailure,
};
use codex_plus_core::relay_environment::{self, RelayEnvironmentReport};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayEnvironmentErrorKind {
    ReadFailed,
    Conflict,
    InvalidSelection,
    MutationFailed,
}

pub struct RelayEnvironmentError {
    kind: RelayEnvironmentErrorKind,
}

impl RelayEnvironmentError {
    fn new(kind: RelayEnvironmentErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> RelayEnvironmentErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            RelayEnvironmentErrorKind::ReadFailed => "relay environment read failed",
            RelayEnvironmentErrorKind::Conflict => "relay environment changed",
            RelayEnvironmentErrorKind::InvalidSelection => "relay environment selection is invalid",
            RelayEnvironmentErrorKind::MutationFailed => "relay environment cleanup failed",
        }
    }
}

impl fmt::Debug for RelayEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayEnvironmentError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for RelayEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for RelayEnvironmentError {}

#[derive(Clone, PartialEq, Eq)]
pub struct RelayEnvironmentWorkspace {
    pub report: RelayEnvironmentReport,
    pub conflicts: Vec<EnvConflict>,
    pub revision: String,
}

impl fmt::Debug for RelayEnvironmentWorkspace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayEnvironmentWorkspace")
            .field("all_passed", &self.report.all_passed())
            .field("conflict_count", &self.conflicts.len())
            .field("revision", &self.revision)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveEnvironmentConflicts {
    pub expected_revision: String,
    pub names: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EnvironmentRemovalOutcome {
    pub removed: Vec<EnvConflictRemoval>,
    pub failures: Vec<EnvConflictRemovalFailure>,
    pub backup_path: Option<String>,
    pub remaining: Vec<EnvConflict>,
    pub report: RelayEnvironmentReport,
    pub revision: String,
}

impl fmt::Debug for EnvironmentRemovalOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentRemovalOutcome")
            .field("removed_count", &self.removed.len())
            .field("failure_count", &self.failures.len())
            .field("has_backup", &self.backup_path.is_some())
            .field("remaining_count", &self.remaining.len())
            .field("revision", &self.revision)
            .finish()
    }
}

pub trait RelayEnvironmentEnvironment: Send + Sync + 'static {
    fn environment_codex_home(&self) -> &Path;
    fn environment_backup_dir(&self) -> &Path;
    fn process_only_env_cleanup(&self) -> bool;
    fn isolated_environment_inspection(&self) -> bool;
}

pub trait RelayEnvironmentSource: Send + Sync + 'static {
    fn inspect(&self) -> Result<RelayEnvironmentWorkspace, RelayEnvironmentError>;
    fn remove_conflicts(
        &self,
        request: RemoveEnvironmentConflicts,
    ) -> Result<EnvironmentRemovalOutcome, RelayEnvironmentError>;
}

#[derive(Clone)]
pub struct RelayEnvironmentService<E> {
    environment: E,
}

impl<E> RelayEnvironmentService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E> RelayEnvironmentSource for RelayEnvironmentService<E>
where
    E: RelayEnvironmentEnvironment,
{
    fn inspect(&self) -> Result<RelayEnvironmentWorkspace, RelayEnvironmentError> {
        let isolated = self.environment.isolated_environment_inspection();
        let report = if isolated {
            relay_environment::inspect_process_relay_environment_at(
                self.environment.environment_codex_home(),
            )
        } else {
            relay_environment::inspect_relay_environment_at(
                self.environment.environment_codex_home(),
            )
        };
        let conflicts = if isolated {
            env_conflicts::detect_process_env_conflicts()
        } else {
            env_conflicts::detect_env_conflicts()
        };
        let revision = workspace_revision(&report, &conflicts)?;
        Ok(RelayEnvironmentWorkspace {
            report,
            conflicts,
            revision,
        })
    }

    fn remove_conflicts(
        &self,
        request: RemoveEnvironmentConflicts,
    ) -> Result<EnvironmentRemovalOutcome, RelayEnvironmentError> {
        let names = env_conflicts::normalize_conflict_names(&request.names);
        if names.is_empty() {
            return Err(RelayEnvironmentError::new(
                RelayEnvironmentErrorKind::InvalidSelection,
            ));
        }
        let before = self.inspect()?;
        if before.revision != request.expected_revision
            || names
                .iter()
                .any(|name| !before.conflicts.iter().any(|item| &item.name == name))
        {
            return Err(RelayEnvironmentError::new(
                RelayEnvironmentErrorKind::Conflict,
            ));
        }

        let result = if self.environment.process_only_env_cleanup() {
            env_conflicts::remove_process_env_conflicts_if_unchanged_for_tests(
                &names,
                &before.conflicts,
                self.environment.environment_backup_dir().to_path_buf(),
            )
        } else {
            env_conflicts::remove_env_conflicts_if_unchanged(
                &names,
                &before.conflicts,
                self.environment.environment_backup_dir().to_path_buf(),
            )
        }
        .map_err(|_| RelayEnvironmentError::new(RelayEnvironmentErrorKind::MutationFailed))?
        .ok_or_else(|| RelayEnvironmentError::new(RelayEnvironmentErrorKind::Conflict))?;

        let after = self.inspect()?;
        Ok(EnvironmentRemovalOutcome {
            removed: result.removed,
            failures: result.failures,
            backup_path: result.backup_path,
            remaining: after.conflicts,
            report: after.report,
            revision: after.revision,
        })
    }
}

fn workspace_revision(
    report: &RelayEnvironmentReport,
    conflicts: &[EnvConflict],
) -> Result<String, RelayEnvironmentError> {
    #[derive(Serialize)]
    struct RevisionInput<'a> {
        report: &'a RelayEnvironmentReport,
        conflicts: &'a [EnvConflict],
    }

    let bytes = serde_json::to_vec(&RevisionInput { report, conflicts })
        .map_err(|_| RelayEnvironmentError::new(RelayEnvironmentErrorKind::ReadFailed))?;
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    Ok(output)
}
