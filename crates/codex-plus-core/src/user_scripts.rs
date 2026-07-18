use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::script_market::{MarketScript, PreparedMarketScript};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserScriptConfig {
    pub enabled: bool,
    pub scripts: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub market: BTreeMap<String, MarketScriptInstall>,
}

impl Default for UserScriptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scripts: BTreeMap::new(),
            market: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarketScriptInstall {
    pub id: String,
    pub name: String,
    pub version: String,
    pub script_url: String,
    pub homepage: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserScriptOrigin {
    Builtin,
    User,
}

impl UserScriptOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserScriptStatus {
    Disabled,
    NotLoaded,
}

impl UserScriptStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NotLoaded => "not_loaded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserScriptRecord {
    pub key: String,
    pub name: String,
    pub source: UserScriptOrigin,
    pub enabled: bool,
    pub status: UserScriptStatus,
    pub market: Option<MarketScriptInstall>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptInventory {
    pub enabled: bool,
    pub builtin_dir: PathBuf,
    pub user_dir: PathBuf,
    pub scripts: Vec<UserScriptRecord>,
}

impl fmt::Debug for UserScriptInventory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptInventory")
            .field("enabled", &self.enabled)
            .field("script_count", &self.scripts.len())
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserScriptRevision([u8; 32]);

impl UserScriptRevision {
    pub fn digest(&self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for UserScriptRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserScriptRevision([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptInspection {
    pub revision: UserScriptRevision,
    pub inventory: UserScriptInventory,
}

impl fmt::Debug for UserScriptInspection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptInspection")
            .field("revision", &self.revision)
            .field("inventory", &self.inventory)
            .finish()
    }
}

#[derive(Clone)]
pub struct UserScriptManager {
    builtin_dir: PathBuf,
    user_dir: PathBuf,
    config_path: PathBuf,
    backup_root: PathBuf,
    config_lock: Arc<Mutex<()>>,
}

impl fmt::Debug for UserScriptManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserScriptManager([paths redacted])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserScriptMutationErrorKind {
    InspectFailed,
    Conflict,
    InvalidTarget,
    BackupFailed,
    WriteFailed,
    RollbackFailed,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptMutationError {
    kind: UserScriptMutationErrorKind,
    rollback_verified: bool,
}

impl UserScriptMutationError {
    fn new(kind: UserScriptMutationErrorKind) -> Self {
        Self {
            kind,
            rollback_verified: false,
        }
    }

    fn write_failed(rollback_verified: bool) -> Self {
        Self {
            kind: UserScriptMutationErrorKind::WriteFailed,
            rollback_verified,
        }
    }

    pub fn kind(&self) -> UserScriptMutationErrorKind {
        self.kind
    }

    pub fn rollback_verified(&self) -> bool {
        self.rollback_verified
    }
}

impl fmt::Debug for UserScriptMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptMutationError")
            .field("kind", &self.kind)
            .field("rollback_verified", &self.rollback_verified)
            .finish()
    }
}

impl fmt::Display for UserScriptMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            UserScriptMutationErrorKind::InspectFailed => "user script inspection failed",
            UserScriptMutationErrorKind::Conflict => "user script state changed",
            UserScriptMutationErrorKind::InvalidTarget => "invalid user script target",
            UserScriptMutationErrorKind::BackupFailed => "user script backup failed",
            UserScriptMutationErrorKind::WriteFailed => "user script write failed",
            UserScriptMutationErrorKind::RollbackFailed => "user script rollback failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for UserScriptMutationError {}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptBackupEvidence {
    pub id: String,
    pub created: bool,
}

impl fmt::Debug for UserScriptBackupEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptBackupEvidence")
            .field("id", &self.id)
            .field("created", &self.created)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserScriptMutationOutcome {
    pub inspection: UserScriptInspection,
    pub backup: UserScriptBackupEvidence,
}

impl fmt::Debug for UserScriptMutationOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserScriptMutationOutcome")
            .field("inspection", &self.inspection)
            .field("backup", &self.backup)
            .finish()
    }
}

