fn main() {
    // Only embeds executable metadata. The release artifact is cargo's binary;
    // this never invokes the Tauri bundler, a signer, or a notarization service.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // tauri-winres/embed-resource attach their manifest only to binary targets.
        // Our library test executable also imports Common Controls v6 functions.
        // Let the linker embed one manifest in every executable, including tests;
        // retain Tauri's icon/version resources without a second manifest resource.
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("failed to generate Windows executable metadata");
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest.xml");
        println!("cargo:rerun-if-changed=windows.manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    } else {
        tauri_build::build();
    }
}
