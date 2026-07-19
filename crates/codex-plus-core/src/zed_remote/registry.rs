use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    SshTarget, ZedRemoteError, build_zed_remote_url, codex_global_state_path,
    fallback_open_request_from_global_state_with_context, resolve_ssh_target_from_global_state,
    target_from_payload,
};

const REGISTRY_FILE: &str = "zed_remote_projects.json";

#[derive(Clone, PartialEq, Eq)]
pub struct ZedRemoteRegistryRevision([u8; 32]);

impl ZedRemoteRegistryRevision {
    /// Construct an opaque revision for callers that need to retain a test or
    /// persisted coordination token without inspecting its bytes.
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Debug for ZedRemoteRegistryRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ZedRemoteRegistryRevision([redacted])")
    }
}

#[derive(Clone)]
pub struct ZedRemoteRegistrySnapshot {
    pub revision: ZedRemoteRegistryRevision,
    pub projects: Vec<ZedRemoteProject>,
}

impl fmt::Debug for ZedRemoteRegistrySnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteRegistrySnapshot")
            .field("revision", &self.revision)
            .field("project_count", &self.projects.len())
            .finish()
    }
}

#[derive(Clone)]
pub struct ZedRemoteRegistryMutation {
    pub snapshot: ZedRemoteRegistrySnapshot,
    pub affected: usize,
}

impl fmt::Debug for ZedRemoteRegistryMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteRegistryMutation")
            .field("snapshot", &self.snapshot)
            .field("affected", &self.affected)
            .finish()
    }
}

#[derive(Clone)]
pub struct ZedRemoteRegistryStore {
    path: PathBuf,
}

impl fmt::Debug for ZedRemoteRegistryStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteRegistryStore")
            .finish_non_exhaustive()
    }
}

impl ZedRemoteRegistryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn inspect(&self) -> Result<ZedRemoteRegistrySnapshot, ZedRemoteError> {
        let data = read_registry_bytes(&self.path)?;
        let document = parse_registry_document(data.as_deref())?;
        Ok(ZedRemoteRegistrySnapshot {
            revision: registry_revision(data.as_deref()),
            projects: document.projects,
        })
    }

    pub fn remember_if_revision(
        &self,
        expected: &ZedRemoteRegistryRevision,
        mut project: ZedRemoteProject,
    ) -> Result<ZedRemoteRegistryMutation, ZedRemoteError> {
        project.source = ZedRemoteProjectSource::Recent;
        project.is_current = false;
        let (affected, snapshot) = self.mutate_if_revision(expected, move |projects| {
            push_project(projects, project);
            projects.sort_by(|left, right| {
                right
                    .last_opened_at_ms
                    .unwrap_or_default()
                    .cmp(&left.last_opened_at_ms.unwrap_or_default())
            });
            projects.truncate(100);
            Ok(1)
        })?;
        Ok(ZedRemoteRegistryMutation { snapshot, affected })
    }

    pub fn forget_if_revision(
        &self,
        expected: &ZedRemoteRegistryRevision,
        project_id: &str,
    ) -> Result<ZedRemoteRegistryMutation, ZedRemoteError> {
        let project_id = project_id.to_string();
        let (affected, snapshot) = self.mutate_if_revision(expected, move |projects| {
            let before = projects.len();
            projects.retain(|project| project.id != project_id);
            Ok(before.saturating_sub(projects.len()))
        })?;
        Ok(ZedRemoteRegistryMutation { snapshot, affected })
    }

    fn mutate_if_revision<T>(
        &self,
        expected: &ZedRemoteRegistryRevision,
        mutation: impl FnOnce(&mut Vec<ZedRemoteProject>) -> Result<T, ZedRemoteError>,
    ) -> Result<(T, ZedRemoteRegistrySnapshot), ZedRemoteError> {
        let lock_path = crate::coordination_lock::sidecar_path(&self.path);
        let _lock = crate::coordination_lock::acquire_exclusive(&lock_path)
            .map_err(ZedRemoteError::RegistryLock)?;
        let data = read_registry_bytes(&self.path)?;
        if registry_revision(data.as_deref()) != *expected {
            return Err(ZedRemoteError::RegistryConflict);
        }

        let mut document = parse_registry_document(data.as_deref())?;
        let result = mutation(&mut document.projects)?;
        document.root.insert(
            "projects".to_string(),
            serde_json::to_value(&document.projects).map_err(ZedRemoteError::RegistryParse)?,
        );
        let bytes = serde_json::to_vec_pretty(&Value::Object(document.root))
            .map_err(ZedRemoteError::RegistryParse)?;
        crate::settings::atomic_write(&self.path, &bytes)
            .map_err(ZedRemoteError::RegistryCommit)?;
        let snapshot = self.inspect()?;
        Ok((result, snapshot))
    }
}

