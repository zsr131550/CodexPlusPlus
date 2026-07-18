use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

const WINDOWS_USER_ENV_KEY: &str = "Environment";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflict {
    pub name: String,
    pub source: EnvConflictSource,
    pub value_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvConflictSource {
    Process,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflictRemoval {
    pub name: String,
    pub removed_process: bool,
    pub removed_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflictRemovalFailure {
    pub name: String,
    pub source: EnvConflictSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflictRemovalResult {
    pub removed: Vec<EnvConflictRemoval>,
    pub backup_path: Option<String>,
    pub failures: Vec<EnvConflictRemovalFailure>,
}

pub fn is_codex_env_conflict_name(name: &str) -> bool {
    let name = name.trim();
    name.starts_with("OPENAI_")
}

pub fn detected_env_conflicts_from_pairs<I, K, V>(
    pairs: I,
    source: EnvConflictSource,
) -> Vec<EnvConflict>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut conflicts = pairs
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.as_ref().trim();
            if !is_codex_env_conflict_name(name) {
                return None;
            }
            Some(EnvConflict {
                name: name.to_string(),
                source,
                value_present: !value.as_ref().trim().is_empty(),
            })
        })
        .collect::<Vec<_>>();
    conflicts.sort_by(|left, right| left.name.cmp(&right.name));
    conflicts.dedup_by(|left, right| left.name == right.name && left.source == right.source);
    conflicts
}

pub fn detect_env_conflicts() -> Vec<EnvConflict> {
    let mut conflicts = detect_process_env_conflicts();
    conflicts.extend(detect_user_env_conflicts());
    conflicts.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| source_order(left.source).cmp(&source_order(right.source)))
    });
    conflicts.dedup_by(|left, right| left.name == right.name && left.source == right.source);
    conflicts
}

pub fn detect_process_env_conflicts() -> Vec<EnvConflict> {
    detected_env_conflicts_from_pairs(std::env::vars(), EnvConflictSource::Process)
}

pub fn remove_env_conflicts(
    names: &[String],
    backup_dir: PathBuf,
) -> anyhow::Result<EnvConflictRemovalResult> {
    remove_env_conflicts_with_user_env(names, None, backup_dir, true)
        .map(|result| result.expect("unconditional environment removal must return a result"))
}

pub fn remove_process_env_conflicts_for_tests(
    names: &[String],
    backup_dir: PathBuf,
) -> anyhow::Result<EnvConflictRemovalResult> {
    remove_env_conflicts_with_user_env(names, None, backup_dir, false)
        .map(|result| result.expect("unconditional environment removal must return a result"))
}

pub fn remove_env_conflicts_if_unchanged(
    names: &[String],
    expected: &[EnvConflict],
    backup_dir: PathBuf,
) -> anyhow::Result<Option<EnvConflictRemovalResult>> {
    remove_env_conflicts_with_user_env(names, Some(expected), backup_dir, true)
}

pub fn remove_process_env_conflicts_if_unchanged_for_tests(
    names: &[String],
    expected: &[EnvConflict],
    backup_dir: PathBuf,
) -> anyhow::Result<Option<EnvConflictRemovalResult>> {
    remove_env_conflicts_with_user_env(names, Some(expected), backup_dir, false)
}

fn remove_env_conflicts_with_user_env(
    names: &[String],
    expected: Option<&[EnvConflict]>,
    backup_dir: PathBuf,
    remove_user_env: bool,
) -> anyhow::Result<Option<EnvConflictRemovalResult>> {
    let names = normalize_conflict_names(names);
    if names.is_empty() {
        return Ok(Some(EnvConflictRemovalResult {
            removed: Vec::new(),
            backup_path: None,
            failures: Vec::new(),
        }));
    }

    std::fs::create_dir_all(&backup_dir)?;
    let _lock =
        crate::coordination_lock::acquire_exclusive(&backup_dir.join(".env-conflicts.lock"))?;
    let detected = if remove_user_env {
        detect_env_conflicts()
    } else {
        detect_process_env_conflicts()
    };
    if expected.is_some_and(|expected| expected != detected) {
        return Ok(None);
    }
    let before = detected
        .into_iter()
        .filter(|conflict| names.iter().any(|name| name == &conflict.name))
        .collect::<Vec<_>>();
    let backup_path = create_backup_file(&backup_dir, &before)?;

    let mut removed = Vec::new();
    let mut failures = Vec::new();
    for name in names {
        let had_process = std::env::var_os(&name).is_some();
        unsafe {
            std::env::remove_var(&name);
        }
        let had_user = before
            .iter()
            .any(|conflict| conflict.name == name && conflict.source == EnvConflictSource::User);
        let removed_user = if remove_user_env && had_user {
            match remove_user_env_value(&name) {
                Ok(removed) => removed,
                Err(_) => {
                    failures.push(EnvConflictRemovalFailure {
                        name: name.clone(),
                        source: EnvConflictSource::User,
                    });
                    false
                }
            }
        } else {
            false
        };
        removed.push(EnvConflictRemoval {
            name,
            removed_process: had_process,
            removed_user,
        });
    }

    Ok(Some(EnvConflictRemovalResult {
        removed,
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        failures,
    }))
}

