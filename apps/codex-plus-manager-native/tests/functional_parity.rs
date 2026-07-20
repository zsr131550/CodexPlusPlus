use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
    schema: u32,
    frozen: bool,
    scope: String,
    oracle: Oracle,
    owners: Vec<OwnerEvidence>,
    routes: Vec<ParityEntry>,
    commands: Vec<ParityEntry>,
    events: Vec<ParityEntry>,
    startup_inputs: Vec<ParityEntry>,
    tray_actions: Vec<ParityEntry>,
    external_links: Vec<ParityEntry>,
    maintenance_controls: Vec<ParityEntry>,
    approved_removals: Vec<ParityEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Oracle {
    command: String,
    compares: Vec<String>,
    excludes: Vec<String>,
    evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerEvidence {
    id: String,
    path: String,
    marker: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceRef {
    path: String,
    marker: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParityEntry {
    legacy: String,
    native: Option<String>,
    owner: Option<String>,
    disposition: Disposition,
    rationale: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    Native,
    ApprovedRemoval,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("native app lives under repository/apps")
        .to_path_buf()
}

fn load_matrix(root: &Path) -> Matrix {
    let path = root.join("docs/contracts/native-manager-functional-parity.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("invalid functional parity matrix: {error}"))
}

fn read_source(root: &Path, relative: &str) -> String {
    let path = root.join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read evidence source {}: {error}", path.display()))
}

fn assert_nonempty(value: &str, label: &str) {
    assert!(!value.trim().is_empty(), "{label} must not be empty");
}

fn validate_evidence(root: &Path, path: &str, marker: &str) {
    let relative = Path::new(path);
    assert!(
        relative.is_relative(),
        "evidence path must be relative: {path}"
    );
    assert!(
        !relative
            .components()
            .any(|component| component == Component::ParentDir),
        "evidence path may not escape the repository: {path}"
    );
    assert_nonempty(marker, "evidence marker");
    let source = read_source(root, path);
    assert!(
        source.contains(marker),
        "evidence marker {marker:?} is missing from {path}"
    );
}

fn owner_index<'a>(
    root: &Path,
    owners: &'a [OwnerEvidence],
) -> BTreeMap<String, &'a OwnerEvidence> {
    let mut index = BTreeMap::new();
    for owner in owners {
        assert_nonempty(&owner.id, "owner id");
        assert_nonempty(&owner.path, "owner path");
        validate_evidence(root, &owner.path, &owner.marker);
        assert!(
            index.insert(owner.id.clone(), owner).is_none(),
            "duplicate owner id: {}",
            owner.id
        );
    }
    index
}

fn validate_entries(
    category: &str,
    entries: &[ParityEntry],
    owners: &BTreeMap<String, &OwnerEvidence>,
) -> BTreeSet<String> {
    assert!(!entries.is_empty(), "{category} matrix must not be empty");
    let mut legacy_ids = BTreeSet::new();
    for entry in entries {
        assert!(
            legacy_ids.insert(entry.legacy.clone()),
            "duplicate {category} legacy id: {}",
            entry.legacy
        );
        assert_nonempty(&entry.legacy, "legacy id");
        assert_nonempty(&entry.rationale, "parity rationale");
        match entry.disposition {
            Disposition::Native => {
                let owner = entry
                    .owner
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| panic!("native entry has no owner: {}", entry.legacy));
                assert!(
                    owners.contains_key(owner),
                    "{} references unknown owner {owner}",
                    entry.legacy
                );
                let native = entry
                    .native
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| panic!("native entry has no target: {}", entry.legacy));
                assert_nonempty(native, "native target");
            }
            Disposition::ApprovedRemoval => {
                assert!(
                    entry.owner.is_none() && entry.native.is_none(),
                    "approved removal must not claim a Native owner: {}",
                    entry.legacy
                );
            }
        }
    }
    legacy_ids
}

fn collect_referenced_owners<'a>(
    categories: impl IntoIterator<Item = &'a [ParityEntry]>,
) -> BTreeSet<String> {
    categories
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter_map(|entry| entry.owner.clone())
        .collect()
}

fn extract_between<'a>(source: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
    let start = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("source marker not found: {start_marker}"))
        + start_marker.len();
    let remaining = &source[start..];
    let end = remaining
        .find(end_marker)
        .unwrap_or_else(|| panic!("source end marker not found: {end_marker}"));
    &remaining[..end]
}

