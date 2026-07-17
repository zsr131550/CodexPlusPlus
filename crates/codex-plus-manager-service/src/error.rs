#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewErrorKind {
    LoadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
pub struct OverviewError {
    kind: OverviewErrorKind,
    detail: String,
}

impl OverviewError {
    pub fn new(kind: OverviewErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> OverviewErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}
