use codex_plus_core::context_ownership::{
    ContextEntryIdentity, ContextOwnershipManifest, OwnedContextEntry, load_context_ownership_at,
    save_context_ownership_at,
};
use codex_plus_core::relay_config::{
    context_entry_body_from_common_config, effective_context_config_for_profile,
    list_context_entries_from_common_config, plan_owned_context_sync,
    set_context_entry_enabled_in_common_config,
};
use codex_plus_core::settings::{RelayContextSelection, RelayProfile};
use std::fs;

const SECRET: &str = "context-secret-sentinel-7f31";

fn context_config() -> String {
    format!(
        r#"feature_flag = true

[mcp_servers.alpha]
command = "{SECRET}"
args = ["--token", "{SECRET}"]

[mcp_servers.beta]
command = "beta"

[skills.writer]
enabled = true
instructions = "{SECRET}"

[plugins.browser]
enabled = true
token = "{SECRET}"
"#
    )
}

#[test]
fn context_entry_debug_redacts_toml_values() {
    let entries = list_context_entries_from_common_config(&context_config()).unwrap();

    let debug = format!("{:?}", entries.mcp_servers[0]);

    assert!(debug.contains("alpha"));
    assert!(debug.contains("mcp"));
    assert!(!debug.contains(SECRET));
    assert!(!debug.contains("command"));
}

#[test]
fn context_entry_body_returns_only_the_exact_kind_and_id() {
    let body = context_entry_body_from_common_config(&context_config(), "mcp", "alpha")
        .unwrap()
        .unwrap();

    assert!(body.contains(SECRET));
    assert!(body.contains("--token"));
    assert!(!body.contains("beta"));
    assert!(!body.contains("feature_flag"));
    assert!(
        context_entry_body_from_common_config(&context_config(), "plugin", "missing")
            .unwrap()
            .is_none()
    );
    assert!(context_entry_body_from_common_config(&context_config(), "mcp", "  ").is_err());
}

#[test]
fn set_context_entry_enabled_preserves_unrelated_tables() {
    let updated =
        set_context_entry_enabled_in_common_config(&context_config(), "mcp", "alpha", false)
            .unwrap();
    let entries = list_context_entries_from_common_config(&updated).unwrap();

    assert!(!entries.mcp_servers[0].enabled);
    assert!(updated.contains("feature_flag = true"));
    assert!(updated.contains("[mcp_servers.beta]"));
    assert!(updated.contains(SECRET));
    assert!(
        set_context_entry_enabled_in_common_config(&context_config(), "mcp", "missing", true)
            .is_err()
    );
}

#[test]
fn effective_context_config_uses_initialized_profile_selection() {
    let profile = RelayProfile {
        context_selection_initialized: true,
        context_selection: RelayContextSelection {
            mcp_servers: vec!["beta".to_string()],
            skills: vec!["writer".to_string()],
            plugins: Vec::new(),
        },
        ..RelayProfile::default()
    };

    let effective = effective_context_config_for_profile(&context_config(), &profile).unwrap();

    assert!(!effective.contains("[mcp_servers.alpha]"));
    assert!(effective.contains("[mcp_servers.beta]"));
    assert!(effective.contains("[skills.writer]"));
    assert!(!effective.contains("[plugins.browser]"));

    let invalid = format!("[mcp_servers.alpha]\ncommand = \"{SECRET}\n");
    let error = effective_context_config_for_profile(&invalid, &profile).unwrap_err();
    assert!(!format!("{error:?}").contains(SECRET));
}

#[test]
fn effective_context_config_uses_all_enabled_when_selection_is_uninitialized() {
    let config = format!(
        r#"[mcp_servers.alpha]
command = "{SECRET}"

[mcp_servers.disabled]
enabled = false
command = "disabled"

[plugins.browser]
enabled = true
token = "{SECRET}"
"#
    );
    let profile = RelayProfile {
        context_selection_initialized: false,
        context_selection: RelayContextSelection {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        },
        ..RelayProfile::default()
    };

    let effective = effective_context_config_for_profile(&config, &profile).unwrap();

    assert!(effective.contains("[mcp_servers.alpha]"));
    assert!(!effective.contains("[mcp_servers.disabled]"));
    assert!(effective.contains("[plugins.browser]"));
}

fn identity(kind: &str, id: &str) -> ContextEntryIdentity {
    ContextEntryIdentity {
        kind: kind.to_string(),
        id: id.to_string(),
    }
}

fn owned(kind: &str, id: &str, hash: char) -> OwnedContextEntry {
    OwnedContextEntry {
        identity: identity(kind, id),
        body_sha256: hash.to_string().repeat(64),
    }
}

#[test]
fn ownership_manifest_round_trips_without_toml_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("context-live-ownership.json");
    let manifest = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("mcp", "alpha", 'a'), owned("plugin", "browser", 'b')],
    };

    save_context_ownership_at(&path, &manifest).unwrap();
    let loaded = load_context_ownership_at(&path).unwrap();
    let json = fs::read_to_string(path).unwrap();

    assert_eq!(loaded, manifest);
    assert!(!json.contains(SECRET));
    assert!(!format!("{loaded:?}").contains(SECRET));
}

