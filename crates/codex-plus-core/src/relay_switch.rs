use std::fmt;
use std::path::Path;

use anyhow::Context;

use crate::relay_config::{
    RelayLiveFilesSnapshot, backfill_relay_profile_from_home_with_common, capture_relay_live_files,
    relay_config_status_from_home, restore_relay_live_files,
};
use crate::settings::{BackendSettings, RelayMode, SettingsStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySwitchResult {
    pub settings: BackendSettings,
    pub configured: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayRollbackOutcome {
    NotRequired,
    Verified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySwitchError {
    message: String,
    rollback_outcome: RelayRollbackOutcome,
    backup_path: Option<String>,
}

impl RelaySwitchError {
    pub fn rollback_outcome(&self) -> RelayRollbackOutcome {
        self.rollback_outcome
    }

    pub fn backup_path(&self) -> Option<&str> {
        self.backup_path.as_deref()
    }

    fn before_mutation(error: impl fmt::Display) -> Self {
        Self {
            message: error.to_string(),
            rollback_outcome: RelayRollbackOutcome::NotRequired,
            backup_path: None,
        }
    }

    fn after_mutation(
        error: impl fmt::Display,
        rollback_outcome: RelayRollbackOutcome,
        backup_path: Option<String>,
    ) -> Self {
        let mut message = error.to_string();
        if rollback_outcome == RelayRollbackOutcome::Failed {
            message.push_str("；自动回滚验证失败。");
        }
        Self {
            message,
            rollback_outcome,
            backup_path,
        }
    }
}

impl fmt::Display for RelaySwitchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RelaySwitchError {}

pub fn switch_relay_profile_in_home(
    store: &SettingsStore,
    home: &Path,
    next_settings: BackendSettings,
    previous_active_relay_id: &str,
) -> Result<RelaySwitchResult, RelaySwitchError> {
    let mut selected_settings = next_settings;
    if !selected_settings.relay_profiles_enabled {
        return Err(RelaySwitchError::before_mutation(
            "供应商配置总开关已关闭，未写入 config.toml / auth.json。",
        ));
    }

    let original_settings = store
        .load()
        .context("读取原供应商设置失败")
        .map_err(RelaySwitchError::before_mutation)?;
    let original_live =
        capture_relay_live_files(home).map_err(RelaySwitchError::before_mutation)?;
    if !previous_active_relay_id.trim().is_empty()
        && previous_active_relay_id != selected_settings.active_relay_id
    {
        backfill_profile_before_switch(home, &mut selected_settings, previous_active_relay_id)
            .map_err(RelaySwitchError::before_mutation)?;
    }

    store
        .save(&selected_settings)
        .context("保存供应商设置失败")
        .map_err(RelaySwitchError::before_mutation)?;
    let selected_settings = match store.load().context("读取供应商设置失败") {
        Ok(settings) => settings,
        Err(error) => {
            return Err(rollback_switch_failure(
                store,
                home,
                &original_settings,
                &original_live,
                error,
                None,
            ));
        }
    };

    match apply_selected_relay_profile(home, &selected_settings) {
        Ok(result)
            if selected_settings.active_relay_profile().relay_mode == RelayMode::PureApi
                && !result.configured =>
        {
            let backup_path = result.backup_path.clone();
            Err(rollback_switch_failure(
                store,
                home,
                &original_settings,
                &original_live,
                "纯 API 配置写入后未检测到完整 custom provider，请检查 config.toml 和供应商 API Key。",
                backup_path,
            ))
        }
        Ok(result) => Ok(result),
        Err(error) => Err(rollback_switch_failure(
            store,
            home,
            &original_settings,
            &original_live,
            error,
            None,
        )),
    }
}

fn rollback_switch_failure(
    store: &SettingsStore,
    home: &Path,
    original_settings: &BackendSettings,
    original_live: &RelayLiveFilesSnapshot,
    error: impl fmt::Display,
    backup_path: Option<String>,
) -> RelaySwitchError {
    let settings_restored = store.save(original_settings).is_ok();
    let live_restored = restore_relay_live_files(home, original_live).is_ok();
    let settings_verified = store
        .load()
        .is_ok_and(|settings| settings == *original_settings);
    let live_verified =
        capture_relay_live_files(home).is_ok_and(|snapshot| snapshot == *original_live);
    let rollback_outcome =
        if settings_restored && live_restored && settings_verified && live_verified {
            RelayRollbackOutcome::Verified
        } else {
            RelayRollbackOutcome::Failed
        };
    RelaySwitchError::after_mutation(error, rollback_outcome, backup_path)
}

fn backfill_profile_before_switch(
    home: &Path,
    settings: &mut BackendSettings,
    previous_active_relay_id: &str,
) -> anyhow::Result<()> {
    let profile = settings
        .relay_profiles
        .iter_mut()
        .find(|profile| profile.id == previous_active_relay_id)
        .with_context(|| "当前供应商已不在配置列表中，已停止切换以避免覆盖用户改动。")?;
    if profile.relay_mode == RelayMode::Aggregate {
        return Ok(());
    }
    backfill_relay_profile_from_home_with_common(
        home,
        profile,
        &mut settings.relay_context_config_contents,
    )
    .with_context(|| "回填当前供应商配置失败")
}

fn apply_selected_relay_profile(
    home: &Path,
    settings: &BackendSettings,
) -> anyhow::Result<RelaySwitchResult> {
    let relay = settings.active_relay_profile();
    let common_config = relay_combined_common_config(settings);
    let result = if relay.relay_mode == RelayMode::Official && !relay.official_mix_api_key {
        let auth_contents =
            (!relay.auth_contents.trim().is_empty()).then_some(relay.auth_contents.as_str());
        crate::relay_config::clear_relay_config_to_home_with_auth_and_computer_use_guard(
            home,
            auth_contents,
            settings.computer_use_guard_enabled,
        )?
    } else {
        validate_switch_profile_files(&relay)?;
        crate::relay_config::apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
            home,
            &relay,
            &common_config,
            settings.computer_use_guard_enabled,
        )?
    };
    let status = relay_config_status_from_home(home);
    Ok(RelaySwitchResult {
        settings: settings.clone(),
        configured: status.configured,
        backup_path: result.backup_path,
    })
}

fn validate_switch_profile_files(profile: &crate::settings::RelayProfile) -> anyhow::Result<()> {
    if profile.relay_mode != RelayMode::Aggregate && profile.config_contents.trim().is_empty() {
        anyhow::bail!(
            "供应商「{}」缺少独立 config.toml，已停止切换，避免继续显示上一套配置文件。",
            if profile.name.trim().is_empty() {
                profile.id.as_str()
            } else {
                profile.name.as_str()
            }
        );
    }
    if profile.relay_mode == RelayMode::Official
        && serde_json::from_str::<serde_json::Value>(&profile.auth_contents)
            .ok()
            .and_then(|value| {
                value
                    .get("OPENAI_API_KEY")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .map(str::is_empty)
            })
            == Some(false)
    {
        anyhow::bail!(
            "官方混合 API 不应在 auth.json 中保存 OPENAI_API_KEY。请清理此供应商的 auth.json 后再切换。"
        );
    }
    Ok(())
}

fn relay_combined_common_config(settings: &BackendSettings) -> String {
    let sections = [
        settings.relay_common_config_contents.trim(),
        settings.relay_context_config_contents.trim(),
    ]
    .into_iter()
    .filter(|section| !section.is_empty())
    .collect::<Vec<_>>();
    if sections.is_empty() {
        String::new()
    } else {
        crate::relay_config::normalize_config_text(&format!("{}\n", sections.join("\n\n")))
    }
}
