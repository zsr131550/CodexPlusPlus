use std::path::{Path, PathBuf};
use std::process::Command;

const SCOPED_CLIPPY_COMMAND: &str = "cargo clippy -p codex-plus-manager-service -p codex-plus-manager --all-targets --no-deps -- -D warnings";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("manager app lives under repository/apps")
        .to_path_buf()
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn python_command() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn run_text(root: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .current_dir(root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{program} failed to start: {error}"));
    assert!(
        output.status.success(),
        "{program} returned a non-success status"
    );
    String::from_utf8(output.stdout).expect("command output should be UTF-8")
}

#[test]
fn stable_manager_is_the_only_workspace_target() {
    let root = repository_root();
    let workspace = read(root.join("Cargo.toml"));
    let manager = read(root.join("apps/codex-plus-manager/Cargo.toml"));
    let lock = read(root.join("Cargo.lock"));

    assert!(workspace.contains("\"apps/codex-plus-manager\""));
    assert!(!workspace.contains("codex-plus-manager-native"));
    assert!(!workspace.contains("src-tauri"));
    assert!(manager.contains("name = \"codex-plus-manager\""));
    assert!(manager.contains("name = \"codex_plus_manager\""));
    assert!(manager.contains("name = \"codex-plus-plus-manager\""));
    assert!(!manager.contains("tauri"));
    assert!(!lock.contains("name = \"codex-plus-manager-native\""));
    assert!(!lock.contains("name = \"tauri\""));
    assert!(!lock.contains("name = \"tauri-build\""));
    assert!(!lock.contains("name = \"tauri-plugin-dialog\""));

    for removed in [
        "apps/codex-plus-manager-native/Cargo.toml",
        "apps/codex-plus-manager-native/src/lib.rs",
        "apps/codex-plus-manager-native/assets/packaging/icon.ico",
        "apps/codex-plus-manager/src-tauri/Cargo.toml",
        "apps/codex-plus-manager/package.json",
        "apps/codex-plus-manager/src/App.tsx",
    ] {
        assert!(
            !root.join(removed).exists(),
            "removed path still exists: {removed}"
        );
    }
    for required in [
        "apps/codex-plus-manager/Cargo.toml",
        "apps/codex-plus-manager/src/lib.rs",
        "apps/codex-plus-manager/src/main.rs",
        "apps/codex-plus-manager/assets/packaging/icon.ico",
    ] {
        assert!(
            root.join(required).exists(),
            "stable path is missing: {required}"
        );
    }
}

#[test]
fn tracked_tree_metadata_and_built_artifact_use_stable_manager() {
    let root = repository_root();
    let tracked = run_text(
        &root,
        "git",
        &[
            "ls-files",
            "--",
            "apps/codex-plus-manager",
            "apps/codex-plus-manager-native",
        ],
    );
    let pending = run_text(
        &root,
        "git",
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            "apps/codex-plus-manager",
            "apps/codex-plus-manager-native",
        ],
    );
    let intended_tracked = tracked
        .lines()
        .chain(pending.lines())
        .filter(|path| root.join(path).is_file())
        .collect::<Vec<_>>();
    assert!(intended_tracked.contains(&"apps/codex-plus-manager/src/main.rs"));
    assert!(intended_tracked.iter().all(|path| {
        !path.contains("apps/codex-plus-manager-native")
            && !path.contains("src-tauri/")
            && !path.ends_with(".tsx")
            && !path.ends_with("package.json")
    }));

    let metadata = run_text(
        &root,
        "cargo",
        &["metadata", "--no-deps", "--format-version", "1", "--locked"],
    );
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata).expect("cargo metadata should be valid JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata should contain packages");
    let managers = packages
        .iter()
        .filter(|package| package["name"] == "codex-plus-manager")
        .collect::<Vec<_>>();
    assert_eq!(managers.len(), 1);
    assert!(
        !packages
            .iter()
            .any(|package| package["name"] == "codex-plus-manager-native")
    );
    let targets = managers[0]["targets"]
        .as_array()
        .expect("manager metadata should contain targets");
    assert!(targets.iter().any(|target| {
        target["name"] == "codex_plus_manager"
            && target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib"))
    }));
    assert!(targets.iter().any(|target| {
        target["name"] == "codex-plus-plus-manager"
            && target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
    }));

    let tree = run_text(&root, "cargo", &["tree", "--workspace", "--locked"]);
    assert!(!tree.contains("tauri"));
    assert!(!tree.contains("codex-plus-manager-native"));

    let binary = Path::new(env!("CARGO_BIN_EXE_codex-plus-plus-manager"));
    assert!(binary.is_file(), "stable manager test artifact is missing");
    let expected_name = if cfg!(windows) {
        "codex-plus-plus-manager.exe"
    } else {
        "codex-plus-plus-manager"
    };
    assert_eq!(
        binary.file_name().and_then(|name| name.to_str()),
        Some(expected_name)
    );
}