struct BackupRecord {
    evidence: UserScriptBackupEvidence,
    script_path: Option<PathBuf>,
}

impl UserScriptManager {
    pub fn new(
        builtin_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        config_path: impl Into<PathBuf>,
    ) -> Self {
        let config_path = config_path.into();
        let backup_root = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups");
        Self {
            builtin_dir: builtin_dir.into(),
            user_dir: user_dir.into(),
            config_path,
            backup_root,
            config_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn with_backup_root(mut self, backup_root: impl Into<PathBuf>) -> Self {
        self.backup_root = backup_root.into();
        self
    }

    pub fn load_config(&self) -> UserScriptConfig {
        let _guard = self.config_lock.lock().unwrap();
        self.load_config_unlocked()
    }

    fn load_config_unlocked(&self) -> UserScriptConfig {
        config_from_object(&self.load_raw_config_unlocked())
    }

    fn load_raw_config_unlocked(&self) -> Map<String, Value> {
        let Ok(bytes) = fs::read(&self.config_path) else {
            return Map::new();
        };
        let Ok(Value::Object(raw)) = serde_json::from_slice::<Value>(&bytes) else {
            return Map::new();
        };
        raw
    }

    pub fn save_config(&self, config: &UserScriptConfig) -> anyhow::Result<()> {
        self.with_transaction(|| self.save_config_unlocked(config))
    }

    fn save_config_unlocked(&self, config: &UserScriptConfig) -> anyhow::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create user script config directory {}",
                    parent.display()
                )
            })?;
        }
        crate::settings::atomic_write(
            &self.config_path,
            serde_json::to_string_pretty(config)?.as_bytes(),
        )
    }

    fn save_raw_config_unlocked(&self, raw: &Map<String, Value>) -> anyhow::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create user script config directory {}",
                    parent.display()
                )
            })?;
        }
        crate::settings::atomic_write(
            &self.config_path,
            serde_json::to_string_pretty(&Value::Object(raw.clone()))?.as_bytes(),
        )
    }

    fn with_transaction<T>(
        &self,
        operation: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let (_process_guard, _coordination_guard) = self.acquire_transaction()?;
        operation()
    }

    fn acquire_transaction(
        &self,
    ) -> anyhow::Result<(
        std::sync::MutexGuard<'_, ()>,
        crate::coordination_lock::CoordinationLock,
    )> {
        let process_guard = self
            .config_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("user script transaction lock is poisoned"))?;
        let lock_path = crate::coordination_lock::sidecar_path(&self.config_path);
        let coordination_guard = crate::coordination_lock::acquire_exclusive(&lock_path)?;
        Ok((process_guard, coordination_guard))
    }

    pub fn set_global_enabled(&self, enabled: bool) -> anyhow::Result<UserScriptConfig> {
        self.with_transaction(|| {
            let mut raw = self.load_raw_config_unlocked();
            raw.insert("enabled".to_string(), Value::Bool(enabled));
            self.save_raw_config_unlocked(&raw)?;
            Ok(config_from_object(&raw))
        })
    }

    pub fn set_script_enabled(&self, key: &str, enabled: bool) -> anyhow::Result<UserScriptConfig> {
        self.with_transaction(|| {
            let mut raw = self.load_raw_config_unlocked();
            normalize_enabled_field(&mut raw);
            object_field_mut(&mut raw, "scripts").insert(key.to_string(), Value::Bool(enabled));
            self.save_raw_config_unlocked(&raw)?;
            Ok(config_from_object(&raw))
        })
    }

    pub fn set_global_enabled_if_revision(
        &self,
        expected_revision: &UserScriptRevision,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptMutationError> {
        self.mutate_config_if_revision(expected_revision, |raw| {
            raw.insert("enabled".to_string(), Value::Bool(enabled));
        })
    }

    pub fn set_script_enabled_if_revision(
        &self,
        expected_revision: &UserScriptRevision,
        key: &str,
        enabled: bool,
    ) -> Result<UserScriptMutationOutcome, UserScriptMutationError> {
        self.mutate_config_if_revision(expected_revision, |raw| {
            normalize_enabled_field(raw);
            object_field_mut(raw, "scripts").insert(key.to_string(), Value::Bool(enabled));
        })
    }

    fn mutate_config_if_revision(
        &self,
        expected_revision: &UserScriptRevision,
        mutation: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<UserScriptMutationOutcome, UserScriptMutationError> {
        let (_process_guard, _coordination_guard) = self
            .acquire_transaction()
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        let current = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        if current.revision != *expected_revision {
            return Err(UserScriptMutationError::new(
                UserScriptMutationErrorKind::Conflict,
            ));
        }

        let mut raw = self.load_raw_config_unlocked();
        mutation(&mut raw);
        self.save_raw_config_unlocked(&raw)
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        let inspection = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        Ok(UserScriptMutationOutcome {
            inspection,
            backup: UserScriptBackupEvidence {
                id: String::new(),
                created: false,
            },
        })
    }

    pub fn delete_user_script(&self, key: &str) -> anyhow::Result<UserScriptConfig> {
        if !key.starts_with("user:") {
            anyhow::bail!("only user scripts can be deleted");
        }
        validated_user_script_file_name(key).map_err(anyhow::Error::new)?;
        let inspection = self.inspect()?;
        self.delete_user_script_with_backup(&inspection.revision, key)
            .map_err(anyhow::Error::new)?;
        Ok(self.load_config())
    }

    pub fn user_script_path_for_market_id(&self, id: &str) -> PathBuf {
        self.user_dir.join(market_script_filename(id))
    }

    pub fn commit_market_script(
        &self,
        expected_revision: &UserScriptRevision,
        prepared: &PreparedMarketScript,
    ) -> Result<UserScriptMutationOutcome, UserScriptMutationError> {
        let (_process_guard, _coordination_guard) = self
            .acquire_transaction()
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        let current = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        if current.revision != *expected_revision {
            return Err(UserScriptMutationError::new(
                UserScriptMutationErrorKind::Conflict,
            ));
        }

        let target = self.user_script_path_for_market_id(&prepared.script.id);
        fs::create_dir_all(&self.user_dir)
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        let key = format!(
            "user:{}",
            target
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default()
        );
        let mut raw = self.load_raw_config_unlocked();
        let target_existed = target.is_file();
        let backup = self
            .backup_existing_unlocked(&target, &key, &raw, "install")
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::BackupFailed))?;

        crate::settings::atomic_write(&target, &prepared.content)
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        apply_market_install_to_raw(&mut raw, &key, &prepared.script);
        if self.save_raw_config_unlocked(&raw).is_err() {
            let rollback_verified = rollback_script(&target, target_existed, &backup);
            return if rollback_verified {
                Err(UserScriptMutationError::write_failed(true))
            } else {
                Err(UserScriptMutationError::new(
                    UserScriptMutationErrorKind::RollbackFailed,
                ))
            };
        }

        let inspection = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        Ok(UserScriptMutationOutcome {
            inspection,
            backup: backup.evidence,
        })
    }

    pub fn delete_user_script_with_backup(
        &self,
        expected_revision: &UserScriptRevision,
        key: &str,
    ) -> Result<UserScriptMutationOutcome, UserScriptMutationError> {
        let file_name = validated_user_script_file_name(key)?;
        let (_process_guard, _coordination_guard) = self
            .acquire_transaction()
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed))?;
        let current = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        if current.revision != *expected_revision {
            return Err(UserScriptMutationError::new(
                UserScriptMutationErrorKind::Conflict,
            ));
        }

        let target = self.user_dir.join(file_name);
        if target.exists() {
            let canonical_user_dir = self.resolve_user_dir().map_err(|_| {
                UserScriptMutationError::new(UserScriptMutationErrorKind::InvalidTarget)
            })?;
            let canonical_target = target.canonicalize().map_err(|_| {
                UserScriptMutationError::new(UserScriptMutationErrorKind::InvalidTarget)
            })?;
            if !canonical_target.starts_with(&canonical_user_dir) {
                return Err(UserScriptMutationError::new(
                    UserScriptMutationErrorKind::InvalidTarget,
                ));
            }
        }

        let mut raw = self.load_raw_config_unlocked();
        let target_existed = target.is_file();
        let backup = self
            .backup_existing_unlocked(&target, key, &raw, "delete")
            .map_err(|_| UserScriptMutationError::new(UserScriptMutationErrorKind::BackupFailed))?;
        if target_existed {
            fs::remove_file(&target).map_err(|_| {
                UserScriptMutationError::new(UserScriptMutationErrorKind::WriteFailed)
            })?;
        }
        normalize_enabled_field(&mut raw);
        remove_object_entry(&mut raw, "scripts", key, false);
        remove_object_entry(&mut raw, "market", key, true);
        if self.save_raw_config_unlocked(&raw).is_err() {
            let rollback_verified = rollback_script(&target, target_existed, &backup);
            return if rollback_verified {
                Err(UserScriptMutationError::write_failed(true))
            } else {
                Err(UserScriptMutationError::new(
                    UserScriptMutationErrorKind::RollbackFailed,
                ))
            };
        }

        let inspection = self.inspect_unlocked().map_err(|_| {
            UserScriptMutationError::new(UserScriptMutationErrorKind::InspectFailed)
        })?;
        Ok(UserScriptMutationOutcome {
            inspection,
            backup: backup.evidence,
        })
    }

    fn backup_existing_unlocked(
        &self,
        target: &Path,
        key: &str,
        raw: &Map<String, Value>,
        operation: &str,
    ) -> anyhow::Result<BackupRecord> {
        let script_choice = object_entry(raw, "scripts", key).cloned();
        let market_entry = object_entry(raw, "market", key).cloned();
        if !target.is_file() && script_choice.is_none() && market_entry.is_none() {
            return Ok(BackupRecord {
                evidence: UserScriptBackupEvidence {
                    id: String::new(),
                    created: false,
                },
                script_path: None,
            });
        }

        let id = uuid::Uuid::new_v4().simple().to_string();
        let directory = self.backup_root.join("user-scripts").join(&id);
        fs::create_dir_all(&directory).with_context(|| {
            format!(
                "failed to create user script backup directory {}",
                directory.display()
            )
        })?;
        let script_path = if target.is_file() {
            let backup_script = directory.join("script.js");
            fs::copy(target, &backup_script)
                .with_context(|| format!("failed to back up user script {}", target.display()))?;
            Some(backup_script)
        } else {
            None
        };
        let metadata = json!({
            "schema": 1,
            "operation": operation,
            "key": key,
            "file_name": target.file_name().map(|name| name.to_string_lossy()).unwrap_or_default(),
            "script_choice": script_choice,
            "market_entry": market_entry
        });
        crate::settings::atomic_write(
            &directory.join("metadata.json"),
            serde_json::to_string_pretty(&metadata)?.as_bytes(),
        )?;
        Ok(BackupRecord {
            evidence: UserScriptBackupEvidence { id, created: true },
            script_path,
        })
    }

    pub fn record_market_install(&self, script: &MarketScript) -> anyhow::Result<UserScriptConfig> {
        self.with_transaction(|| self.record_market_install_unlocked(script))
    }

    fn record_market_install_unlocked(
        &self,
        script: &MarketScript,
    ) -> anyhow::Result<UserScriptConfig> {
        let mut raw = self.load_raw_config_unlocked();
        normalize_enabled_field(&mut raw);
        let key = format!("user:{}", market_script_filename(&script.id));
        object_field_mut(&mut raw, "scripts")
            .entry(key.clone())
            .or_insert(Value::Bool(true));

        let market = object_field_mut(&mut raw, "market");
        let mut install = market
            .remove(&key)
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        insert_string(&mut install, "id", &script.id);
        insert_string(&mut install, "name", &script.name);
        insert_string(&mut install, "version", &script.version);
        insert_string(&mut install, "script_url", &script.script_url);
        insert_string(&mut install, "homepage", &script.homepage);
        insert_string(
            &mut install,
            "installed_at",
            &current_unix_timestamp_string(),
        );
        market.insert(key, Value::Object(install));
        self.save_raw_config_unlocked(&raw)?;
        Ok(config_from_object(&raw))
    }

    pub fn typed_inventory(&self) -> anyhow::Result<UserScriptInventory> {
        let config = self.load_config();
        self.typed_inventory_for_config(&config)
    }

    pub fn inspect(&self) -> anyhow::Result<UserScriptInspection> {
        self.with_transaction(|| self.inspect_unlocked())
    }

    fn inspect_unlocked(&self) -> anyhow::Result<UserScriptInspection> {
        let raw_config = fs::read(&self.config_path).unwrap_or_default();
        let raw = serde_json::from_slice::<Value>(&raw_config)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let config = config_from_object(&raw);
        let files = self.scan_script_files(&config)?;
        let revision = revision_for(&raw_config, &files)?;
        let inventory = self.typed_inventory_from_files(&config, &files);
        Ok(UserScriptInspection {
            revision,
            inventory,
        })
    }

    pub fn inventory(&self) -> anyhow::Result<Value> {
        let inventory = self.typed_inventory()?;
        Ok(json!({
            "enabled": inventory.enabled,
            "builtin_dir": inventory.builtin_dir.to_string_lossy(),
            "user_dir": inventory.user_dir.to_string_lossy(),
            "scripts": inventory.scripts.iter().map(legacy_script_value).collect::<Vec<_>>()
        }))
    }

    pub fn build_enabled_bundle(&self) -> anyhow::Result<String> {
        let config = self.load_config();
        if !config.enabled {
            return Ok(String::new());
        }
        let mut blocks = Vec::new();
        for script in self.scan_script_files(&config)? {
            if !script.enabled {
                continue;
            }
            let source = fs::read_to_string(&script.path)
                .unwrap_or_else(|error| format!("throw new Error({});", json!(error.to_string())));
            blocks.push(wrap_script(&script, &source));
        }
        Ok(blocks.join("\n"))
    }

    fn typed_inventory_for_config(
        &self,
        config: &UserScriptConfig,
    ) -> anyhow::Result<UserScriptInventory> {
        let files = self.scan_script_files(config)?;
        Ok(self.typed_inventory_from_files(config, &files))
    }

    fn typed_inventory_from_files(
        &self,
        config: &UserScriptConfig,
        files: &[UserScriptFile],
    ) -> UserScriptInventory {
        let scripts = files
            .iter()
            .map(|script| UserScriptRecord {
                key: script.key.clone(),
                name: script.name.clone(),
                source: script.source,
                enabled: script.enabled,
                status: if !config.enabled || !script.enabled {
                    UserScriptStatus::Disabled
                } else {
                    UserScriptStatus::NotLoaded
                },
                market: config.market.get(&script.key).cloned(),
            })
            .collect();
        UserScriptInventory {
            enabled: config.enabled,
            builtin_dir: self.builtin_dir.clone(),
            user_dir: self.user_dir.clone(),
            scripts,
        }
    }

    fn scan_script_files(&self, config: &UserScriptConfig) -> anyhow::Result<Vec<UserScriptFile>> {
        fs::create_dir_all(&self.user_dir).with_context(|| {
            format!(
                "failed to create user scripts directory {}",
                self.user_dir.display()
            )
        })?;
        let mut scripts = Vec::new();
        self.append_scripts(
            UserScriptOrigin::Builtin,
            &self.builtin_dir,
            config,
            &mut scripts,
        )?;
        self.append_scripts(UserScriptOrigin::User, &self.user_dir, config, &mut scripts)?;
        Ok(scripts)
    }

    fn append_scripts(
        &self,
        source: UserScriptOrigin,
        directory: &Path,
        config: &UserScriptConfig,
        scripts: &mut Vec<UserScriptFile>,
    ) -> anyhow::Result<()> {
        let Ok(entries) = fs::read_dir(directory) else {
            return Ok(());
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("js"))
            .collect::<Vec<_>>();
        paths.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });

        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let key = format!("{}:{name}", source.as_str());
            scripts.push(UserScriptFile {
                enabled: config.scripts.get(&key).copied().unwrap_or(true),
                key,
                name,
                source,
                path,
            });
        }
        Ok(())
    }

    fn resolve_user_dir(&self) -> anyhow::Result<PathBuf> {
        self.user_dir
            .canonicalize()
            .or_else(|_| {
                fs::create_dir_all(&self.user_dir)?;
                self.user_dir.canonicalize()
            })
            .with_context(|| {
                format!(
                    "failed to resolve user script directory {}",
                    self.user_dir.display()
                )
            })
    }
}