pub fn normalize_conflict_names(names: &[String]) -> Vec<String> {
    let mut names = names
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| is_codex_env_conflict_name(name))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn create_backup_file(
    backup_dir: &std::path::Path,
    conflicts: &[EnvConflict],
) -> anyhow::Result<PathBuf> {
    let contents = serde_json::to_vec_pretty(conflicts)?;
    let timestamp = timestamp_millis();
    for suffix in 0..=999u16 {
        let file_name = if suffix == 0 {
            format!("env-conflicts-{timestamp}.json")
        } else {
            format!("env-conflicts-{timestamp}-{suffix}.json")
        };
        let path = backup_dir.join(file_name);
        let mut file = match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = file.write_all(&contents).and_then(|()| file.flush()) {
            let _ = std::fs::remove_file(&path);
            return Err(error.into());
        }
        return Ok(path);
    }
    anyhow::bail!("无法创建唯一的环境变量备份文件")
}

fn source_order(source: EnvConflictSource) -> u8 {
    match source {
        EnvConflictSource::Process => 0,
        EnvConflictSource::User => 1,
    }
}

fn timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(windows)]
fn detect_user_env_conflicts() -> Vec<EnvConflict> {
    crate::windows_integration::read_current_user_string_values(WINDOWS_USER_ENV_KEY)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, value.unwrap_or_default()))
        .pipe(|pairs| detected_env_conflicts_from_pairs(pairs, EnvConflictSource::User))
}

#[cfg(not(windows))]
fn detect_user_env_conflicts() -> Vec<EnvConflict> {
    Vec::new()
}

#[cfg(windows)]
fn remove_user_env_value(name: &str) -> anyhow::Result<bool> {
    crate::windows_integration::delete_current_user_value(WINDOWS_USER_ENV_KEY, name)?;
    Ok(true)
}

#[cfg(not(windows))]
fn remove_user_env_value(_name: &str) -> anyhow::Result<bool> {
    Ok(false)
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn detects_openai_prefixed_conflicts_but_not_codex_home() {
        let conflicts = detected_env_conflicts_from_pairs(
            [
                ("OPENAI_API_KEY", "sk-test"),
                ("OPENAI_BASE_URL", "https://example.test/v1"),
                ("CODEX_HOME", "C:/Users/me/.codex"),
                ("CUSTOM_OPENAI_API_KEY", "sk-custom"),
            ],
            EnvConflictSource::Process,
        );

        assert_eq!(
            conflicts
                .iter()
                .map(|conflict| conflict.name.as_str())
                .collect::<Vec<_>>(),
            vec!["OPENAI_API_KEY", "OPENAI_BASE_URL"]
        );
    }

    #[test]
    fn removal_normalization_only_keeps_conflict_names() {
        assert_eq!(
            normalize_conflict_names(&[
                "CODEX_HOME".to_string(),
                "OPENAI_API_KEY".to_string(),
                " OPENAI_BASE_URL ".to_string(),
                "OPENAI_API_KEY".to_string(),
            ]),
            vec!["OPENAI_API_KEY", "OPENAI_BASE_URL"]
        );
    }

    #[test]
    fn environment_backup_names_do_not_collide() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let name = "OPENAI_CODEX_PLUS_BACKUP_COLLISION_TEST".to_owned();
        unsafe {
            std::env::set_var(&name, "first-secret");
        }
        let first = remove_process_env_conflicts_for_tests(
            std::slice::from_ref(&name),
            dir.path().to_path_buf(),
        )
        .unwrap();
        unsafe {
            std::env::set_var(&name, "second-secret");
        }
        let second = remove_process_env_conflicts_for_tests(
            std::slice::from_ref(&name),
            dir.path().to_path_buf(),
        )
        .unwrap();
        unsafe {
            std::env::remove_var(&name);
        }

        assert_ne!(first.backup_path, second.backup_path);
        assert!(
            first
                .backup_path
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).exists())
        );
        assert!(
            second
                .backup_path
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).exists())
        );
    }

    #[test]
    fn stale_environment_selection_writes_no_backup() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let selected = "OPENAI_CODEX_PLUS_STALE_SELECTED".to_owned();
        let changed = "OPENAI_CODEX_PLUS_STALE_CHANGED".to_owned();
        unsafe {
            std::env::set_var(&selected, "selected-secret");
        }
        let expected = detect_env_conflicts();
        unsafe {
            std::env::set_var(&changed, "changed-secret");
        }

        let result = remove_process_env_conflicts_if_unchanged_for_tests(
            std::slice::from_ref(&selected),
            &expected,
            dir.path().to_path_buf(),
        )
        .unwrap();

        assert!(result.is_none());
        assert!(std::env::var_os(&selected).is_some());
        let backups = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("env-conflicts-")
            })
            .count();
        assert_eq!(backups, 0);
        unsafe {
            std::env::remove_var(&selected);
            std::env::remove_var(&changed);
        }
    }
}