struct ZedRemoteRegistryDocument {
    root: Map<String, Value>,
    projects: Vec<ZedRemoteProject>,
}

fn read_registry_bytes(path: &Path) -> Result<Option<Vec<u8>>, ZedRemoteError> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ZedRemoteError::RegistryRead(error)),
    }
}

fn parse_registry_document(
    data: Option<&[u8]>,
) -> Result<ZedRemoteRegistryDocument, ZedRemoteError> {
    let Some(data) = data else {
        return Ok(ZedRemoteRegistryDocument {
            root: Map::new(),
            projects: Vec::new(),
        });
    };
    if data.iter().all(u8::is_ascii_whitespace) {
        return Ok(ZedRemoteRegistryDocument {
            root: Map::new(),
            projects: Vec::new(),
        });
    }
    if let Ok(projects) = serde_json::from_slice::<Vec<ZedRemoteProject>>(data) {
        return Ok(ZedRemoteRegistryDocument {
            root: Map::new(),
            projects,
        });
    }

    let root = serde_json::from_slice::<Map<String, Value>>(data)
        .map_err(ZedRemoteError::RegistryParse)?;
    let projects = root
        .get("projects")
        .cloned()
        .map(serde_json::from_value::<Vec<ZedRemoteProject>>)
        .transpose()
        .map_err(ZedRemoteError::RegistryParse)?
        .unwrap_or_default();
    Ok(ZedRemoteRegistryDocument { root, projects })
}

