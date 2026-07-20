use std::fmt;
use std::path::Path;
use std::sync::Arc;

use codex_plus_core::desktop_integration::{
    DesktopIntegrationAssessment, DesktopIntegrationHealth, DesktopIntegrationItem,
    DesktopRepairOperation, MacosDesktopSnapshot, ShortcutSnapshot, WindowsDesktopSnapshot,
    assess_macos_desktop_integration, assess_windows_desktop_integration,
};
use codex_plus_core::startup_registration::{
    OwnedStringValueSnapshot, StartAtSignInHealth, StartupRegistrationOperation,
    StartupRegistrationSnapshot, build_migrate_start_at_sign_in_plan,
    build_set_start_at_sign_in_plan, inspect_start_at_sign_in,
};
use sha2::{Digest, Sha256};

use crate::revision_ledger::{RevisionLedger, RevisionScope, RevisionTicket};

const DESKTOP_INTEGRATION_FINGERPRINT_DOMAIN: &[u8] = b"codex-plus-manager/desktop-integration/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationPlatform {
    Windows,
    Macos,
    Unsupported,
}

#[derive(Clone, PartialEq, Eq)]
pub enum DesktopIntegrationEnvironmentSnapshot {
    Windows {
        repair: Box<WindowsDesktopSnapshot>,
        sign_in: StartupRegistrationSnapshot,
    },
    Macos {
        repair: MacosDesktopSnapshot,
    },
    Unsupported,
}

impl fmt::Debug for DesktopIntegrationEnvironmentSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Windows { .. } => "DesktopIntegrationEnvironmentSnapshot::Windows(<redacted>)",
            Self::Macos { .. } => "DesktopIntegrationEnvironmentSnapshot::Macos(<redacted>)",
            Self::Unsupported => "DesktopIntegrationEnvironmentSnapshot::Unsupported",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationEnvironmentError {
    InspectFailed,
    EffectFailed,
}

