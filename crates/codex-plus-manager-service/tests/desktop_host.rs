use std::ffi::OsString;

use codex_plus_core::manager_instance::ManagerActivation;
use codex_plus_core::provider_import::load_pending_provider_import_at;
use codex_plus_manager_service::{
    DesktopHostEnvironment, DesktopStartupArgs, DesktopStartupIssueKind,
};

struct TestEnvironment {
    pending_path: std::path::PathBuf,
}

impl DesktopHostEnvironment for TestEnvironment {
    fn pending_import_path(&self) -> &std::path::Path {
        &self.pending_path
    }
}

#[test]
fn desktop_host_preserves_os_arguments_and_ignores_unknown_values() {
    let temp = tempfile::tempdir().unwrap();
    let unknown = non_unicode_argument();
    let original = vec![
        OsString::from("manager"),
        unknown.clone(),
        OsString::from("--future-option"),
    ];
    let args = DesktopStartupArgs::new(original.clone());

    let plan = args.prepare(&TestEnvironment {
        pending_path: temp.path().join("pending.json"),
    });

    assert_eq!(args.original(), original);
    assert_eq!(plan.actions(), &[ManagerActivation::Show]);
    assert_eq!(plan.recognized_count(), 0);
    assert_eq!(plan.unknown_count(), 3);
    assert!(plan.issues().is_empty());
}

#[test]
fn desktop_host_persists_imports_before_returning_ordered_typed_activations() {
    let temp = tempfile::tempdir().unwrap();
    let pending_path = temp.path().join("pending.json");
    let secret = "desktop-host-secret-sentinel";
    let first = provider_url("First", "first-secret");
    let second = provider_url("Second", secret);
    let args = DesktopStartupArgs::new([
        OsString::from("manager"),
        OsString::from(first),
        OsString::from("--show-update"),
        OsString::from(second),
    ]);

    let plan = args.prepare(&TestEnvironment {
        pending_path: pending_path.clone(),
    });

    assert_eq!(
        plan.actions(),
        &[
            ManagerActivation::ReloadPendingProviderImport,
            ManagerActivation::ShowUpdate,
            ManagerActivation::ReloadPendingProviderImport,
        ]
    );
    assert_eq!(plan.recognized_count(), 3);
    assert_eq!(plan.unknown_count(), 1);
    assert!(plan.issues().is_empty());
    let pending = load_pending_provider_import_at(&pending_path)
        .unwrap()
        .unwrap();
    assert_eq!(pending.name, "Second");
    assert_eq!(pending.api_key, secret);
    assert!(!format!("{args:?} {plan:?}").contains(secret));
}

#[test]
fn desktop_host_reports_stable_issues_without_signaling_unpersisted_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let blocking_file = temp.path().join("not-a-directory");
    std::fs::write(&blocking_file, b"fixture").unwrap();
    let invalid_secret = "invalid-import-secret-sentinel";
    let invalid =
        format!("codexplusplus://v1/import/provider?name=MissingBase&apiKey={invalid_secret}");
    let valid_secret = "persist-failure-secret-sentinel";
    let args = DesktopStartupArgs::new([
        OsString::from("manager"),
        OsString::from(invalid),
        OsString::from(provider_url("Blocked", valid_secret)),
    ]);

    let plan = args.prepare(&TestEnvironment {
        pending_path: blocking_file.join("pending.json"),
    });

    assert_eq!(plan.actions(), &[ManagerActivation::Show]);
    assert_eq!(
        plan.issues()
            .iter()
            .map(|issue| issue.kind())
            .collect::<Vec<_>>(),
        [
            DesktopStartupIssueKind::InvalidProviderImport,
            DesktopStartupIssueKind::PersistFailed,
        ]
    );
    let debug = format!("{args:?} {plan:?}");
    assert!(!debug.contains(invalid_secret));
    assert!(!debug.contains(valid_secret));
}

fn provider_url(name: &str, api_key: &str) -> String {
    let mut url = url::Url::parse("codexplusplus://v1/import/provider").unwrap();
    url.query_pairs_mut()
        .append_pair("resource", "provider")
        .append_pair("name", name)
        .append_pair("baseUrl", "https://provider.example/v1")
        .append_pair("apiKey", api_key)
        .append_pair("wireApi", "responses")
        .append_pair("relayMode", "pureApi");
    url.to_string()
}

#[cfg(windows)]
fn non_unicode_argument() -> OsString {
    use std::os::windows::ffi::OsStringExt as _;

    OsString::from_wide(&[0x66, 0x6f, 0x80, 0xd800])
}

#[cfg(unix)]
fn non_unicode_argument() -> OsString {
    use std::os::unix::ffi::OsStringExt as _;

    OsString::from_vec(vec![b'f', b'o', 0x80])
}

#[cfg(not(any(windows, unix)))]
fn non_unicode_argument() -> OsString {
    std::ffi::OsStr::new("unknown-platform-argument").to_os_string()
}