fn parse_react_routes(source: &str) -> BTreeSet<String> {
    let value = extract_between(source, "type Route = ", ";");
    value
        .split('|')
        .map(|route| route.trim().trim_matches('"').to_string())
        .filter(|route| !route.is_empty())
        .collect()
}

fn parse_rust_enum_variants(source: &str, enum_name: &str) -> BTreeSet<String> {
    let start_marker = format!("pub enum {enum_name} {{");
    let body = extract_between(source, &start_marker, "}");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let variant = line
                .split(|character: char| {
                    character == '('
                        || character == '{'
                        || character == ','
                        || character.is_whitespace()
                })
                .next()
                .unwrap_or_default();
            (!variant.is_empty()).then(|| variant.to_string())
        })
        .collect()
}

fn parse_tauri_handlers(source: &str) -> BTreeSet<String> {
    let body = extract_between(source, "tauri::generate_handler![", "])");
    body.lines()
        .map(str::trim)
        .map(|line| line.trim_end_matches(','))
        .filter(|line| !line.is_empty())
        .map(|line| line.rsplit("::").next().unwrap_or(line).to_string())
        .collect()
}

fn collect_quoted_after(source: &str, needle: &str, prefix_len: usize) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(needle) {
        let start = cursor + relative + prefix_len;
        let Some(end) = source[start..].find('"') else {
            break;
        };
        values.insert(source[start..start + end].to_string());
        cursor = start + end + 1;
    }
    values
}

fn collect_call_commands(source: &str) -> BTreeSet<String> {
    let needle = "call<";
    let mut values = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(needle) {
        let call_start = cursor + relative;
        let suffix_start = call_start + needle.len();
        let suffix = &source[suffix_start..];
        let Some(open) = suffix.find(">(\"") else {
            cursor = suffix_start;
            continue;
        };
        let value_start = suffix_start + open + 3;
        let Some(end) = source[value_start..].find('"') else {
            break;
        };
        values.insert(source[value_start..value_start + end].to_string());
        cursor = value_start + end + 1;
    }
    values
}

#[test]
fn final_functional_parity_matrix_is_well_formed_and_evidenced() {
    let root = repository_root();
    let matrix = load_matrix(&root);
    assert_eq!(matrix.schema, 1);
    assert!(matrix.frozen, "the matrix must be frozen before packaging");
    assert_eq!(matrix.scope, "react-tauri-to-egui-native-before-packaging");
    assert_eq!(
        matrix.oracle.command,
        "cargo test -p codex-plus-manager --lib --jobs 1"
    );
    assert_eq!(
        matrix.oracle.compares,
        vec![
            "safe_typed_outputs".to_string(),
            "side_effect_plans".to_string()
        ]
    );
    assert_eq!(
        matrix.oracle.excludes,
        vec!["screenshots".to_string(), "raw_secrets".to_string()]
    );
    for evidence in &matrix.oracle.evidence {
        validate_evidence(&root, &evidence.path, &evidence.marker);
    }

    let owners = owner_index(&root, &matrix.owners);
    let route_ids = validate_entries("route", &matrix.routes, &owners);
    let command_ids = validate_entries("command", &matrix.commands, &owners);
    let event_ids = validate_entries("event", &matrix.events, &owners);
    let startup_ids = validate_entries("startup", &matrix.startup_inputs, &owners);
    let tray_ids = validate_entries("tray", &matrix.tray_actions, &owners);
    let external_ids = validate_entries("external link", &matrix.external_links, &owners);
    let maintenance_ids = validate_entries("maintenance", &matrix.maintenance_controls, &owners);
    let removal_ids = validate_entries("approved removal", &matrix.approved_removals, &owners);

    assert_eq!(route_ids.len(), 11);
    assert_eq!(command_ids.len(), 57);
    assert_eq!(event_ids.len(), 2);
    assert_eq!(startup_ids.len(), 5);
    assert_eq!(tray_ids.len(), 7);
    assert_eq!(external_ids.len(), 6);
    assert_eq!(maintenance_ids.len(), 21);
    assert_eq!(removal_ids.len(), 8);

    let referenced_owners = collect_referenced_owners([
        matrix.routes.as_slice(),
        matrix.commands.as_slice(),
        matrix.events.as_slice(),
        matrix.startup_inputs.as_slice(),
        matrix.tray_actions.as_slice(),
        matrix.external_links.as_slice(),
        matrix.maintenance_controls.as_slice(),
        matrix.approved_removals.as_slice(),
    ]);
    let declared_owners: BTreeSet<String> = owners.keys().cloned().collect();
    assert_eq!(referenced_owners, declared_owners);
}