#[test]
fn active_workflows_have_no_frontend_oracle() {
    let root = repository_root();
    let pr = read(root.join(".github/workflows/pr-build.yml"));
    let release = read(root.join(".github/workflows/release-assets.yml"));

    for (name, content) in [("pull request", pr.as_str()), ("release", release.as_str())] {
        for forbidden in [
            "oracle-validation",
            "setup-node",
            "npm.cmd",
            "vite:build",
            "apps/codex-plus-manager-native",
            "apps/codex-plus-manager/src-tauri",
            "codex-plus-manager-native --",
            "codex-plus-plus-manager-native.exe",
        ] {
            assert!(
                !content.contains(forbidden),
                "{name} contains stale {forbidden}"
            );
        }
        for line in content.lines() {
            if line.contains("codex-plus-plus-manager-native") {
                assert!(
                    line.contains("--forbid"),
                    "legacy implementation marker must only be a negative guard: {line}"
                );
            }
        }
        assert!(content.contains("cargo build -p codex-plus-launcher -p codex-plus-manager"));
        assert!(content.contains("codex-plus-plus-manager.exe"));
    }
    assert!(pr.contains("cargo test --workspace"));
    assert!(!pr.contains("--exclude codex-plus-manager"));
}

#[test]
fn active_packagers_use_stable_manager_assets() {
    let root = repository_root();
    let workspace = read(root.join("Cargo.toml"));
    let pr = read(root.join(".github/workflows/pr-build.yml"));
    let release = read(root.join(".github/workflows/release-assets.yml"));
    let launcher = read(root.join("apps/codex-plus-launcher/build.rs"));
    let manager_build = read(root.join("apps/codex-plus-manager/build.rs"));
    let windows_manifest =
        read(root.join("apps/codex-plus-manager/assets/packaging/windows-app-manifest.xml"));
    let nsis = read(root.join("scripts/installer/windows/CodexPlusPlus.nsi"));
    let dmg = read(root.join("scripts/installer/macos/package-dmg.sh"));
    let perf = read(root.join("scripts/perf/native-manager.ps1"));

    assert!(launcher.contains("../codex-plus-manager/assets/packaging/icon.ico"));
    assert!(!launcher.contains("codex-plus-manager-native"));
    assert!(nsis.contains("apps\\codex-plus-manager\\assets\\packaging\\icon.ico"));
    assert!(!nsis.contains("apps\\codex-plus-manager-native\\assets"));
    assert!(dmg.contains("MANAGER_BINARY"));
    assert!(dmg.contains("apps/codex-plus-manager/assets/packaging/icon.png"));
    assert!(!dmg.contains("apps/codex-plus-manager-native"));
    assert!(perf.contains("codex-plus-plus-manager.exe"));
    assert!(perf.contains("cargo build -p codex-plus-manager"));
    assert!(workspace.contains("[profile.native-perf]"));
    assert!(workspace.contains("inherits = \"release\""));
    assert!(windows_manifest.contains("requireAdministrator"));
    assert!(manager_build.contains("CODEX_PLUS_MANAGER_PERF_AS_INVOKER"));
    assert!(
        manager_build.contains("cargo:rerun-if-env-changed=CODEX_PLUS_MANAGER_PERF_AS_INVOKER")
    );
    assert!(manager_build.contains("replace(\"requireAdministrator\", \"asInvoker\")"));
    assert!(perf.contains("$FirstFrameLimitMs = 200.0"));
    assert!(perf.contains("--profile native-perf"));
    assert!(perf.contains("$env:CODEX_PLUS_MANAGER_PERF_AS_INVOKER = '1'"));
    assert!(perf.contains("$PreviousPerfAsInvoker"));
    assert!(perf.contains(r"target\native-perf\codex-plus-plus-manager.exe"));
    for (name, packager) in [
        ("pull request workflow", pr.as_str()),
        ("release workflow", release.as_str()),
        ("Windows installer", nsis.as_str()),
        ("macOS packager", dmg.as_str()),
    ] {
        for forbidden in [
            "--profile native-perf",
            "target/native-perf",
            "target\\native-perf",
            "CODEX_PLUS_MANAGER_PERF_AS_INVOKER",
        ] {
            assert!(
                !packager.contains(forbidden),
                "{name} must not stage the measurement-only artifact: {forbidden}"
            );
        }
    }
}

#[test]
fn package_manifest_contract_records_hashes_without_private_paths() {
    let root = repository_root();
    let manifest = read(root.join("scripts/installer/generate-package-manifest.py"));

    assert!(manifest.contains("sha256"));
    assert!(manifest.contains("source_sha256"));
    assert!(manifest.contains("staged_sha256"));
    assert!(manifest.contains("implementation"));
    assert!(manifest.contains("native"));
    assert!(manifest.contains("relative_to"));
    assert!(manifest.contains("os.replace"));
    assert!(!manifest.contains("os.environ"));
}

