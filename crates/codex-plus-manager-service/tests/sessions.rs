use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use codex_plus_core::models::{DeleteResult, DeleteStatus, SessionRef};
use codex_plus_data::LocalSession;
use codex_plus_manager_service::{
    DeleteSessionSelection, DeleteSessions, SessionEnvironment, SessionErrorKind, SessionService,
    SessionSource,
};

type RowStore = Arc<Mutex<HashMap<PathBuf, Result<Vec<LocalSession>, String>>>>;

#[derive(Clone, Default)]
struct FakeSessionEnvironment {
    db_paths: Arc<Vec<PathBuf>>,
    rows: RowStore,
    delete_results: Arc<Mutex<HashMap<String, VecDeque<DeleteResult>>>>,
    delete_calls: Arc<AtomicUsize>,
}

impl FakeSessionEnvironment {
    fn new(paths: &[&str]) -> Self {
        Self {
            db_paths: Arc::new(paths.iter().map(PathBuf::from).collect()),
            ..Self::default()
        }
    }

    fn set_rows(&self, path: &str, rows: Vec<LocalSession>) {
        self.rows
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), Ok(rows));
    }

    fn set_read_failure(&self, path: &str, detail: &str) {
        self.rows
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), Err(detail.to_owned()));
    }

    fn set_delete_results(&self, id: &str, results: Vec<DeleteResult>) {
        self.delete_results
            .lock()
            .unwrap()
            .insert(id.to_owned(), results.into());
    }

    fn replace_updated_at(&self, id: &str, updated_at_ms: i64) {
        for rows in self.rows.lock().unwrap().values_mut().flatten() {
            for row in rows.iter_mut().filter(|row| row.id == id) {
                row.updated_at_ms = Some(updated_at_ms);
            }
        }
    }

    fn delete_calls(&self) -> usize {
        self.delete_calls.load(Ordering::SeqCst)
    }
}

impl SessionEnvironment for FakeSessionEnvironment {
    fn session_db_paths(&self) -> Vec<PathBuf> {
        self.db_paths.as_ref().clone()
    }

    fn list_local_sessions(&self, db_path: &Path) -> anyhow::Result<Vec<LocalSession>> {
        match self.rows.lock().unwrap().get(db_path) {
            Some(Ok(rows)) => Ok(rows.clone()),
            Some(Err(detail)) => anyhow::bail!(detail.clone()),
            None => Ok(Vec::new()),
        }
    }

    fn delete_local_from_paths(
        &self,
        _db_paths: Vec<PathBuf>,
        session: &SessionRef,
    ) -> DeleteResult {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        let result = self
            .delete_results
            .lock()
            .unwrap()
            .get_mut(&session.session_id)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| failed(&session.session_id, "fixture delete missing"));
        if result.status == DeleteStatus::LocalDeleted {
            for rows in self.rows.lock().unwrap().values_mut().flatten() {
                rows.retain(|row| row.id != session.session_id);
            }
        }
        result
    }
}

fn local_session(db_path: &str, id: &str, updated_at_ms: i64, archived: bool) -> LocalSession {
    LocalSession {
        id: id.to_owned(),
        title: format!("Title {id}"),
        cwd: format!("C:/workspace/{id}"),
        model_provider: "openai".to_owned(),
        archived,
        updated_at_ms: Some(updated_at_ms),
        rollout_path: format!("C:/rollouts/{id}.jsonl"),
        db_path: db_path.to_owned(),
    }
}

fn deleted(id: &str, backup_path: &str) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::LocalDeleted,
        session_id: id.to_owned(),
        message: "deleted".to_owned(),
        undo_token: Some(format!("undo-{id}")),
        backup_path: Some(backup_path.to_owned()),
    }
}

fn failed(id: &str, message: &str) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::Failed,
        session_id: id.to_owned(),
        message: message.to_owned(),
        undo_token: None,
        backup_path: None,
    }
}