#[test]
fn ownership_revision_is_canonical_and_order_independent() {
    let first = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("plugin", "browser", 'b'), owned("mcp", "alpha", 'a')],
    };
    let second = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("mcp", "alpha", 'a'), owned("plugin", "browser", 'b')],
    };

    assert_eq!(first.revision(), second.revision());
    assert_eq!(first.revision().as_str().len(), 64);
}

#[test]
fn ownership_save_replaces_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested/context-live-ownership.json");
    let first = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("mcp", "alpha", 'a')],
    };
    let second = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("skill", "writer", 'b')],
    };

    save_context_ownership_at(&path, &first).unwrap();
    save_context_ownership_at(&path, &second).unwrap();

    assert_eq!(load_context_ownership_at(&path).unwrap(), second);
    assert!(!path.with_extension("json.tmp").exists());
}

#[test]
fn invalid_ownership_manifest_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("context-live-ownership.json");
    let invalid_documents = [
        r#"{"version":2,"entries":[]}"#,
        r#"{"version":1,"entries":[{"identity":{"kind":"other","id":"x"},"bodySha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
        r#"{"version":1,"entries":[{"identity":{"kind":"mcp","id":"x"},"bodySha256":"bad"}]}"#,
        r#"{"version":1,"entries":[{"identity":{"kind":"mcp","id":"x"},"bodySha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},{"identity":{"kind":"mcp","id":"x"},"bodySha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}]}"#,
    ];

    for document in invalid_documents {
        fs::write(&path, document).unwrap();
        assert!(load_context_ownership_at(&path).is_err(), "{document}");
    }
}

#[test]
fn default_ownership_path_uses_app_state_directory() {
    assert!(
        codex_plus_core::paths::default_context_ownership_path()
            .ends_with(".codex-session-delete/context-live-ownership.json")
    );
}

#[test]
fn sync_plan_preserves_unknown_live_entries() {
    let live = r#"model = "gpt"

[mcp_servers.desired]
command = "old"

[skills.deleted]
enabled = true

[plugins.manual]
token = "manual-secret"
"#;
    let desired = r#"[mcp_servers.desired]
command = "new"
"#;
    let previous = ContextOwnershipManifest {
        version: 1,
        entries: vec![owned("mcp", "desired", 'a'), owned("skill", "deleted", 'b')],
    };

    let plan = plan_owned_context_sync(live, desired, &previous).unwrap();

    assert!(plan.updated_live_config.contains("[plugins.manual]"));
    assert!(plan.updated_live_config.contains("manual-secret"));
    assert!(plan.updated_live_config.contains("command = \"new\""));
    assert!(!plan.updated_live_config.contains("[skills.deleted]"));
}

#[test]
fn sync_plan_removes_owned_deleted_disabled_and_excluded_entries() {
    let live = r#"[mcp_servers.deleted]
command = "delete"

[skills.disabled]
enabled = true

[plugins.excluded]
enabled = true

[plugins.manual]
enabled = true
"#;
    let desired = r#"[skills.disabled]
enabled = false
"#;
    let previous = ContextOwnershipManifest {
        version: 1,
        entries: vec![
            owned("mcp", "deleted", 'a'),
            owned("skill", "disabled", 'b'),
            owned("plugin", "excluded", 'c'),
        ],
    };

    let plan = plan_owned_context_sync(live, desired, &previous).unwrap();

    assert!(!plan.updated_live_config.contains("deleted"));
    assert!(!plan.updated_live_config.contains("disabled"));
    assert!(!plan.updated_live_config.contains("excluded"));
    assert!(plan.updated_live_config.contains("[plugins.manual]"));
    assert!(plan.next_manifest.entries.is_empty());
}

#[test]
fn sync_plan_classifies_added_updated_removed_and_unchanged_keys() {
    let live = r#"[mcp_servers.added_elsewhere]
command = "manual"

[mcp_servers.unchanged]
command = "same"

[skills.updated]
enabled = true
instructions = "old"

[plugins.removed]
enabled = true
"#;
    let desired = r#"[mcp_servers.added]
command = "new"

[mcp_servers.unchanged]
command = "same"

[skills.updated]
enabled = true
instructions = "new"
"#;
    let previous = ContextOwnershipManifest {
        version: 1,
        entries: vec![
            owned("mcp", "unchanged", 'a'),
            owned("skill", "updated", 'b'),
            owned("plugin", "removed", 'c'),
        ],
    };

    let plan = plan_owned_context_sync(live, desired, &previous).unwrap();

    assert_eq!(plan.diff.added, vec![identity("mcp", "added")]);
    assert_eq!(plan.diff.updated, vec![identity("skill", "updated")]);
    assert_eq!(plan.diff.removed, vec![identity("plugin", "removed")]);
    assert_eq!(plan.diff.unchanged, vec![identity("mcp", "unchanged")]);
}

#[test]
fn sync_plan_debug_redacts_updated_live_config() {
    let desired = format!(
        r#"[plugins.browser]
token = "{SECRET}"
"#
    );

    let plan = plan_owned_context_sync("", &desired, &ContextOwnershipManifest::default()).unwrap();
    let debug = format!("{plan:?}");

    assert!(!debug.contains(SECRET));
    assert!(!debug.contains("token"));
    assert!(debug.contains("added"));
}
