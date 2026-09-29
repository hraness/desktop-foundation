fn main() {
    // Embeds the Common Controls v6 manifest in every Windows MSVC
    // executable, tests included: the notice task dialog and the prompt
    // need it. Nothing else runs here: no bundler, signer, or notarization.
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest.xml");
        println!("cargo:rerun-if-changed=windows.manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
