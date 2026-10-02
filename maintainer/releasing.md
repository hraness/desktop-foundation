# Validation and releases


Run `npm run check` on macOS: the SDK build and tests, `cargo test --locked
--release` and a release build of both executables. Tests use private
temporary homes, fake `security` and `codesign` tools, and never touch the
real login keychain, LaunchAgents or login items.

Release from the reviewed, validated tree: bump `package.json`, the Cargo
versions (workspace and crates) and the CHANGELOG section, merge, then push
an annotated immutable `v*` tag. The tag workflow builds the six targets,
the package and `SHA256SUMS`, attests every asset and creates the GitHub
Release. Never move existing tags. Consumers update their tgz URL or Git
tag and their lockfile.

Pushing the tag by hand is optional: once the Companion run on `main` passes
for a commit that bumps `package.json`, the Tag release workflow creates the
annotated `v<version>` tag on that commit with the `hraness-release-tagger`
App. A manually pushed tag still works.