#[test]
fn final_functional_parity_matrix_matches_routes_and_commands() {
    let root = repository_root();
    let matrix = load_matrix(&root);
    let owners = owner_index(&root, &matrix.owners);
    let _ = validate_entries("route", &matrix.routes, &owners);
    let _ = validate_entries("command", &matrix.commands, &owners);

    let react_source = read_source(&root, "apps/codex-plus-manager/src/App.tsx");
    let native_state_source = read_source(&root, "apps/codex-plus-manager-native/src/state.rs");
    let tauri_source = read_source(&root, "apps/codex-plus-manager/src-tauri/src/lib.rs");

    let expected_routes = BTreeMap::from([
        ("overview".to_string(), "Overview".to_string()),
        ("relay".to_string(), "Providers".to_string()),
        ("relayEnvironment".to_string(), "Environment".to_string()),
        ("sessions".to_string(), "Sessions".to_string()),
        ("context".to_string(), "Context".to_string()),
        ("enhance".to_string(), "Enhancements".to_string()),
        ("zedRemote".to_string(), "ZedRemote".to_string()),
        ("userScripts".to_string(), "Scripts".to_string()),
        ("maintenance".to_string(), "Maintenance".to_string()),
        ("about".to_string(), "About".to_string()),
        ("settings".to_string(), "Settings".to_string()),
    ]);
    let actual_routes: BTreeMap<String, String> = matrix
        .routes
        .iter()
        .map(|entry| {
            (
                entry.legacy.clone(),
                entry.native.clone().expect("validated route target"),
            )
        })
        .collect();
    assert_eq!(actual_routes, expected_routes);
    assert_eq!(
        parse_react_routes(&react_source),
        expected_routes.keys().cloned().collect()
    );
    assert_eq!(
        parse_rust_enum_variants(&native_state_source, "Route"),
        expected_routes.values().cloned().collect()
    );

    let handlers = parse_tauri_handlers(&tauri_source);
    let command_ids: BTreeSet<String> = matrix
        .commands
        .iter()
        .map(|entry| entry.legacy.clone())
        .collect();
    assert_eq!(handlers.len(), 57);
    assert_eq!(handlers, command_ids);

    let mut frontend_commands = collect_call_commands(&react_source);
    frontend_commands.extend(collect_quoted_after(&react_source, "invoke(\"", 8));
    assert!(
        frontend_commands.is_subset(&handlers),
        "frontend invokes commands absent from Tauri handler: {:?}",
        frontend_commands.difference(&handlers).collect::<Vec<_>>()
    );
}

