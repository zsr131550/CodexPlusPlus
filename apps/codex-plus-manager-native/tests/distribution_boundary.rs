use std::path::{Path, PathBuf};

const SCOPED_CLIPPY_COMMAND: &str = "cargo clippy -p codex-plus-manager-service -p codex-plus-manager-native --all-targets --no-deps -- -D warnings";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("native app lives under repository/apps")
        .to_path_buf()
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn python_command() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

#[test]
fn native_manager_is_staged_under_the_stable_contract() {
    let root = repository_root();
    let pr = read(root.join(".github/workflows/pr-build.yml"));
    let release = read(root.join(".github/workflows/release-assets.yml"));
    let nsis = read(root.join("scripts/installer/windows/CodexPlusPlus.nsi"));
    let dmg = read(root.join("scripts/installer/macos/package-dmg.sh"));
    let windows_package_job = pr
        .split_once("  windows-artifacts:")
        .unwrap()
        .1
        .split_once("  oracle-validation:")
        .unwrap()
        .0;
    let macos_package_job = pr.split_once("  macos-dmg:").unwrap().1;

    assert!(pr.contains("cargo test --workspace --exclude codex-plus-manager"));
    assert!(
        pr.contains("cargo build -p codex-plus-launcher -p codex-plus-manager-native --release")
    );
    assert!(pr.contains("codex-plus-plus-manager-native.exe"));
    assert!(pr.contains("codex-plus-plus-manager.exe"));
    assert!(
        release
            .contains("cargo build -p codex-plus-launcher -p codex-plus-manager-native --release")
    );
    assert!(release.contains("codex-plus-plus-manager-native.exe"));
    assert!(release.contains("codex-plus-plus-manager.exe"));

    for (name, content) in [
        ("Windows package job", windows_package_job),
        ("macOS package job", macos_package_job),
        ("release workflow", release.as_str()),
    ] {
        assert!(
            !content.contains("npm run vite:build"),
            "{name} must not build the WebView frontend"
        );
        assert!(
            !content.contains("target/release/codex-plus-plus-manager.exe"),
            "{name} must not copy the legacy Tauri output"
        );
        assert!(
            !content.contains("apps/codex-plus-manager/src-tauri"),
            "{name} must not consume Tauri packaging assets"
        );
    }
    assert!(pr.contains("oracle-validation:"));
    assert!(pr.contains("npm.cmd run vite:build"));
    assert!(pr.contains("cargo test -p codex-plus-manager --jobs 1"));

    assert!(nsis.contains("apps\\codex-plus-manager-native\\assets\\packaging\\icon.ico"));
    assert!(nsis.contains("SetShellVarContext current"));
    assert!(nsis.contains("Software\\Classes\\codexplusplus"));
    assert!(nsis.contains("CodexPlusPlus"));
    assert!(dmg.contains("apps/codex-plus-manager-native/assets/packaging/icon.png"));
    assert!(dmg.contains("NATIVE_BINARY"));
    assert!(dmg.contains("CodexPlusPlusManager"));
    assert!(!dmg.contains("apps/codex-plus-manager/src-tauri"));
}

#[test]
fn package_manifest_contract_records_native_hashes_without_private_paths() {
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
fn package_workflows_upload_bounded_manifests_and_cannot_publish_from_prs() {
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
    assert!(pr.contains("Environment+SpecialFolder]::UserProfile"));
    assert!(pr.contains(
        "PREVIOUS_SHA256: 40e7603223a0e8fef43d546f94ad594a3f3a97717ef01d31401edb5ce86e62ef"
    ));
    assert!(!pr.contains("dist/windows/app/native-package-manifest"));
    assert!(!pr.contains("softprops/action-gh-release"));
    assert!(release.contains("generate-package-manifest.py"));
    assert!(release.contains("if: github.event_name == 'release'"));
    assert!(release.contains("latest-json:"));
    assert!(!release.contains("node <<"));
    assert!(!nsis.contains("native-package-manifest"));
    assert!(lifecycle.contains("verify_native_tree"));
    assert!(lifecycle.contains("previous_sha256"));
    assert!(!lifecycle.contains("latest"));
    assert!(probe.contains("CODEX_PLUS_PACKAGE_DISPOSABLE_PROFILE"));
    assert!(probe.contains("CODEX_PLUS_PACKAGE_WINDOWS_PROFILE"));
    assert!(probe.contains("manager.provider_import_url.pending"));
    assert!(probe.contains("MAX_STATE_EVIDENCE_BYTES"));
    assert!(probe.contains("process.pid"));
}

#[test]
fn package_manifest_hashes_staged_files_and_rejects_mismatch() {
    let root = repository_root();
    let script = root.join("scripts/installer/generate-package-manifest.py");
    let temp = tempfile::tempdir().expect("create manifest fixture");
    let stage = temp.path().join("stage");
    std::fs::create_dir_all(&stage).expect("create stage");
    let source = temp.path().join("native-source.bin");
    let staged = stage.join("codex-plus-plus-manager.exe");
    std::fs::write(&source, b"native-manager-binary").expect("write source");
    std::fs::write(&staged, b"native-manager-binary").expect("write staged manager");
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
fn pull_request_ci_checks_formatting_and_new_rust_crates() {
    let pr = read(repository_root().join(".github/workflows/pr-build.yml"));

    assert!(pr.contains("cargo fmt --all -- --check"));
    assert!(pr.contains(SCOPED_CLIPPY_COMMAND));
}
