fn main() {
    let release_manifest = include_str!("windows-app-manifest.xml");
    let development_manifest;
    let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") {
        release_manifest
    } else {
        development_manifest = release_manifest.replace("requireAdministrator", "asInvoker");
        &development_manifest
    };
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run Tauri build script");
}
