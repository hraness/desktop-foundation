fn main() {
    // Only embeds executable metadata. The release artifact is cargo's binary;
    // this never invokes the Tauri bundler, a signer, or a notarization service.
    tauri_build::build();
}