pub trait DesktopIntegrationEnvironment: Send + Sync + 'static {
    fn inspect_desktop_integration(
        &self,
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationEnvironmentError>;

    fn apply_desktop_repair_operation(
        &self,
        operation: &DesktopRepairOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError>;

    fn apply_startup_registration_operation(
        &self,
        operation: &StartupRegistrationOperation,
    ) -> Result<(), DesktopIntegrationEnvironmentError>;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DesktopIntegrationRevision(RevisionTicket);

impl fmt::Debug for DesktopIntegrationRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DesktopIntegrationRevision([opaque])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartAtSignInStatus {
    pub health: StartAtSignInHealth,
    pub effective_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopIntegrationWorkspace {
    pub revision: DesktopIntegrationRevision,
    pub platform: DesktopIntegrationPlatform,
    pub repair_health: DesktopIntegrationHealth,
    pub repair_items: Vec<DesktopIntegrationItem>,
    pub sign_in: Option<StartAtSignInStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairDesktopIntegration {
    pub expected_revision: DesktopIntegrationRevision,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrateStartAtSignIn {
    pub expected_revision: DesktopIntegrationRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetStartAtSignIn {
    pub expected_revision: DesktopIntegrationRevision,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationMutationKind {
    Repair,
    MigrateSignIn,
    EnableSignIn,
    DisableSignIn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopIntegrationMutation {
    pub kind: DesktopIntegrationMutationKind,
    pub applied_operation_count: usize,
    pub workspace: DesktopIntegrationWorkspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIntegrationErrorKind {
    InspectFailed,
    InvalidRevision,
    Conflict,
    ConfirmationRequired,
    RepairUnavailable,
    MigrationUnavailable,
    SignInUnavailable,
    EffectFailed,
    WorkerStopped,
}

pub struct DesktopIntegrationError {
    kind: DesktopIntegrationErrorKind,
    refreshed_workspace: Option<DesktopIntegrationWorkspace>,
}

impl DesktopIntegrationError {
    pub fn new(kind: DesktopIntegrationErrorKind) -> Self {
        Self {
            kind,
            refreshed_workspace: None,
        }
    }

    fn with_refreshed_workspace(mut self, workspace: DesktopIntegrationWorkspace) -> Self {
        self.refreshed_workspace = Some(workspace);
        self
    }

    pub const fn kind(&self) -> DesktopIntegrationErrorKind {
        self.kind
    }

    pub fn refreshed_workspace(&self) -> Option<&DesktopIntegrationWorkspace> {
        self.refreshed_workspace.as_ref()
    }

    pub const fn detail(&self) -> &'static str {
        match self.kind {
            DesktopIntegrationErrorKind::InspectFailed => "desktop integration inspection failed",
            DesktopIntegrationErrorKind::InvalidRevision => {
                "desktop integration revision is invalid"
            }
            DesktopIntegrationErrorKind::Conflict => "desktop integration state changed",
            DesktopIntegrationErrorKind::ConfirmationRequired => {
                "desktop integration confirmation is required"
            }
            DesktopIntegrationErrorKind::RepairUnavailable => {
                "desktop integration repair is unavailable"
            }
            DesktopIntegrationErrorKind::MigrationUnavailable => {
                "start-at-sign-in migration is unavailable"
            }
            DesktopIntegrationErrorKind::SignInUnavailable => "start-at-sign-in is unavailable",
            DesktopIntegrationErrorKind::EffectFailed => "desktop integration operation failed",
            DesktopIntegrationErrorKind::WorkerStopped => "desktop integration worker stopped",
        }
    }
}

impl fmt::Debug for DesktopIntegrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopIntegrationError")
            .field("kind", &self.kind)
            .field(
                "has_refreshed_workspace",
                &self.refreshed_workspace.is_some(),
            )
            .finish()
    }
}

impl fmt::Display for DesktopIntegrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for DesktopIntegrationError {}

#[derive(Clone)]
pub struct DesktopIntegrationService<E> {
    environment: E,
    revisions: Arc<RevisionLedger>,
}

impl<E> DesktopIntegrationService<E> {
    pub fn new(environment: E) -> Self {
        Self {
            environment,
            revisions: Arc::new(RevisionLedger::default()),
        }
    }
}

impl<E: DesktopIntegrationEnvironment> DesktopIntegrationService<E> {
    pub fn inspect(&self) -> Result<DesktopIntegrationWorkspace, DesktopIntegrationError> {
        let snapshot = self
            .environment
            .inspect_desktop_integration()
            .map_err(|_| {
                DesktopIntegrationError::new(DesktopIntegrationErrorKind::InspectFailed)
            })?;
        Ok(self.workspace_from_snapshot(&snapshot))
    }

    pub fn repair(
        &self,
        request: RepairDesktopIntegration,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        let expected = self.take_revision(request.expected_revision)?;
        if !request.confirmed {
            return Err(DesktopIntegrationError::new(
                DesktopIntegrationErrorKind::ConfirmationRequired,
            ));
        }
        let snapshot = self.current_snapshot(expected)?;
        let assessment = assess_repair(&snapshot);
        let Some(plan) = assessment.plan else {
            if assessment.health == DesktopIntegrationHealth::Current {
                return self.completed_mutation(DesktopIntegrationMutationKind::Repair, 0);
            }
            return Err(
                self.error_with_snapshot(DesktopIntegrationErrorKind::RepairUnavailable, &snapshot)
            );
        };
        let operation_count = plan.operations.len();
        for operation in &plan.operations {
            self.environment
                .apply_desktop_repair_operation(operation)
                .map_err(|_| {
                    self.error_after_effect_failure(DesktopIntegrationErrorKind::EffectFailed)
                })?;
        }
        self.completed_mutation(DesktopIntegrationMutationKind::Repair, operation_count)
    }

    pub fn migrate_sign_in(
        &self,
        request: MigrateStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        let expected = self.take_revision(request.expected_revision)?;
        let snapshot = self.current_snapshot(expected)?;
        let DesktopIntegrationEnvironmentSnapshot::Windows { sign_in, .. } = &snapshot else {
            return Err(
                self.error_with_snapshot(DesktopIntegrationErrorKind::SignInUnavailable, &snapshot)
            );
        };
        let plan = build_migrate_start_at_sign_in_plan(sign_in).map_err(|_| {
            self.error_with_snapshot(DesktopIntegrationErrorKind::MigrationUnavailable, &snapshot)
        })?;
        let operation_count = plan.operations.len();
        for operation in &plan.operations {
            self.environment
                .apply_startup_registration_operation(operation)
                .map_err(|_| {
                    self.error_after_effect_failure(DesktopIntegrationErrorKind::EffectFailed)
                })?;
        }
        self.completed_mutation(
            DesktopIntegrationMutationKind::MigrateSignIn,
            operation_count,
        )
    }

    pub fn set_start_at_sign_in(
        &self,
        request: SetStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        let expected = self.take_revision(request.expected_revision)?;
        let snapshot = self.current_snapshot(expected)?;
        let DesktopIntegrationEnvironmentSnapshot::Windows { sign_in, .. } = &snapshot else {
            return Err(
                self.error_with_snapshot(DesktopIntegrationErrorKind::SignInUnavailable, &snapshot)
            );
        };
        if request.enabled
            && inspect_start_at_sign_in(sign_in).health == StartAtSignInHealth::Unavailable
        {
            return Err(
                self.error_with_snapshot(DesktopIntegrationErrorKind::SignInUnavailable, &snapshot)
            );
        }
        let plan = build_set_start_at_sign_in_plan(sign_in, request.enabled);
        let operation_count = plan.operations.len();
        for operation in &plan.operations {
            self.environment
                .apply_startup_registration_operation(operation)
                .map_err(|_| {
                    self.error_after_effect_failure(DesktopIntegrationErrorKind::EffectFailed)
                })?;
        }
        self.completed_mutation(
            if request.enabled {
                DesktopIntegrationMutationKind::EnableSignIn
            } else {
                DesktopIntegrationMutationKind::DisableSignIn
            },
            operation_count,
        )
    }

    fn take_revision(
        &self,
        revision: DesktopIntegrationRevision,
    ) -> Result<[u8; 32], DesktopIntegrationError> {
        self.revisions
            .take(revision.0, RevisionScope::DesktopIntegration)
            .ok_or_else(|| {
                DesktopIntegrationError::new(DesktopIntegrationErrorKind::InvalidRevision)
            })
    }

    fn current_snapshot(
        &self,
        expected: [u8; 32],
    ) -> Result<DesktopIntegrationEnvironmentSnapshot, DesktopIntegrationError> {
        let snapshot = self
            .environment
            .inspect_desktop_integration()
            .map_err(|_| {
                DesktopIntegrationError::new(DesktopIntegrationErrorKind::InspectFailed)
            })?;
        if desktop_integration_fingerprint(&snapshot) != expected {
            return Err(self.error_with_snapshot(DesktopIntegrationErrorKind::Conflict, &snapshot));
        }
        Ok(snapshot)
    }

    fn completed_mutation(
        &self,
        kind: DesktopIntegrationMutationKind,
        applied_operation_count: usize,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        Ok(DesktopIntegrationMutation {
            kind,
            applied_operation_count,
            workspace: self.inspect()?,
        })
    }

    fn error_after_effect_failure(
        &self,
        kind: DesktopIntegrationErrorKind,
    ) -> DesktopIntegrationError {
        match self.environment.inspect_desktop_integration() {
            Ok(snapshot) => self.error_with_snapshot(kind, &snapshot),
            Err(_) => DesktopIntegrationError::new(kind),
        }
    }

    fn error_with_snapshot(
        &self,
        kind: DesktopIntegrationErrorKind,
        snapshot: &DesktopIntegrationEnvironmentSnapshot,
    ) -> DesktopIntegrationError {
        DesktopIntegrationError::new(kind)
            .with_refreshed_workspace(self.workspace_from_snapshot(snapshot))
    }

    fn workspace_from_snapshot(
        &self,
        snapshot: &DesktopIntegrationEnvironmentSnapshot,
    ) -> DesktopIntegrationWorkspace {
        let fingerprint = desktop_integration_fingerprint(snapshot);
        let revision = DesktopIntegrationRevision(
            self.revisions
                .issue(RevisionScope::DesktopIntegration, fingerprint),
        );
        match snapshot {
            DesktopIntegrationEnvironmentSnapshot::Windows { repair, sign_in } => {
                let repair = assess_windows_desktop_integration(repair);
                let sign_in = inspect_start_at_sign_in(sign_in);
                DesktopIntegrationWorkspace {
                    revision,
                    platform: DesktopIntegrationPlatform::Windows,
                    repair_health: repair.health,
                    repair_items: repair.items,
                    sign_in: Some(StartAtSignInStatus {
                        health: sign_in.health,
                        effective_enabled: sign_in.effective_enabled,
                    }),
                }
            }
            DesktopIntegrationEnvironmentSnapshot::Macos { repair } => {
                let repair = assess_macos_desktop_integration(repair);
                DesktopIntegrationWorkspace {
                    revision,
                    platform: DesktopIntegrationPlatform::Macos,
                    repair_health: repair.health,
                    repair_items: repair.items,
                    sign_in: None,
                }
            }
            DesktopIntegrationEnvironmentSnapshot::Unsupported => DesktopIntegrationWorkspace {
                revision,
                platform: DesktopIntegrationPlatform::Unsupported,
                repair_health: DesktopIntegrationHealth::Unavailable,
                repair_items: Vec::new(),
                sign_in: None,
            },
        }
    }
}

pub trait DesktopIntegrationSource: Send + Sync + 'static {
    fn inspect(&self) -> Result<DesktopIntegrationWorkspace, DesktopIntegrationError>;
    fn repair(
        &self,
        request: RepairDesktopIntegration,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError>;
    fn migrate_sign_in(
        &self,
        request: MigrateStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError>;
    fn set_start_at_sign_in(
        &self,
        request: SetStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError>;
}

impl<E: DesktopIntegrationEnvironment> DesktopIntegrationSource for DesktopIntegrationService<E> {
    fn inspect(&self) -> Result<DesktopIntegrationWorkspace, DesktopIntegrationError> {
        DesktopIntegrationService::inspect(self)
    }

    fn repair(
        &self,
        request: RepairDesktopIntegration,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        DesktopIntegrationService::repair(self, request)
    }

    fn migrate_sign_in(
        &self,
        request: MigrateStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        DesktopIntegrationService::migrate_sign_in(self, request)
    }

    fn set_start_at_sign_in(
        &self,
        request: SetStartAtSignIn,
    ) -> Result<DesktopIntegrationMutation, DesktopIntegrationError> {
        DesktopIntegrationService::set_start_at_sign_in(self, request)
    }
}

fn assess_repair(snapshot: &DesktopIntegrationEnvironmentSnapshot) -> DesktopIntegrationAssessment {
    match snapshot {
        DesktopIntegrationEnvironmentSnapshot::Windows { repair, .. } => {
            assess_windows_desktop_integration(repair)
        }
        DesktopIntegrationEnvironmentSnapshot::Macos { repair } => {
            assess_macos_desktop_integration(repair)
        }
        DesktopIntegrationEnvironmentSnapshot::Unsupported => DesktopIntegrationAssessment {
            health: DesktopIntegrationHealth::Unavailable,
            items: Vec::new(),
            plan: None,
        },
    }
}

fn desktop_integration_fingerprint(snapshot: &DesktopIntegrationEnvironmentSnapshot) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DESKTOP_INTEGRATION_FINGERPRINT_DOMAIN);
    hasher.update([0]);
    match snapshot {
        DesktopIntegrationEnvironmentSnapshot::Windows { repair, sign_in } => {
            hasher.update([1]);
            hash_windows_snapshot(&mut hasher, repair);
            hash_startup_snapshot(&mut hasher, sign_in);
        }
        DesktopIntegrationEnvironmentSnapshot::Macos { repair } => {
            hasher.update([2]);
            hash_path(&mut hasher, &repair.current_exe);
            hash_optional_bytes(&mut hasher, repair.info_plist.as_deref());
            hasher.update([u8::from(repair.registered)]);
        }
        DesktopIntegrationEnvironmentSnapshot::Unsupported => hasher.update([3]),
    }
    hasher.finalize().into()
}

fn hash_windows_snapshot(hasher: &mut Sha256, snapshot: &WindowsDesktopSnapshot) {
    hash_path(hasher, &snapshot.current_exe);
    hasher.update([u8::from(snapshot.launcher_is_file)]);
    hash_optional_path(hasher, snapshot.desktop_dir.as_deref());
    hash_optional_path(hasher, snapshot.programs_dir.as_deref());
    hash_optional_shortcut(hasher, snapshot.desktop_manager.as_ref());
    hash_optional_shortcut(hasher, snapshot.start_menu_launcher.as_ref());
    hash_optional_shortcut(hasher, snapshot.start_menu_manager.as_ref());
    hash_optional_bytes(
        hasher,
        snapshot.protocol_command.as_deref().map(str::as_bytes),
    );
}

fn hash_startup_snapshot(hasher: &mut Sha256, snapshot: &StartupRegistrationSnapshot) {
    hash_path(hasher, &snapshot.launcher_path);
    hasher.update([u8::from(snapshot.launcher_is_file)]);
    hash_owned_string(hasher, &snapshot.canonical_run);
    hash_owned_string(hasher, &snapshot.legacy_run);
    hash_optional_shortcut(hasher, snapshot.legacy_shortcut.as_ref());
}

fn hash_owned_string(hasher: &mut Sha256, value: &OwnedStringValueSnapshot) {
    match value {
        OwnedStringValueSnapshot::Absent => hasher.update([0]),
        OwnedStringValueSnapshot::String(value) => {
            hasher.update([1]);
            hash_bytes(hasher, value.as_bytes());
        }
        OwnedStringValueSnapshot::UnsupportedType => hasher.update([2]),
    }
}

fn hash_optional_shortcut(hasher: &mut Sha256, value: Option<&ShortcutSnapshot>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hash_path(hasher, &value.target);
            hash_bytes(hasher, value.arguments.as_bytes());
        }
        None => hasher.update([0]),
    }
}

fn hash_optional_path(hasher: &mut Sha256, value: Option<&Path>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hash_path(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn hash_optional_bytes(hasher: &mut Sha256, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hash_bytes(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn hash_path(hasher: &mut Sha256, value: &Path) {
    hash_bytes(hasher, value.to_string_lossy().as_bytes());
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}
