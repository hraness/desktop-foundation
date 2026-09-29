# Capabilities

This crate grants no Tauri capabilities, so this folder holds no capability files.

It exists because `tauri-build` watches `capabilities/`. When the folder is
missing, cargo reruns the build script on every build, and that rebuilds the
crate even when nothing changed. Tauri reads only `.json`, `.toml` and `.json5`
files here, so this README is ignored.