#[derive(Debug)]
struct UserScriptFile {
    key: String,
    name: String,
    source: UserScriptOrigin,
    path: PathBuf,
    enabled: bool,
}

fn wrap_script(script: &UserScriptFile, source: &str) -> String {
    format!(
        r#"
(() => {{
  window.__codexPlusUserScripts = window.__codexPlusUserScripts || {{ scripts: {{}} }};
  const key = {key};
  window.__codexPlusUserScripts.scripts[key] = {{ key, name: {name}, source: {source_name}, status: "loading", error: "", loadedAt: new Date().toISOString() }};
  try {{
{source}
    window.__codexPlusUserScripts.scripts[key].status = "loaded";
    window.__codexPlusUserScripts.scripts[key].loadedAt = new Date().toISOString();
  }} catch (error) {{
    window.__codexPlusUserScripts.scripts[key].status = "failed";
    window.__codexPlusUserScripts.scripts[key].error = String(error && (error.stack || error.message) || error);
  }}
}})();
"#,
        key = json!(script.key),
        name = json!(script.name),
        source_name = json!(script.source),
        source = source
    )
}

fn legacy_script_value(script: &UserScriptRecord) -> Value {
    let market = script.market.as_ref();
    json!({
        "key": script.key,
        "name": script.name,
        "source": script.source.as_str(),
        "enabled": script.enabled,
        "status": script.status.as_str(),
        "error": "",
        "market_id": market.map(|item| item.id.as_str()).unwrap_or(""),
        "version": market.map(|item| item.version.as_str()).unwrap_or(""),
        "installed": market.is_some(),
        "source_url": market.map(|item| item.script_url.as_str()).unwrap_or(""),
        "homepage": market.map(|item| item.homepage.as_str()).unwrap_or("")
    })
}

