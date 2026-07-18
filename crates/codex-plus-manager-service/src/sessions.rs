use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codex_plus_core::models::{DeleteResult, DeleteStatus, SessionRef};
use codex_plus_data::LocalSession;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionRevision([u8; 32]);

impl SessionRevision {
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for SessionRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionRevision(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionErrorKind {
    LoadFailed,
    Conflict,
    ConfirmationMismatch,
    DeleteFailed,
}

pub struct SessionError {
    kind: SessionErrorKind,
    compatibility_detail: Option<String>,
}

impl SessionError {
    pub fn new(kind: SessionErrorKind) -> Self {
        Self {
            kind,
            compatibility_detail: None,
        }
    }

    pub fn with_compatibility_detail(kind: SessionErrorKind, detail: String) -> Self {
        Self {
            kind,
            compatibility_detail: Some(detail),
        }
    }

    pub fn kind(&self) -> SessionErrorKind {
        self.kind
    }

    pub fn compatibility_detail(&self) -> Option<&str> {
        self.compatibility_detail.as_deref()
    }

    pub fn detail(&self) -> &'static str {
        match self.kind {
            SessionErrorKind::LoadFailed => "session workspace load failed",
            SessionErrorKind::Conflict => "session metadata changed on disk",
            SessionErrorKind::ConfirmationMismatch => "session confirmation does not match",
            SessionErrorKind::DeleteFailed => "session deletion failed",
        }
    }
}

impl fmt::Debug for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

impl std::error::Error for SessionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionReadIssueKind {
    OpenFailed,
    UnsupportedSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReadIssue {
    pub database_id: String,
    pub kind: SessionReadIssueKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model_provider: String,
    pub archived: bool,
    pub updated_at_ms: Option<i64>,
    pub source_db_paths: Vec<String>,
    pub revision: SessionRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionWorkspace {
    pub db_paths: Vec<String>,
    pub sessions: Vec<SessionSummary>,
    pub read_issues: Vec<SessionReadIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteSessionSelection {
    pub id: String,
    pub expected_revision: SessionRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteSessions {
    pub selections: Vec<DeleteSessionSelection>,
    pub confirmed_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CompatibilityUndoToken(String);

impl CompatibilityUndoToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CompatibilityUndoToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CompatibilityUndoToken(..)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionDeleteOutcome {
    pub session_id: String,
    pub status: DeleteStatus,
    pub backup_path: Option<String>,
    compatibility_message: String,
    compatibility_undo_token: Option<CompatibilityUndoToken>,
}

impl SessionDeleteOutcome {
    pub fn compatibility_message(&self) -> &str {
        &self.compatibility_message
    }

    pub fn compatibility_undo_token(&self) -> Option<&str> {
        self.compatibility_undo_token
            .as_ref()
            .map(CompatibilityUndoToken::as_str)
    }

    pub fn compatibility_delete_result(&self) -> DeleteResult {
        DeleteResult {
            status: self.status.clone(),
            session_id: self.session_id.clone(),
            message: self.compatibility_message.clone(),
            undo_token: self
                .compatibility_undo_token
                .as_ref()
                .map(|token| token.0.clone()),
            backup_path: self.backup_path.clone(),
        }
    }
}

impl fmt::Debug for SessionDeleteOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionDeleteOutcome")
            .field("session_id", &self.session_id)
            .field("status", &self.status)
            .field("has_backup", &self.backup_path.is_some())
            .field(
                "has_compatibility_undo",
                &self.compatibility_undo_token.is_some(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDeleteBatchOutcome {
    pub outcomes: Vec<SessionDeleteOutcome>,
    pub workspace: SessionWorkspace,
}

pub type SessionLoadResult = Result<Arc<SessionWorkspace>, SessionError>;
pub type SessionDeleteResult = Result<Arc<SessionDeleteBatchOutcome>, SessionError>;

pub trait SessionEnvironment: Send + Sync + 'static {
    fn session_db_paths(&self) -> Vec<PathBuf>;
    fn list_local_sessions(&self, db_path: &Path) -> anyhow::Result<Vec<LocalSession>>;
    fn delete_local_from_paths(&self, db_paths: Vec<PathBuf>, session: &SessionRef)
    -> DeleteResult;
}

pub trait SessionSource: Send + Sync + 'static {
    fn load_workspace(&self) -> SessionLoadResult;
    fn delete_sessions(&self, request: DeleteSessions) -> SessionDeleteResult;
}

#[derive(Clone)]
pub struct SessionService<E> {
    environment: E,
}

impl<E> SessionService<E> {
    pub fn new(environment: E) -> Self {
        Self { environment }
    }
}

impl<E: SessionEnvironment> SessionService<E> {
    fn load_workspace_inner(&self) -> SessionLoadResult {
        let db_paths = self.environment.session_db_paths();
        let database_ids = db_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut grouped = BTreeMap::<String, Vec<ObservedSession>>::new();
        let mut read_issues = Vec::new();

        for (db_path, database_id) in db_paths.iter().zip(&database_ids) {
            match self.environment.list_local_sessions(db_path) {
                Ok(rows) => {
                    for mut row in rows {
                        let id = normalize_session_id(&row.id);
                        if id.is_empty() {
                            continue;
                        }
                        row.id = id.clone();
                        grouped.entry(id).or_default().push(ObservedSession {
                            database_id: database_id.clone(),
                            row,
                        });
                    }
                }
                Err(error) => read_issues.push(SessionReadIssue {
                    database_id: database_id.clone(),
                    kind: classify_read_issue(&error),
                }),
            }
        }

        let mut sessions = grouped
            .into_values()
            .filter_map(session_summary)
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            right
                .updated_at_ms
                .unwrap_or(i64::MIN)
                .cmp(&left.updated_at_ms.unwrap_or(i64::MIN))
                .then_with(|| right.id.cmp(&left.id))
        });

        Ok(Arc::new(SessionWorkspace {
            db_paths: database_ids,
            sessions,
            read_issues,
        }))
    }
}

impl<E: SessionEnvironment> SessionSource for SessionService<E> {
    fn load_workspace(&self) -> SessionLoadResult {
        self.load_workspace_inner()
    }

    fn delete_sessions(&self, request: DeleteSessions) -> SessionDeleteResult {
        validate_confirmation(&request)?;
        let current = self.load_workspace_inner()?;
        let by_id = current
            .sessions
            .iter()
            .map(|session| (session.id.as_str(), session))
            .collect::<BTreeMap<_, _>>();

        for selection in &request.selections {
            let id = normalize_session_id(&selection.id);
            let Some(current_session) = by_id.get(id.as_str()) else {
                return Err(SessionError::new(SessionErrorKind::Conflict));
            };
            if current_session.revision != selection.expected_revision {
                return Err(SessionError::new(SessionErrorKind::Conflict));
            }
        }

        let db_paths = current
            .db_paths
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        let mut outcomes = Vec::with_capacity(request.selections.len());
        for selection in request.selections {
            let current_session = by_id
                .get(normalize_session_id(&selection.id).as_str())
                .expect("selected session was preflighted");
            let result = self.environment.delete_local_from_paths(
                db_paths.clone(),
                &SessionRef {
                    session_id: current_session.id.clone(),
                    title: current_session.title.clone(),
                },
            );
            outcomes.push(SessionDeleteOutcome {
                session_id: result.session_id,
                status: result.status,
                backup_path: result.backup_path,
                compatibility_message: result.message,
                compatibility_undo_token: result.undo_token.map(CompatibilityUndoToken),
            });
        }

        let workspace = Arc::unwrap_or_clone(self.load_workspace_inner()?);
        Ok(Arc::new(SessionDeleteBatchOutcome {
            outcomes,
            workspace,
        }))
    }
}

#[derive(Clone)]
struct ObservedSession {
    database_id: String,
    row: LocalSession,
}

fn session_summary(mut observed: Vec<ObservedSession>) -> Option<SessionSummary> {
    observed.sort_by(|left, right| {
        right
            .row
            .updated_at_ms
            .unwrap_or(i64::MIN)
            .cmp(&left.row.updated_at_ms.unwrap_or(i64::MIN))
            .then_with(|| right.row.id.cmp(&left.row.id))
            .then_with(|| left.database_id.cmp(&right.database_id))
    });
    let display = observed.first()?.row.clone();
    let source_db_paths = observed
        .iter()
        .map(|record| record.database_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let revision = revision_for_records(&display.id, &observed);
    Some(SessionSummary {
        id: display.id,
        title: display.title,
        cwd: display.cwd,
        model_provider: display.model_provider,
        archived: display.archived,
        updated_at_ms: display.updated_at_ms,
        source_db_paths,
        revision,
    })
}

fn revision_for_records(id: &str, observed: &[ObservedSession]) -> SessionRevision {
    let mut records = observed.to_vec();
    records.sort_by(|left, right| {
        left.database_id
            .cmp(&right.database_id)
            .then_with(|| left.row.updated_at_ms.cmp(&right.row.updated_at_ms))
            .then_with(|| left.row.archived.cmp(&right.row.archived))
            .then_with(|| left.row.model_provider.cmp(&right.row.model_provider))
            .then_with(|| left.row.rollout_path.cmp(&right.row.rollout_path))
    });
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, id.as_bytes());
    for record in records {
        hash_field(&mut hasher, record.database_id.as_bytes());
        match record.row.updated_at_ms {
            Some(value) => {
                hash_field(&mut hasher, &[1]);
                hash_field(&mut hasher, &value.to_le_bytes());
            }
            None => hash_field(&mut hasher, &[0]),
        }
        hash_field(&mut hasher, &[u8::from(record.row.archived)]);
        hash_field(&mut hasher, record.row.model_provider.as_bytes());
        hash_field(&mut hasher, record.row.rollout_path.as_bytes());
    }
    SessionRevision(hasher.finalize().into())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn normalize_session_id(value: &str) -> String {
    value.trim().to_owned()
}

fn classify_read_issue(error: &anyhow::Error) -> SessionReadIssueKind {
    if format!("{error:#}")
        .to_ascii_lowercase()
        .contains("unsupported")
    {
        SessionReadIssueKind::UnsupportedSchema
    } else {
        SessionReadIssueKind::OpenFailed
    }
}

fn validate_confirmation(request: &DeleteSessions) -> Result<(), SessionError> {
    let selected = request
        .selections
        .iter()
        .map(|selection| normalize_session_id(&selection.id))
        .collect::<Vec<_>>();
    let confirmed = request
        .confirmed_ids
        .iter()
        .map(|id| normalize_session_id(id))
        .collect::<Vec<_>>();
    if selected.is_empty()
        || selected.iter().any(String::is_empty)
        || confirmed.iter().any(String::is_empty)
        || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
        || confirmed.iter().collect::<BTreeSet<_>>().len() != confirmed.len()
        || selected.iter().collect::<BTreeSet<_>>() != confirmed.iter().collect::<BTreeSet<_>>()
    {
        return Err(SessionError::new(SessionErrorKind::ConfirmationMismatch));
    }
    Ok(())
}