#[test]
fn session_workspace_merges_duplicate_ids_without_invalidating_unrelated_revisions() {
    let environment = FakeSessionEnvironment::new(&["db-a.sqlite", "db-b.sqlite", "bad.sqlite"]);
    environment.set_rows(
        "db-a.sqlite",
        vec![
            local_session("db-a.sqlite", "newest", 300, false),
            local_session("db-a.sqlite", "other", 200, false),
        ],
    );
    environment.set_rows(
        "db-b.sqlite",
        vec![
            local_session("db-b.sqlite", "newest", 250, false),
            local_session("db-b.sqlite", "archived", 100, true),
        ],
    );
    environment.set_read_failure("bad.sqlite", "unsupported schema");
    let service = SessionService::new(environment.clone());

    let workspace = service.load_workspace().unwrap();

    assert_eq!(workspace.sessions[0].id, "newest");
    assert_eq!(workspace.sessions[0].source_db_paths.len(), 2);
    assert_eq!(workspace.read_issues.len(), 1);
    assert_eq!(
        workspace
            .sessions
            .iter()
            .filter(|item| !item.archived)
            .count(),
        2
    );
    let revision = workspace.sessions[0].revision.clone();
    environment.replace_updated_at("other", 999);

    let refreshed = service.load_workspace().unwrap();
    let newest = refreshed
        .sessions
        .iter()
        .find(|session| session.id == "newest")
        .unwrap();
    assert_eq!(newest.revision, revision);
}

#[test]
fn stale_delete_is_rejected_before_the_first_mutation() {
    let environment = FakeSessionEnvironment::new(&["db.sqlite"]);
    environment.set_rows(
        "db.sqlite",
        vec![local_session("db.sqlite", "selected", 100, false)],
    );
    environment.set_delete_results("selected", vec![deleted("selected", "backup.json")]);
    let service = SessionService::new(environment.clone());
    let workspace = service.load_workspace().unwrap();
    let selected = workspace.sessions[0].clone();
    environment.replace_updated_at("selected", 101);

    let error = service
        .delete_sessions(DeleteSessions {
            selections: vec![DeleteSessionSelection {
                id: selected.id.clone(),
                expected_revision: selected.revision,
            }],
            confirmed_ids: vec![selected.id],
        })
        .unwrap_err();

    assert_eq!(error.kind(), SessionErrorKind::Conflict);
    assert_eq!(environment.delete_calls(), 0);
}

#[test]
fn batch_delete_returns_ordered_partial_outcomes_and_a_refreshed_workspace() {
    let environment = FakeSessionEnvironment::new(&["db.sqlite"]);
    environment.set_rows(
        "db.sqlite",
        vec![
            local_session("db.sqlite", "first", 300, false),
            local_session("db.sqlite", "second", 200, false),
            local_session("db.sqlite", "untouched", 100, false),
        ],
    );
    environment.set_delete_results("first", vec![deleted("first", "first-backup.json")]);
    environment.set_delete_results("second", vec![failed("second", "locked")]);
    let service = SessionService::new(environment);
    let workspace = service.load_workspace().unwrap();
    let selection = |id: &str| {
        let session = workspace
            .sessions
            .iter()
            .find(|session| session.id == id)
            .unwrap();
        DeleteSessionSelection {
            id: id.to_owned(),
            expected_revision: session.revision.clone(),
        }
    };

    let outcome = service
        .delete_sessions(DeleteSessions {
            selections: vec![selection("first"), selection("second")],
            confirmed_ids: vec!["first".to_owned(), "second".to_owned()],
        })
        .unwrap();

    assert_eq!(outcome.outcomes.len(), 2);
    assert_eq!(outcome.outcomes[0].session_id, "first");
    assert_eq!(outcome.outcomes[0].status, DeleteStatus::LocalDeleted);
    assert_eq!(
        outcome.outcomes[0].backup_path.as_deref(),
        Some("first-backup.json")
    );
    assert_eq!(outcome.outcomes[1].session_id, "second");
    assert_eq!(outcome.outcomes[1].status, DeleteStatus::Failed);
    assert!(
        outcome
            .workspace
            .sessions
            .iter()
            .all(|session| session.id != "first")
    );
    assert!(
        outcome
            .workspace
            .sessions
            .iter()
            .any(|session| session.id == "untouched")
    );
}
