# Security

Report a vulnerability through GitHub's private vulnerability reporting on
this repository (Security, then Report a vulnerability). Please do not open a
public issue for it.

## What the companion and helper are

`hraness-companion` and `hraness-helper` are unsigned-by-publisher native
executables, checked against the SHA-256 digests in the release manifest
before first use and attested by GitHub build provenance. macOS helpers carry
an ad-hoc signature only and are not notarized. The operating system's own
trust controls apply to them; see [docs/installation.md](docs/installation.md).
They hold no privilege of their own: a product's daemon or owner remains the
authority for its state.

## Headless control and the human gate (0.9.0)

The owner process and its sockets are described in
[docs/control.md](docs/control.md); the gate in
[docs/human-gate.md](docs/human-gate.md). In short:

- The control directory is 0700 and every socket and file in it is 0600, so
  other users on the machine cannot reach the owner.
- The admin socket also needs the per-generation capability in `admin.cap`.
  This separates agent requests from admin requests; it does not keep out a
  process running as the same user, which can read the file.
- The human gate (T1 foreground terminal plus T2 one-time code on
  `/dev/tty`) stops an agent from approving a `decide` verb by accident or on
  instruction. It is not a boundary against a determined process running as
  the same user, which can drive a pseudo-terminal.
- OS owner authentication (T3) and peer-credential checks are deferred. Asking
  for T3 returns `unsupported-platform`; it never falls back to a weaker tier.
- Retiring an old login item renames it aside (`.retired-<ms>`) after checking
  that it is a regular file owned by the user, not a symlink, and launches the
  expected program. It never deletes a file.
