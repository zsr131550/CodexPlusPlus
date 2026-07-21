use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

const BASELINE_COMMIT: &str = "ae13ba110a18ddb1f93ce799dfcbc052292626e2";
const BASELINE_CI_RUN: u64 = 29_768_331_734;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
    schema: u32,
    frozen: bool,
    scope: String,
    baseline: Baseline,
    single_stack: SingleStackIdentity,
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
struct Baseline {
    commit: String,
    ci_run: u64,
    compares: Vec<String>,
    excludes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingleStackIdentity {
    app_root: String,
    package: String,
    library: String,
    binary: String,
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
struct ParityEntry {
    legacy: String,
    native: Option<String>,
    owner: Option<String>,
    disposition: Disposition,
    rationale: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    Native,
    ApprovedRemoval,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("manager app lives under repository/apps")
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
            !owner.path.contains("src-tauri"),
            "stale owner: {}",
            owner.id
        );
        assert!(
            !owner.path.contains("codex-plus-manager-native"),
            "implementation-specific owner: {}",
            owner.id
        );
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
        assert_nonempty(&entry.legacy, "legacy id");
        assert_nonempty(&entry.rationale, "parity rationale");
        assert!(
            legacy_ids.insert(entry.legacy.clone()),
            "duplicate {category} legacy id: {}",
            entry.legacy
        );
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
            Disposition::ApprovedRemoval => assert!(
                entry.owner.is_none() && entry.native.is_none(),
                "approved removal must not claim a Native owner: {}",
                entry.legacy
            ),
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

fn expected_routes() -> BTreeMap<String, String> {
    BTreeMap::from([
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
    ])
}

#[test]
fn final_single_stack_matrix_is_well_formed_and_evidenced() {
    let root = repository_root();
    let matrix = load_matrix(&root);

    assert_eq!(matrix.schema, 2);
    assert!(matrix.frozen, "the final matrix must stay frozen");
    assert_eq!(
        matrix.scope,
        "egui-native-single-stack-after-react-tauri-removal"
    );
    assert_eq!(matrix.baseline.commit, BASELINE_COMMIT);
    assert_eq!(matrix.baseline.ci_run, BASELINE_CI_RUN);
    assert_eq!(
        matrix.baseline.compares,
        vec![
            "safe_typed_outputs".to_string(),
            "side_effect_plans".to_string(),
        ]
    );
    assert_eq!(matrix.baseline.excludes, vec!["raw_secrets".to_string()]);
    assert_eq!(matrix.single_stack.app_root, "apps/codex-plus-manager");
    assert_eq!(matrix.single_stack.package, "codex-plus-manager");
    assert_eq!(matrix.single_stack.library, "codex_plus_manager");
    assert_eq!(matrix.single_stack.binary, "codex-plus-plus-manager");

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
fn final_single_stack_matrix_matches_native_routes_and_commands() {
    let root = repository_root();
    let matrix = load_matrix(&root);
    let owners = owner_index(&root, &matrix.owners);
    let _ = validate_entries("route", &matrix.routes, &owners);
    let _ = validate_entries("command", &matrix.commands, &owners);

    let native_state = read_source(&root, "apps/codex-plus-manager/src/state.rs");
    let expected_routes = expected_routes();
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
        parse_rust_enum_variants(&native_state, "Route"),
        expected_routes.values().cloned().collect()
    );

    let removed_commands = matrix
        .commands
        .iter()
        .filter(|entry| entry.disposition == Disposition::ApprovedRemoval)
        .map(|entry| entry.legacy.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        removed_commands,
        BTreeSet::from(["load_settings", "save_settings"])
    );
    assert_eq!(
        matrix
            .commands
            .iter()
            .filter(|entry| entry.disposition == Disposition::Native)
            .count(),
        55
    );
}

#[test]
fn final_single_stack_matrix_matches_host_links_and_controls() {
    let root = repository_root();
    let matrix = load_matrix(&root);
    let owners = owner_index(&root, &matrix.owners);
    let _ = validate_entries("event", &matrix.events, &owners);
    let _ = validate_entries("startup", &matrix.startup_inputs, &owners);
    let _ = validate_entries("tray", &matrix.tray_actions, &owners);
    let _ = validate_entries("external link", &matrix.external_links, &owners);
    let _ = validate_entries("maintenance", &matrix.maintenance_controls, &owners);
    let _ = validate_entries("approved removal", &matrix.approved_removals, &owners);

    let startup_source = read_source(
        &root,
        "crates/codex-plus-manager-service/src/desktop_host.rs",
    );
    let instance_source = read_source(&root, "crates/codex-plus-core/src/manager_instance.rs");
    let native_host = read_source(&root, "apps/codex-plus-manager/src/desktop_host.rs");
    let maintenance = read_source(&root, "apps/codex-plus-manager/src/views/maintenance.rs");
    let about = read_source(&root, "apps/codex-plus-manager/src/views/about.rs");
    let scripts = read_source(&root, "apps/codex-plus-manager/src/views/user_scripts.rs");

    let event_ids = matrix
        .events
        .iter()
        .map(|entry| entry.legacy.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        event_ids,
        BTreeSet::from([
            "manager://pending-provider-import-changed",
            "provider-import\\n"
        ])
    );
    let removed_text_signal = matrix
        .events
        .iter()
        .find(|entry| entry.legacy == "provider-import\\n")
        .expect("legacy text signal disposition");
    assert_eq!(
        removed_text_signal.disposition,
        Disposition::ApprovedRemoval
    );
    for marker in [
        "ManagerActivation::Show",
        "ManagerActivation::ReloadPendingProviderImport",
    ] {
        assert!(
            instance_source.contains(marker),
            "broker evidence missing: {marker}"
        );
    }
    assert!(
        startup_source.contains("ManagerActivation::ShowUpdate"),
        "desktop startup evidence missing: ManagerActivation::ShowUpdate"
    );

    let startup_ids = matrix
        .startup_inputs
        .iter()
        .map(|entry| entry.legacy.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        startup_ids,
        BTreeSet::from([
            "ordinary-or-no-recognized-input",
            "--show-update",
            "codexplusplus://...",
            "unknown-unicode-argument",
            "non-unicode-argument",
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

    let tray_targets = matrix
        .tray_actions
        .iter()
        .map(|entry| entry.native.as_deref().expect("validated tray target"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tray_targets,
        BTreeSet::from([
            "DesktopHostEvent::TrayShow",
            "DesktopHostEvent::TrayQuit",
            "DesktopHostEvent::CloseRequested",
            "DesktopHostEvent::Minimized(true)",
            "TrayController::set_locale",
        ])
    );
    for marker in [
        "DesktopHostEvent::TrayShow",
        "DesktopHostEvent::TrayQuit",
        "DesktopHostEvent::Minimized(true)",
        "fn set_locale",
    ] {
        assert!(
            native_host.contains(marker),
            "tray evidence missing: {marker}"
        );
    }

    let expected_links = [
        "https://github.com/BigPizzaV3/CodexPlusPlus",
        "https://github.com/BigPizzaV3/CodexPlusPlus/issues",
        "https://discord.gg/y96kX7A76v",
        "https://t.me/CodexPlusPlus",
        "https://github.com/BigPizzaV3/CodexPlusPlusScriptMarket",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let declared_links = matrix
        .external_links
        .iter()
        .filter(|entry| entry.legacy.starts_with("https://"))
        .map(|entry| entry.legacy.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(declared_links, expected_links);
    for link in expected_links {
        assert!(
            about.contains(link) || scripts.contains(link),
            "Native link missing: {link}"
        );
    }
    assert!(scripts.contains("ExternalUrl::parse(value.as_str())"));

    let maintenance_targets = matrix
        .maintenance_controls
        .iter()
        .map(|entry| {
            entry
                .native
                .as_deref()
                .expect("validated maintenance target")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        maintenance_targets,
        [
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
        .collect()
    );
    assert!(maintenance.contains("pub enum MaintenanceAction"));
    assert!(maintenance.contains("SetStartAtSignIn(bool)"));

    let removed_controls = matrix
        .approved_removals
        .iter()
        .map(|entry| entry.legacy.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        removed_controls,
        BTreeSet::from([
            "restart_codex_plus",
            "watcher_install",
            "watcher_uninstall",
            "watcher_enable",
            "watcher_disable",
            "manager_self_install",
            "manager_self_uninstall",
            "global_reset",
        ])
    );
}
