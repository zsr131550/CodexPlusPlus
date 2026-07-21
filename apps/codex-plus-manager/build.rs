fn main() {
    #[cfg(windows)]
    {
        const PERF_AS_INVOKER_ENV: &str = "CODEX_PLUS_MANAGER_PERF_AS_INVOKER";
        println!("cargo:rerun-if-env-changed=CODEX_PLUS_MANAGER_PERF_AS_INVOKER");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/packaging/icon.ico");
        let release_manifest = include_str!("assets/packaging/windows-app-manifest.xml");
        let development_manifest;
        let perf_as_invoker = std::env::var(PERF_AS_INVOKER_ENV).as_deref() == Ok("1");
        let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") && !perf_as_invoker {
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