fn registry_revision(data: Option<&[u8]>) -> ZedRemoteRegistryRevision {
    let mut hasher = Sha256::new();
    match data {
        Some(data) => {
            hasher.update(b"present\0");
            hasher.update(data);
        }
        None => hasher.update(b"missing\0"),
    }
    ZedRemoteRegistryRevision(hasher.finalize().into())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZedRemoteProject {
    pub id: String,
    pub label: String,
    pub host_id: String,
    pub ssh: SshTarget,
    pub path: String,
    pub url: String,
    pub source: ZedRemoteProjectSource,
    pub last_opened_at_ms: Option<i64>,
    pub is_current: bool,
}

impl fmt::Debug for ZedRemoteProject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZedRemoteProject")
            .field("source", &self.source)
            .field("is_current", &self.is_current)
            .field("port_present", &self.ssh.port.is_some())
            .field("last_opened_present", &self.last_opened_at_ms.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZedRemoteProjectSource {
    CurrentThread,
    CodexRemoteProject,
    ThreadWorkspaceHint,
    SqliteThreadCwd,
    Recent,
}

pub fn list_zed_remote_projects_response(payload: &Value) -> Value {
    let state = match fs::read_to_string(codex_global_state_path()) {
        Ok(data) => match serde_json::from_str::<Value>(&data) {
            Ok(state) => Some(state),
            Err(error) => {
                return json!({"status": "failed", "message": ZedRemoteError::StateParse(error).to_string()});
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return json!({"status": "failed", "message": ZedRemoteError::StateRead(error).to_string()});
        }
    };
    let registry = ZedRemoteRegistryStore::new(default_zed_remote_project_registry_path());
    let result = registry.inspect().and_then(|snapshot| {
        list_zed_remote_projects_from_sources(
            state.as_ref(),
            payload,
            &snapshot.projects,
            Some(&crate::codex_sqlite::codex_session_db_path()),
        )
    });
    match result {
        Ok(projects) => json!({
            "status": "ok",
            "projects": projects,
        }),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

pub fn list_zed_remote_projects_from_state(
    state: &Value,
    payload: &Value,
    registry_path: Option<&Path>,
    sqlite_state_path: Option<&Path>,
) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    let registry_path = registry_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_zed_remote_project_registry_path);
    let snapshot = ZedRemoteRegistryStore::new(registry_path).inspect()?;
    list_zed_remote_projects_from_sources(
        Some(state),
        payload,
        &snapshot.projects,
        sqlite_state_path,
    )
}

pub fn list_zed_remote_projects_from_sources(
    state: Option<&Value>,
    payload: &Value,
    registry_projects: &[ZedRemoteProject],
    sqlite_state_path: Option<&Path>,
) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    let sqlite_paths = sqlite_state_path
        .map(|path| vec![path.to_path_buf()])
        .unwrap_or_else(|| {
            crate::codex_sqlite::codex_session_db_paths_from_home(
                &crate::codex_sqlite::default_codex_home_dir(),
            )
        });
    list_zed_remote_projects_from_sources_with_sqlite_paths(
        state,
        payload,
        registry_projects,
        &sqlite_paths,
    )
}

pub fn list_zed_remote_projects_from_sources_with_sqlite_paths(
    state: Option<&Value>,
    payload: &Value,
    registry_projects: &[ZedRemoteProject],
    sqlite_paths: &[PathBuf],
) -> Result<Vec<ZedRemoteProject>, ZedRemoteError> {
    let mut projects = Vec::new();
    if let Some(state) = state {
        collect_current_project(state, payload, &mut projects);
        collect_codex_remote_projects(state, &mut projects);
        collect_thread_workspace_hints(state, &mut projects);
        collect_sqlite_thread_cwds(state, sqlite_paths, &mut projects);
    }
    for mut project in registry_projects.iter().cloned() {
        project.source = ZedRemoteProjectSource::Recent;
        project.is_current = false;
        push_project(&mut projects, project);
    }
    Ok(projects)
}

pub fn remember_zed_remote_project_response(payload: &Value) -> Value {
    match remember_zed_remote_project(payload, None, None) {
        Ok(project) => json!({"status": "ok", "project": project}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

pub(super) fn remember_zed_remote_project(
    payload: &Value,
    resolved_target: Option<&SshTarget>,
    resolved_url: Option<&str>,
) -> Result<ZedRemoteProject, ZedRemoteError> {
    let target = resolved_target
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| target_from_payload(payload))?;
    let path = string_value(payload.get("path"));
    let url = resolved_url
        .map(ToString::to_string)
        .map(Ok)
        .unwrap_or_else(|| build_zed_remote_url(&target, &path))?;
    let host_id = string_value(payload.get("hostId"));
    let label = string_value(payload.get("label"));
    let project = project_from_parts(
        &host_id,
        target,
        &path,
        &url,
        label,
        ZedRemoteProjectSource::Recent,
        Some(now_ms()),
    );
    let store = ZedRemoteRegistryStore::new(default_zed_remote_project_registry_path());
    let snapshot = store.inspect()?;
    store.remember_if_revision(&snapshot.revision, project.clone())?;
    Ok(project)
}

pub fn forget_zed_remote_project_response(payload: &Value) -> Value {
    match forget_zed_remote_project(payload) {
        Ok(removed) => json!({"status": "ok", "removed": removed}),
        Err(error) => json!({"status": "failed", "message": error.to_string()}),
    }
}

fn forget_zed_remote_project(payload: &Value) -> Result<usize, ZedRemoteError> {
    let explicit_id = string_value(payload.get("id"));
    let target_id = if explicit_id.is_empty() {
        let target = target_from_payload(payload)?;
        let path = string_value(payload.get("path"));
        project_id(&target, &path)
    } else {
        explicit_id
    };
    let store = ZedRemoteRegistryStore::new(default_zed_remote_project_registry_path());
    let snapshot = store.inspect()?;
    store
        .forget_if_revision(&snapshot.revision, &target_id)
        .map(|mutation| mutation.affected)
}

fn collect_current_project(state: &Value, payload: &Value, projects: &mut Vec<ZedRemoteProject>) {
    let host_id = string_value(payload.get("hostId"));
    let thread_id = string_value(payload.get("threadId"))
        .or_else_nonempty(|| string_value(payload.get("sessionId")))
        .or_else_nonempty(|| string_value(payload.get("session_id")));
    let workspace_root = string_value(payload.get("remoteWorkspaceRoot"))
        .or_else_nonempty(|| string_value(payload.get("workspaceRoot")))
        .or_else_nonempty(|| string_value(payload.get("cwd")))
        .or_else_nonempty(|| string_value(payload.get("path")));
    let remote_project_id = string_value(payload.get("remoteProjectId"))
        .or_else_nonempty(|| string_value(payload.get("projectId")));
    if host_id.is_empty()
        && thread_id.is_empty()
        && workspace_root.is_empty()
        && remote_project_id.is_empty()
        && string_value(state.get("selected-remote-host-id")).is_empty()
    {
        return;
    }
    let Ok(request) = fallback_open_request_from_global_state_with_context(
        state,
        &host_id,
        &thread_id,
        &workspace_root,
        &remote_project_id,
    ) else {
        return;
    };
    let Ok(target) = target_from_payload(&request) else {
        return;
    };
    let path = string_value(request.get("path"));
    let Ok(url) = build_zed_remote_url(&target, &path) else {
        return;
    };
    let project = project_from_parts(
        &string_value(request.get("hostId")),
        target,
        &path,
        &url,
        String::new(),
        ZedRemoteProjectSource::CurrentThread,
        None,
    );
    push_project(projects, project);
}

fn collect_codex_remote_projects(state: &Value, projects: &mut Vec<ZedRemoteProject>) {
    for project in ordered_remote_projects_from_global_state(state) {
        let Some(object) = project.as_object() else {
            continue;
        };
        let host_id = string_value(object.get("hostId"));
        let path = string_value(object.get("remotePath"));
        if !path.starts_with('/') || host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &path) else {
            continue;
        };
        let label =
            string_value(object.get("label")).or_else_nonempty(|| string_value(object.get("name")));
        let project = project_from_parts(
            &host_id,
            target,
            &path,
            &url,
            label,
            ZedRemoteProjectSource::CodexRemoteProject,
            None,
        );
        push_project(projects, project);
    }
}

fn collect_thread_workspace_hints(state: &Value, projects: &mut Vec<ZedRemoteProject>) {
    let Some(hints) = state
        .get("thread-workspace-root-hints")
        .and_then(Value::as_object)
    else {
        return;
    };
    for hint in hints.values() {
        let path = workspace_path_from_hint(Some(hint));
        if !path.starts_with('/') {
            continue;
        }
        let hinted_host_id = host_id_from_hint(Some(hint));
        let host_id = host_id_for_remote_path(state, &hinted_host_id, &path);
        if host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &path) else {
            continue;
        };
        let project = project_from_parts(
            &host_id,
            target,
            &path,
            &url,
            String::new(),
            ZedRemoteProjectSource::ThreadWorkspaceHint,
            None,
        );
        push_project(projects, project);
    }
}

fn collect_sqlite_thread_cwds(
    state: &Value,
    paths: &[PathBuf],
    projects: &mut Vec<ZedRemoteProject>,
) {
    let mut cwds = Vec::new();
    for path in paths {
        if let Ok(mut items) = sqlite_thread_cwds(path) {
            cwds.append(&mut items);
        }
    }
    for cwd in cwds {
        if !cwd.starts_with('/') {
            continue;
        }
        let host_id = host_id_for_remote_path(state, "", &cwd);
        if host_id.is_empty() {
            continue;
        }
        let Ok(target) = resolve_ssh_target_from_global_state(state, &host_id) else {
            continue;
        };
        let Ok(url) = build_zed_remote_url(&target, &cwd) else {
            continue;
        };
        let project = project_from_parts(
            &host_id,
            target,
            &cwd,
            &url,
            String::new(),
            ZedRemoteProjectSource::SqliteThreadCwd,
            None,
        );
        push_project(projects, project);
    }
}

fn sqlite_thread_cwds(path: &Path) -> anyhow::Result<Vec<String>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let db = Connection::open(path)?;
    let mut statement = db
        .prepare("SELECT DISTINCT cwd FROM threads WHERE cwd IS NOT NULL AND cwd != '' LIMIT 80")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut cwds = Vec::new();
    for cwd in rows.flatten() {
        let cwd = cwd.trim().to_string();
        if !cwd.is_empty() {
            cwds.push(cwd);
        }
    }
    Ok(cwds)
}

fn push_project(projects: &mut Vec<ZedRemoteProject>, project: ZedRemoteProject) {
    if let Some(existing) = projects
        .iter_mut()
        .find(|existing| existing.id == project.id)
    {
        if source_priority(project.source) < source_priority(existing.source) {
            existing.source = project.source;
            existing.label = project.label;
            existing.host_id = project.host_id;
        }
        existing.last_opened_at_ms = match (existing.last_opened_at_ms, project.last_opened_at_ms) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        existing.is_current |= project.is_current;
        return;
    }
    projects.push(project);
}

fn source_priority(source: ZedRemoteProjectSource) -> u8 {
    match source {
        ZedRemoteProjectSource::CurrentThread => 0,
        ZedRemoteProjectSource::CodexRemoteProject => 1,
        ZedRemoteProjectSource::ThreadWorkspaceHint => 2,
        ZedRemoteProjectSource::SqliteThreadCwd => 3,
        ZedRemoteProjectSource::Recent => 4,
    }
}

fn project_from_parts(
    host_id: &str,
    target: SshTarget,
    path: &str,
    url: &str,
    label: String,
    source: ZedRemoteProjectSource,
    last_opened_at_ms: Option<i64>,
) -> ZedRemoteProject {
    let label = label.or_else_nonempty(|| label_from_path(path));
    let is_current = source == ZedRemoteProjectSource::CurrentThread;
    ZedRemoteProject {
        id: project_id(&target, path),
        label,
        host_id: host_id.trim().to_string(),
        ssh: target,
        path: path.trim().to_string(),
        url: url.to_string(),
        source,
        last_opened_at_ms,
        is_current,
    }
}

fn project_id(target: &SshTarget, path: &str) -> String {
    let composite = format!(
        "{}|{}|{}|{}",
        target.user.trim(),
        target.host.trim(),
        target.port.map(|port| port.to_string()).unwrap_or_default(),
        path.trim()
    );
    format!("zed-remote-project:{:016x}", stable_hash(&composite))
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn label_from_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn ordered_remote_projects_from_global_state(state: &Value) -> Vec<Value> {
    let projects = state
        .get("remote-projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|project| project.as_object().is_some())
        .collect::<Vec<_>>();
    let project_order = state
        .get("project-order")
        .and_then(Value::as_array)
        .map(|order| {
            order
                .iter()
                .map(|item| string_value(Some(item)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut ordered = Vec::new();
    for project_id in project_order {
        if let Some(project) = projects
            .iter()
            .find(|project| string_value(project.get("id")) == project_id)
        {
            ordered.push(project.clone());
        }
    }
    let ordered_ids = ordered
        .iter()
        .map(|project| string_value(project.get("id")))
        .collect::<std::collections::HashSet<_>>();
    ordered.extend(
        projects
            .into_iter()
            .filter(|project| !ordered_ids.contains(&string_value(project.get("id")))),
    );
    ordered
}

fn workspace_path_from_hint(hint: Option<&Value>) -> String {
    match hint {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Object(object)) => {
            for key in [
                "remotePath",
                "remoteWorkspaceRoot",
                "workspaceRoot",
                "path",
                "cwd",
            ] {
                let value = string_value(object.get(key));
                if !value.is_empty() {
                    return value;
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn host_id_from_hint(hint: Option<&Value>) -> String {
    match hint.and_then(Value::as_object) {
        Some(object) => string_value(object.get("hostId"))
            .or_else_nonempty(|| string_value(object.get("remoteHostId"))),
        None => String::new(),
    }
}

fn project_path_matches(remote_path: &str, project_path: &str) -> bool {
    let project_path = project_path.trim_end_matches('/');
    !project_path.is_empty()
        && (remote_path == project_path
            || remote_path
                .strip_prefix(project_path)
                .is_some_and(|suffix| suffix.starts_with('/')))
}

fn host_id_for_remote_path(state: &Value, preferred_host_id: &str, remote_path: &str) -> String {
    if !preferred_host_id.is_empty() {
        return preferred_host_id.to_string();
    }
    ordered_remote_projects_from_global_state(state)
        .into_iter()
        .find_map(|project| {
            let project_path = string_value(project.get("remotePath"));
            if project_path_matches(remote_path, &project_path) {
                Some(string_value(project.get("hostId")))
            } else {
                None
            }
        })
        .or_else(|| string_value(state.get("selected-remote-host-id")).into_nonempty())
        .unwrap_or_default()
}

fn default_zed_remote_project_registry_path() -> PathBuf {
    crate::paths::default_app_state_dir().join(REGISTRY_FILE)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

trait NonEmptyStringExt {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String;

    fn into_nonempty(self) -> Option<String>;
}

impl NonEmptyStringExt for String {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String,
    {
        if self.is_empty() { fallback() } else { self }
    }

    fn into_nonempty(self) -> Option<String> {
        if self.is_empty() { None } else { Some(self) }
    }
}
