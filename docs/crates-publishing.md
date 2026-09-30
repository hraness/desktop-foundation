# crates.io publishing

`hraness-cli-kit` is the independently publishable Rust package. The desktop
helper and the other workspace crates keep their existing release channels.

After the existing `companion.yml` checks and GitHub Release succeed, its
crates.io job checks the tag against both package versions and confirms that
the source belongs to the default branch. It publishes with GitHub OIDC,
refuses older registry versions, leaves existing versions unchanged, and checks
the registry afterward. The temporary credential is revoked when the job ends.

The job is disabled until the one-time registry setup is complete:

1. From the verified `v1.1.2` release tag, publish `hraness-cli-kit` using
   `cargo publish --locked -p hraness-cli-kit` and the registry's
   initial-publication authentication.
2. In the crate's settings, add a GitHub trusted publisher: owner `hraness`,
   repository `desktop-foundation`, workflow `companion.yml`, no environment.
3. Enable `CRATES_IO_PUBLISH` only after the crate and trusted publisher exist:
   `gh variable set CRATES_IO_PUBLISH --repo hraness/desktop-foundation --body true`.

Keep account two-factor authentication enabled. Do not add a stored publication
token or an approval environment. Future version tags publish after the release
checks without a recurring sign-in.