fn validated_user_script_file_name(key: &str) -> Result<&str, UserScriptMutationError> {
    let Some(file_name) = key.strip_prefix("user:").filter(|value| !value.is_empty()) else {
        return Err(UserScriptMutationError::new(
            UserScriptMutationErrorKind::InvalidTarget,
        ));
    };
    if file_name.contains(['/', '\\']) || file_name == "." || file_name == ".." {
        return Err(UserScriptMutationError::new(
            UserScriptMutationErrorKind::InvalidTarget,
        ));
    }
    Ok(file_name)
}

fn object_entry<'a>(raw: &'a Map<String, Value>, field: &str, key: &str) -> Option<&'a Value> {
    raw.get(field)
        .and_then(Value::as_object)
        .and_then(|items| items.get(key))
}

fn apply_market_install_to_raw(raw: &mut Map<String, Value>, key: &str, script: &MarketScript) {
    normalize_enabled_field(raw);
    object_field_mut(raw, "scripts")
        .entry(key.to_string())
        .or_insert(Value::Bool(true));

    let market = object_field_mut(raw, "market");
    let mut install = market
        .remove(key)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    insert_string(&mut install, "id", &script.id);
    insert_string(&mut install, "name", &script.name);
    insert_string(&mut install, "version", &script.version);
    insert_string(&mut install, "script_url", &script.script_url);
    insert_string(&mut install, "homepage", &script.homepage);
    insert_string(
        &mut install,
        "installed_at",
        &current_unix_timestamp_string(),
    );
    market.insert(key.to_string(), Value::Object(install));
}

