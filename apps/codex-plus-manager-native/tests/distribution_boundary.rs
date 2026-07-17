use std::path::{Path, PathBuf};

const NATIVE_BINARY_NAME: &str = "codex-plus-plus-manager-native";
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

#[test]
fn native_manager_is_compiled_but_not_staged_or_released() {
    let root = repository_root();
    let pr = read(root.join(".github/workflows/pr-build.yml"));
    let release = read(root.join(".github/workflows/release-assets.yml"));
    let nsis = read(root.join("scripts/installer/windows/CodexPlusPlus.nsi"));
    let dmg = read(root.join("scripts/installer/macos/package-dmg.sh"));

    assert!(pr.contains("cargo test --workspace"));
    assert!(pr.contains("cargo build --release"));
    for (name, content) in [
        ("PR workflow", &pr),
        ("release workflow", &release),
        ("NSIS installer", &nsis),
        ("macOS packager", &dmg),
    ] {
        assert!(
            !content.contains(NATIVE_BINARY_NAME),
            "{name} must not distribute the native manager"
        );
    }
}

#[test]
fn pull_request_ci_checks_formatting_and_new_rust_crates() {
    let pr = read(repository_root().join(".github/workflows/pr-build.yml"));

    assert!(pr.contains("cargo fmt --all -- --check"));
    assert!(pr.contains(SCOPED_CLIPPY_COMMAND));
}