#[test]
fn package_workflows_keep_pinned_rollback_and_privacy_guards() {
    let root = repository_root();
    let pr = read(root.join(".github/workflows/pr-build.yml"));
    let release = read(root.join(".github/workflows/release-assets.yml"));
    let nsis = read(root.join("scripts/installer/windows/CodexPlusPlus.nsi"));
    let lifecycle = read(root.join("scripts/installer/run-package-lifecycle-fixture.py"));
    let probe = read(root.join("scripts/installer/probe-packaged-manager.py"));

    assert!(pr.contains("generate-package-manifest.py"));
    assert!(pr.contains("native-package-manifest"));
    assert!(pr.contains("Run pinned package lifecycle fixture"));
    assert!(pr.contains("Mark disposable package profile"));
    assert!(pr.contains("package lifecycle profile ownership requires GitHub Actions"));
    assert!(pr.contains("CODEX_PLUS_PACKAGE_WINDOWS_PROFILE"));
    assert!(pr.contains(
        "PREVIOUS_SHA256: 40e7603223a0e8fef43d546f94ad594a3f3a97717ef01d31401edb5ce86e62ef"
    ));
    assert!(!pr.contains("softprops/action-gh-release"));
    assert!(release.contains("generate-package-manifest.py"));
    assert!(release.contains("if: github.event_name == 'release'"));
    assert!(release.contains("latest-json:"));
    assert!(!nsis.contains("native-package-manifest"));
    assert!(lifecycle.contains("verify_native_tree"));
    assert!(lifecycle.contains("previous_sha256"));
    assert!(!lifecycle.contains("latest"));
    assert!(probe.contains("CODEX_PLUS_PACKAGE_DISPOSABLE_PROFILE"));
    assert!(probe.contains("CODEX_PLUS_PACKAGE_WINDOWS_PROFILE"));
    assert!(probe.contains("manager.provider_import_url.pending"));
    assert!(probe.contains("native_manager.run_failed"));
    assert!(probe.contains("MAX_STATE_EVIDENCE_BYTES"));
    assert!(probe.contains("WEBVIEW2_USER_DATA_FOLDER"));
    assert!(probe.contains("cwd=working_directory"));
    assert!(
        lifecycle.contains("assert_install_root_empty(install_root, \"previous package removal\")")
    );
}

#[test]
fn package_manifest_hashes_staged_files_and_rejects_mismatch() {
    let root = repository_root();
    let script = root.join("scripts/installer/generate-package-manifest.py");
    let temp = tempfile::tempdir().expect("create manifest fixture");
    let stage = temp.path().join("stage");
    std::fs::create_dir_all(&stage).expect("create stage");
    let source = temp.path().join("manager-source.bin");
    let staged = stage.join("codex-plus-plus-manager.exe");
    std::fs::write(&source, b"manager-binary").expect("write source");
    std::fs::write(&staged, b"manager-binary").expect("write staged manager");
    std::fs::write(stage.join("codex-plus-plus.exe"), b"launcher").expect("write launcher");
    let manifest = temp.path().join("native-package-manifest.json");

    let status = std::process::Command::new(python_command())
        .arg(&script)
        .args([
            "--root",
            stage.to_str().unwrap(),
            "--output",
            manifest.to_str().unwrap(),
            "--platform",
            "windows-x64",
            "--source-binary",
            source.to_str().unwrap(),
            "--staged-binary",
            "codex-plus-plus-manager.exe",
            "--forbid",
            "codex-plus-plus-manager-native",
        ])
        .status()
        .expect("run manifest generator");
    assert!(status.success());
    let contents = std::fs::read_to_string(&manifest).expect("read manifest");
    assert!(contents.contains("\"source_sha256\""));
    assert!(contents.contains("\"codex-plus-plus-manager.exe\""));
    assert!(!contents.contains(temp.path().to_str().unwrap()));

    std::fs::write(&staged, b"different-manager-binary").expect("change staged manager");
    let mismatch = std::process::Command::new(python_command())
        .arg(&script)
        .args([
            "--root",
            stage.to_str().unwrap(),
            "--output",
            temp.path().join("mismatch.json").to_str().unwrap(),
            "--platform",
            "windows-x64",
            "--source-binary",
            source.to_str().unwrap(),
            "--staged-binary",
            "codex-plus-plus-manager.exe",
        ])
        .status()
        .expect("run mismatch manifest generator");
    assert!(!mismatch.success());
}

#[test]
fn pull_request_ci_checks_formatting_and_stable_manager() {
    let pr = read(repository_root().join(".github/workflows/pr-build.yml"));

    assert!(pr.contains("cargo fmt --all -- --check"));
    assert!(pr.contains(SCOPED_CLIPPY_COMMAND));
    assert!(pr.contains("cargo build -p codex-plus-launcher -p codex-plus-manager"));
}