fn rollback_script(target: &Path, target_existed: bool, backup: &BackupRecord) -> bool {
    if target_existed {
        let Some(backup_path) = backup.script_path.as_ref() else {
            return false;
        };
        if fs::copy(backup_path, target).is_err() {
            return false;
        }
        files_equal(backup_path, target).unwrap_or(false)
    } else if target.exists() {
        fs::remove_file(target).is_ok() && !target.exists()
    } else {
        true
    }
}

fn files_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_metadata = fs::metadata(left)?;
    let right_metadata = fs::metadata(right)?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }

    let mut left_file = fs::File::open(left)?;
    let mut right_file = fs::File::open(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left_file.read(&mut left_buffer)?;
        let right_read = right_file.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn normalize_enabled_field(raw: &mut Map<String, Value>) {
    if !raw.get("enabled").is_some_and(Value::is_boolean) {
        raw.insert("enabled".to_string(), Value::Bool(true));
    }
}

fn object_field_mut<'a>(raw: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let value = raw
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("object field is initialized above")
}

fn remove_object_entry(
    raw: &mut Map<String, Value>,
    field: &str,
    key: &str,
    remove_empty_field: bool,
) {
    let empty = raw
        .get_mut(field)
        .and_then(Value::as_object_mut)
        .map(|items| {
            items.remove(key);
            items.is_empty()
        })
        .unwrap_or(false);
    if remove_empty_field && empty {
        raw.remove(field);
    }
}

