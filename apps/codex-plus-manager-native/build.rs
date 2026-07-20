fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/packaging/icon.ico");
        let release_manifest = include_str!("assets/packaging/windows-app-manifest.xml");
        let development_manifest;
        let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") {
            release_manifest
        } else {
            development_manifest = release_manifest.replace("requireAdministrator", "asInvoker");
            &development_manifest
        };
        resource.set_manifest(manifest);
        resource
            .compile()
            .expect("compile native manager icon resource");
    }
}
