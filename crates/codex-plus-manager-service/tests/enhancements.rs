use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use codex_plus_core::settings::{BackendSettings, LaunchMode};
use codex_plus_manager_service::{
    EnhancementErrorKind, EnhancementSettings, EnhancementSettingsEnvironment,
    EnhancementSettingsService, ResetEnhancements, SaveEnhancements,
};
use serde_json::Value;

#[derive(Clone)]
struct FakeEnvironment {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    settings: BackendSettings,
    payloads: Vec<Value>,
    unknown_root_preserved: bool,
}

impl FakeEnvironment {
    fn new(settings: BackendSettings) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                settings,
                payloads: Vec::new(),
                unknown_root_preserved: true,
            })),
        }
    }

    fn mutate_unrelated_fields(&self) {
        let mut state = self.state.lock().unwrap();
        state.settings.provider_sync_enabled = !state.settings.provider_sync_enabled;
        state.settings.zed_remote_sync_to_zed_settings = true;
    }

    fn mutate_owned_field(&self) {
        let mut state = self.state.lock().unwrap();
        state.settings.codex_app_markdown_export = !state.settings.codex_app_markdown_export;
    }
}

impl EnhancementSettingsEnvironment for FakeEnvironment {
    fn load_enhancement_settings(&self) -> anyhow::Result<BackendSettings> {
        Ok(self.state.lock().unwrap().settings.clone())
    }

    fn update_enhancement_settings_if<F>(
        &self,
        payload: Value,
        predicate: F,
    ) -> anyhow::Result<Option<BackendSettings>>
    where
        F: FnOnce(&BackendSettings) -> bool,
    {
        let mut state = self.state.lock().unwrap();
        state.payloads.push(payload.clone());
        if !predicate(&state.settings) {
            return Ok(None);
        }

        let mut serialized = serde_json::to_value(&state.settings)?;
        let target = serialized
            .as_object_mut()
            .expect("BackendSettings serializes as an object");
        for (key, value) in payload
            .as_object()
            .expect("enhancement patch is a JSON object")
        {
            target.insert(key.clone(), value.clone());
        }
        state.settings = serde_json::from_value(serialized)?;
        Ok(Some(state.settings.clone()))
    }
}

#[test]
fn load_projects_only_owned_fields_and_uses_an_opaque_revision() {
    let stored = BackendSettings {
        enhancements_enabled: false,
        computer_use_guard_enabled: true,
        launch_mode: LaunchMode::Relay,
        codex_app_service_tier_controls: true,
        codex_app_thread_id_badge: true,
        zed_remote_sync_to_zed_settings: true,
        relay_api_key: "private-adjacent-key".to_owned(),
        ..BackendSettings::default()
    };
    let service = EnhancementSettingsService::new(FakeEnvironment::new(stored));

    let workspace = service.load().unwrap();

    assert!(!workspace.settings.enabled);
    assert!(workspace.settings.computer_use_guard);
    assert_eq!(workspace.settings.launch_mode, LaunchMode::Relay);
    assert!(workspace.settings.service_tier_controls);
    assert!(workspace.settings.thread_id_badge);
    let debug = format!("{workspace:?}");
    assert!(debug.contains("EnhancementRevision([opaque])"));
    assert!(!debug.contains("private-adjacent-key"));
    assert!(!debug.contains("zed_remote_sync_to_zed_settings"));
}

#[test]
fn save_patches_exact_owned_keys_and_preserves_unrelated_and_inert_fields() {
    let environment = FakeEnvironment::new(BackendSettings {
        provider_sync_enabled: false,
        zed_remote_sync_to_zed_settings: false,
        ..BackendSettings::default()
    });
    let service = EnhancementSettingsService::new(environment.clone());
    let workspace = service.load().unwrap();
    environment.mutate_unrelated_fields();

    let saved = service
        .save(SaveEnhancements {
            expected_revision: workspace.revision,
            settings: all_disabled_settings(),
        })
        .unwrap();

    assert!(!saved.settings.enabled);
    assert!(!saved.settings.plugin_marketplace_unlock);
    let state = environment.state.lock().unwrap();
    assert!(state.settings.provider_sync_enabled);
    assert!(state.settings.zed_remote_sync_to_zed_settings);
    assert!(state.unknown_root_preserved);
    let keys = state.payloads[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys, owned_json_keys());
}

