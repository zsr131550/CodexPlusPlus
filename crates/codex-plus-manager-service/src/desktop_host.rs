use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;

use codex_plus_core::manager_instance::ManagerActivation;

const PROVIDER_IMPORT_SCHEME: &str = "codexplusplus://";
const SHOW_UPDATE_ARGUMENT: &str = "--show-update";

pub trait DesktopHostEnvironment {
    fn pending_import_path(&self) -> &Path;
}

impl DesktopHostEnvironment for crate::SystemProviderEnvironment {
    fn pending_import_path(&self) -> &Path {
        <Self as crate::ProviderImportEnvironment>::pending_import_path(self)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DesktopStartupArgs {
    original: Vec<OsString>,
}

impl DesktopStartupArgs {
    pub fn new(args: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            original: args.into_iter().collect(),
        }
    }

    pub fn original(&self) -> &[OsString] {
        &self.original
    }

    pub fn prepare<E>(&self, environment: &E) -> DesktopStartupPlan
    where
        E: DesktopHostEnvironment,
    {
        let mut actions = Vec::new();
        let mut issues = Vec::new();
        let mut recognized_count = 0;
        let mut unknown_count = 0;

        for (argument_index, argument) in self.original.iter().enumerate() {
            if argument == OsStr::new(SHOW_UPDATE_ARGUMENT) {
                recognized_count += 1;
                actions.push(ManagerActivation::ShowUpdate);
                continue;
            }

            let Some(argument) = argument.to_str() else {
                unknown_count += 1;
                continue;
            };
            if !argument.starts_with(PROVIDER_IMPORT_SCHEME) {
                unknown_count += 1;
                continue;
            }

            recognized_count += 1;
            let result = codex_plus_core::provider_import::request_from_url(argument)
                .map_err(|_| DesktopStartupIssueKind::InvalidProviderImport)
                .and_then(|request| {
                    codex_plus_core::provider_import::save_pending_provider_import_at(
                        environment.pending_import_path(),
                        &request,
                    )
                    .map_err(|_| DesktopStartupIssueKind::PersistFailed)
                });
            match result {
                Ok(()) => actions.push(ManagerActivation::ReloadPendingProviderImport),
                Err(kind) => issues.push(DesktopStartupIssue {
                    kind,
                    argument_index,
                }),
            }
        }

        if actions.is_empty() {
            actions.push(ManagerActivation::Show);
        }

        DesktopStartupPlan {
            actions,
            issues,
            recognized_count,
            unknown_count,
        }
    }
}

impl fmt::Debug for DesktopStartupArgs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopStartupArgs")
            .field("argument_count", &self.original.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopStartupIssueKind {
    InvalidProviderImport,
    PersistFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopStartupIssue {
    kind: DesktopStartupIssueKind,
    argument_index: usize,
}

impl DesktopStartupIssue {
    pub fn kind(&self) -> DesktopStartupIssueKind {
        self.kind
    }

    pub fn argument_index(&self) -> usize {
        self.argument_index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopStartupPlan {
    actions: Vec<ManagerActivation>,
    issues: Vec<DesktopStartupIssue>,
    recognized_count: usize,
    unknown_count: usize,
}

impl DesktopStartupPlan {
    pub fn actions(&self) -> &[ManagerActivation] {
        &self.actions
    }

    pub fn into_actions(self) -> Vec<ManagerActivation> {
        self.actions
    }

    pub fn issues(&self) -> &[DesktopStartupIssue] {
        &self.issues
    }

    pub fn recognized_count(&self) -> usize {
        self.recognized_count
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown_count
    }
}