fn insert_string(raw: &mut Map<String, Value>, key: &str, value: &str) {
    raw.insert(key.to_string(), Value::String(value.to_string()));
}

fn revision_for(raw_config: &[u8], files: &[UserScriptFile]) -> anyhow::Result<UserScriptRevision> {
    let mut digest = Sha256::new();
    digest.update(b"codex-plus-user-scripts-v1\0");
    hash_bytes(&mut digest, raw_config);

    let mut buffer = [0_u8; 64 * 1024];
    for script in files {
        hash_bytes(&mut digest, script.key.as_bytes());
        hash_bytes(&mut digest, script.source.as_str().as_bytes());
        let metadata = fs::metadata(&script.path).with_context(|| {
            format!(
                "failed to inspect user script metadata {}",
                script.path.display()
            )
        })?;
        digest.update(metadata.len().to_le_bytes());
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok());
        digest.update(
            modified
                .map(|value| value.as_secs())
                .unwrap_or(0)
                .to_le_bytes(),
        );
        digest.update(
            modified
                .map(|value| value.subsec_nanos())
                .unwrap_or(0)
                .to_le_bytes(),
        );

        let mut file = fs::File::open(&script.path)
            .with_context(|| format!("failed to read user script {}", script.path.display()))?;
        loop {
            let read = file
                .read(&mut buffer)
                .with_context(|| format!("failed to hash user script {}", script.path.display()))?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
    }

    Ok(UserScriptRevision(digest.finalize().into()))
}