#[test]
fn same_scope_conflicts_refresh_but_consumed_revisions_cannot_be_reused() {
    let environment = FakeEnvironment::new(BackendSettings::default());
    let service = EnhancementSettingsService::new(environment.clone());
    let stale = service.load().unwrap();
    environment.mutate_owned_field();

    let conflict = service
        .save(SaveEnhancements {
            expected_revision: stale.revision,
            settings: all_disabled_settings(),
        })
        .unwrap_err();

    assert_eq!(conflict.kind(), EnhancementErrorKind::SettingsConflict);
    assert!(conflict.refreshed_workspace().is_some());
    assert_eq!(
        service
            .save(SaveEnhancements {
                expected_revision: stale.revision,
                settings: all_disabled_settings(),
            })
            .unwrap_err()
            .kind(),
        EnhancementErrorKind::InvalidRevision
    );
}

#[test]
fn unconfirmed_reset_does_not_consume_the_revision_and_confirmed_reset_is_scoped() {
    let stored = BackendSettings {
        enhancements_enabled: false,
        computer_use_guard_enabled: true,
        codex_app_plugin_marketplace_unlock: false,
        zed_remote_sync_to_zed_settings: true,
        provider_sync_enabled: true,
        ..BackendSettings::default()
    };
    let environment = FakeEnvironment::new(stored);
    let service = EnhancementSettingsService::new(environment.clone());
    let workspace = service.load().unwrap();

    assert_eq!(
        service
            .reset(ResetEnhancements {
                expected_revision: workspace.revision,
                confirmed: false,
            })
            .unwrap_err()
            .kind(),
        EnhancementErrorKind::ConfirmationRequired
    );
    let reset = service
        .reset(ResetEnhancements {
            expected_revision: workspace.revision,
            confirmed: true,
        })
        .unwrap();

    assert_eq!(reset.settings, EnhancementSettings::default());
    let state = environment.state.lock().unwrap();
    assert!(state.settings.zed_remote_sync_to_zed_settings);
    assert!(state.settings.provider_sync_enabled);
    assert_eq!(
        state.payloads.last().unwrap().as_object().unwrap().len(),
        owned_json_keys().len()
    );
}

fn all_disabled_settings() -> EnhancementSettings {
    EnhancementSettings {
        enabled: false,
        computer_use_guard: false,
        launch_mode: LaunchMode::Patch,
        plugin_marketplace_unlock: false,
        plugin_auto_expand: false,
        model_whitelist_unlock: false,
        service_tier_controls: false,
        session_delete: false,
        markdown_export: false,
        paste_fix: false,
        project_move: false,
        thread_id_badge: false,
        conversation_view: false,
        thread_scroll_restore: false,
        pet_real_mouse_look: false,
        force_chinese_locale: false,
        fast_startup: false,
        native_menu_placement: false,
        native_menu_localization: false,
        zed_remote_open: false,
        upstream_worktree_create: false,
    }
}

fn owned_json_keys() -> BTreeSet<&'static str> {
    [
        "enhancementsEnabled",
        "computerUseGuardEnabled",
        "launchMode",
        "codexAppPluginMarketplaceUnlock",
        "codexAppPluginAutoExpand",
        "codexAppModelWhitelistUnlock",
        "codexAppServiceTierControls",
        "codexAppSessionDelete",
        "codexAppMarkdownExport",
        "codexAppPasteFix",
        "codexAppProjectMove",
        "codexAppThreadIdBadge",
        "codexAppConversationView",
        "codexAppThreadScrollRestore",
        "codexAppPetRealMouseLook",
        "codexAppForceChineseLocale",
        "codexAppFastStartup",
        "codexAppNativeMenuPlacement",
        "codexAppNativeMenuLocalization",
        "codexAppZedRemoteOpen",
        "codexAppUpstreamWorktreeCreate",
    ]
    .into_iter()
    .collect()
}
