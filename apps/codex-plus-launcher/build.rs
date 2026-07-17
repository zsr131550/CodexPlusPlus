fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../codex-plus-manager/src-tauri/icons/icon.ico");
        let release_manifest =
            include_str!("../codex-plus-manager/src-tauri/windows-app-manifest.xml");
        let development_manifest;
        let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") {
            release_manifest
        } else {
            development_manifest = release_manifest.replace("requireAdministrator", "asInvoker");
            &development_manifest
        };
        resource.set_manifest(manifest);
        resource.compile().expect("compile launcher icon resource");
    }
}