#[test]
fn final_functional_parity_matrix_matches_host_links_and_controls() {
    let root = repository_root();
    let matrix = load_matrix(&root);
    let owners = owner_index(&root, &matrix.owners);
    let _ = validate_entries("event", &matrix.events, &owners);
    let _ = validate_entries("startup", &matrix.startup_inputs, &owners);
    let _ = validate_entries("tray", &matrix.tray_actions, &owners);
    let _ = validate_entries("external link", &matrix.external_links, &owners);
    let _ = validate_entries("maintenance", &matrix.maintenance_controls, &owners);
    let _ = validate_entries("approved removal", &matrix.approved_removals, &owners);

    let tauri_source = read_source(&root, "apps/codex-plus-manager/src-tauri/src/lib.rs");
    let startup_source = read_source(
        &root,
        "crates/codex-plus-manager-service/src/desktop_host.rs",
    );
    let native_host_source =
        read_source(&root, "apps/codex-plus-manager-native/src/desktop_host.rs");
    let native_maintenance_source = read_source(
        &root,
        "apps/codex-plus-manager-native/src/views/maintenance.rs",
    );
    let react_source = read_source(&root, "apps/codex-plus-manager/src/App.tsx");
    let native_about_source =
        read_source(&root, "apps/codex-plus-manager-native/src/views/about.rs");
    let native_scripts_source = read_source(
        &root,
        "apps/codex-plus-manager-native/src/views/user_scripts.rs",
    );

    let event_ids: BTreeSet<String> = matrix
        .events
        .iter()
        .map(|entry| entry.legacy.clone())
        .collect();
    assert_eq!(
        event_ids,
        BTreeSet::from([
            "manager://pending-provider-import-changed".to_string(),
            "provider-import\\n".to_string(),
        ])
    );
    assert!(tauri_source.contains("manager://pending-provider-import-changed"));
    assert!(tauri_source.contains("provider-import\\n"));

    let startup_ids: BTreeSet<String> = matrix
        .startup_inputs
        .iter()
        .map(|entry| entry.legacy.clone())
        .collect();
    assert_eq!(
        startup_ids,
        BTreeSet::from([
            "ordinary-or-no-recognized-input".to_string(),
            "--show-update".to_string(),
            "codexplusplus://...".to_string(),
            "unknown-unicode-argument".to_string(),
            "non-unicode-argument".to_string(),
        ])
    );
    for marker in [
        "--show-update",
        "codexplusplus://",
        "argument.to_str",
        "actions.is_empty()",
    ] {
        assert!(
            startup_source.contains(marker),
            "startup evidence missing: {marker}"
        );
    }

    let tray_native_targets: BTreeSet<String> = matrix
        .tray_actions
        .iter()
        .map(|entry| entry.native.clone().expect("validated tray target"))
        .collect();
    assert_eq!(
        tray_native_targets,
        BTreeSet::from([
            "DesktopHostEvent::TrayShow".to_string(),
            "DesktopHostEvent::TrayQuit".to_string(),
            "DesktopHostEvent::CloseRequested".to_string(),
            "DesktopHostEvent::Minimized(true)".to_string(),
            "TrayController::set_locale".to_string(),
        ])
    );
    for marker in [
        "DesktopHostEvent::TrayShow",
        "DesktopHostEvent::TrayQuit",
        "DesktopHostEvent::Minimized(true)",
        "fn set_locale",
    ] {
        assert!(
            native_host_source.contains(marker),
            "tray evidence missing: {marker}"
        );
    }

    let expected_links: BTreeSet<String> = [
        "https://github.com/BigPizzaV3/CodexPlusPlus",
        "https://github.com/BigPizzaV3/CodexPlusPlus/issues",
        "https://discord.gg/y96kX7A76v",
        "https://t.me/CodexPlusPlus",
        "https://github.com/BigPizzaV3/CodexPlusPlusScriptMarket",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let declared_links: BTreeSet<String> = matrix
        .external_links
        .iter()
        .filter(|entry| entry.legacy.starts_with("https://"))
        .map(|entry| entry.legacy.clone())
        .collect();
    assert_eq!(declared_links, expected_links);
    for link in expected_links {
        assert!(react_source.contains(&link), "React link missing: {link}");
        assert!(
            native_about_source.contains(&link) || native_scripts_source.contains(&link),
            "Native link missing: {link}"
        );
    }
    assert!(native_scripts_source.contains("ExternalUrl::parse(value.as_str())"));

    let maintenance_targets: BTreeSet<String> = matrix
        .maintenance_controls
        .iter()
        .map(|entry| entry.native.clone().expect("validated maintenance target"))
        .collect();
    let expected_maintenance = [
        "Refresh",
        "SetAppPath",
        "PickExecutable",
        "PickDirectory",
        "SaveAppPath",
        "RequestClear",
        "ConfirmClear",
        "CancelClear",
        "SetDebugPort",
        "SetHelperPort",
        "Launch",
        "SetDocumentTab",
        "SetLogLimit",
        "CopyDocument",
        "ConfirmDiscard",
        "CancelDiscard",
        "RequestRepair",
        "ConfirmRepair",
        "CancelRepair",
        "MigrateSignIn",
        "SetStartAtSignIn",
    ]
    .into_iter()
    .map(String::from)
    .collect::<BTreeSet<_>>();
    assert_eq!(maintenance_targets, expected_maintenance);
    assert!(native_maintenance_source.contains("pub enum MaintenanceAction"));
    assert!(native_maintenance_source.contains("SetStartAtSignIn(bool)"));
}