fn hash_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn config_from_object(raw: &Map<String, Value>) -> UserScriptConfig {
    let enabled = raw.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let scripts = raw
        .get("scripts")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_bool()?)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let market = raw
        .get("market")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), market_install_from_value(value)?)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    UserScriptConfig {
        enabled,
        scripts,
        market,
    }
}

pub fn market_script_filename(id: &str) -> String {
    let sanitized = sanitize_market_id(id);
    format!(
        "market-{}.js",
        if sanitized.is_empty() {
            "script".to_string()
        } else {
            sanitized
        }
    )
}

pub fn default_user_scripts_config_dir() -> PathBuf {
    if cfg!(windows)
        && let Some(roaming) = std::env::var_os("APPDATA")
    {
        return PathBuf::from(roaming).join("Codex++");
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("Codex++")
}

fn market_install_from_value(value: &Value) -> Option<MarketScriptInstall> {
    let raw = value.as_object()?;
    Some(MarketScriptInstall {
        id: string_field(raw, "id")?,
        name: string_field(raw, "name").unwrap_or_default(),
        version: string_field(raw, "version")?,
        script_url: string_field(raw, "script_url")?,
        homepage: string_field(raw, "homepage").unwrap_or_default(),
        installed_at: string_field(raw, "installed_at").unwrap_or_default(),
    })
}

fn string_field(raw: &Map<String, Value>, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sanitize_market_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn current_unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
