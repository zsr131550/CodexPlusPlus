use std::fmt;

use crate::provider::ProviderValidationIssue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    LoadFailed,
    SaveFailed,
    Conflict,
    Validation,
}

pub struct ProviderError {
    kind: ProviderErrorKind,
    validation_issues: Vec<ProviderValidationIssue>,
}

impl ProviderError {
    pub(crate) fn load_failed() -> Self {
        Self::new(ProviderErrorKind::LoadFailed)
    }

    pub(crate) fn save_failed() -> Self {
        Self::new(ProviderErrorKind::SaveFailed)
    }

    pub(crate) fn conflict() -> Self {
        Self::new(ProviderErrorKind::Conflict)
    }

    pub(crate) fn validation(issues: Vec<ProviderValidationIssue>) -> Self {
        Self {
            kind: ProviderErrorKind::Validation,
            validation_issues: issues,
        }
    }

    fn new(kind: ProviderErrorKind) -> Self {
        Self {
            kind,
            validation_issues: Vec::new(),
        }
    }

    pub fn kind(&self) -> ProviderErrorKind {
        self.kind
    }

    pub fn validation_issues(&self) -> &[ProviderValidationIssue] {
        &self.validation_issues
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            ProviderErrorKind::LoadFailed => "provider workspace load failed",
            ProviderErrorKind::SaveFailed => "provider workspace save failed",
            ProviderErrorKind::Conflict => "provider workspace changed on disk",
            ProviderErrorKind::Validation => "provider workspace validation failed",
        }
    }
}

impl fmt::Debug for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderError")
            .field("kind", &self.kind)
            .field("validation_issue_count", &self.validation_issues.len())
            .finish()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for ProviderError {}
