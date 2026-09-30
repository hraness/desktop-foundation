#!/usr/bin/env python3
"""Dependency-free, non-executing boundary for Developer ID release signing.

Only this program receives Apple credentials. It never executes the payload,
installs dependencies, or invokes build scripts. Build and native tests run in separate jobs. Final packaging consumes only
the exact signed artifacts after credential cleanup.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import stat
import struct
import subprocess
import sys
import tempfile
import zipfile

TEAM_ID = "8AAP53VTW3"
IDENTIFIERS = {"hraness-helper": "dev.hraness.desktop-foundation.helper",
               "hraness-companion": "dev.hraness.desktop-foundation.companion"}
TARGETS = {"aarch64-apple-darwin": 0x0100000C, "x86_64-apple-darwin": 0x01000007}
MAX_BYTES = 128 * 1024 * 1024
UUID_PATTERN = r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
SECRET_NAMES = (
    "APPLE_DEVELOPER_ID_P12_BASE64", "APPLE_DEVELOPER_ID_P12_PASSWORD",
    "APPLE_NOTARY_KEY_P8_BASE64", "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID",
)


class SigningError(Exception):
    """A controlled diagnostic that never contains subprocess output."""


def require(condition, message):
    if not condition:
        raise SigningError(message)


def version_value(value):
    value = value.removeprefix("v")
    require(re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value),
            "version must be stable semantic version")
    return value


def asset_names():
    return {f"{stem}-{target}" for target in TARGETS for stem in IDENTIFIERS}


def native_names(target):
    require(target in TARGETS, "unsupported signing target")
    return {f"{stem}-{target}" for stem in IDENTIFIERS} | {f"hraness-companion-{target}.evidence.json"}


def signed_names():
    return asset_names() | {f"hraness-companion-{target}.evidence.json" for target in TARGETS} | {"macos-signing.json"}


def regular_file(path, maximum=MAX_BYTES):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and 0 < info.st_size <= maximum,
            "input must be one bounded regular file")
    return path.read_bytes()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unpack_artifact(archive, expected_digest, expected_names, destination):
    """Bind GitHub's immutable artifact ZIP digest, then extract exact entries."""
    require(re.fullmatch(r"[0-9a-f]{64}", expected_digest), "invalid artifact digest")
    require(digest(regular_file(archive, MAX_BYTES * 5)) == expected_digest, "artifact ZIP digest mismatch")
    with zipfile.ZipFile(archive) as source:
        entries = source.infolist()
        require(len(entries) == len(expected_names) and {e.filename for e in entries} == expected_names,
                "artifact ZIP has unexpected or duplicate entries")
        for entry in entries:
            mode = entry.external_attr >> 16
            require(not entry.is_dir() and stat.S_IFMT(mode) in (0, stat.S_IFREG),
                    "artifact ZIP entry is not regular")
            maximum = 65536 if entry.filename.endswith('.json') else MAX_BYTES
            require(0 < entry.file_size <= maximum and not entry.flag_bits & 1,
                    "artifact ZIP entry is not bounded plaintext")
        destination.mkdir(mode=0o700)
        for entry in entries:
            with (destination / entry.filename).open("xb") as output:
                os.chmod(output.name, 0o600)
                maximum = 65536 if entry.filename.endswith('.json') else MAX_BYTES
                with source.open(entry) as stream:
                    data = stream.read(maximum + 1)
                require(len(data) == entry.file_size and len(data) <= maximum, "ZIP payload length mismatch")
                output.write(data)


def prepare_binaries(inputs, work):
    expected = set().union(*(native_names(target) for target in TARGETS))
    require(inputs.is_dir() and not inputs.is_symlink() and {p.name for p in inputs.iterdir()} == expected,
            "native input directory does not contain exactly the Mac build artifacts")
    binaries = []
    for target, cpu in TARGETS.items():
        evidence = json.loads(regular_file(inputs / f"hraness-companion-{target}.evidence.json", 65536))
        require(evidence.get("target") == target and evidence.get("sourceCommit") == os.environ.get("GITHUB_SHA"),
                "native evidence belongs to another source or target")
        for stem, identifier in IDENTIFIERS.items():
            name = f"{stem}-{target}"
            contents = regular_file(inputs / name)
            require(len(contents) >= 32 and struct.unpack("<II", contents[:8]) == (0xFEEDFACF, cpu)
                    and struct.unpack("<I", contents[12:16])[0] == 2,
                    "payload must be the expected thin Mach-O executable")
            binary = work / name
            private_file(binary, contents)
            binary.chmod(0o755)
            binaries.append((binary, identifier))
    return binaries


def apple_requirement(identifier):
    require(identifier in IDENTIFIERS.values(), "unexpected signing identifier")
    require(re.fullmatch(r"[A-Z0-9]{10}", TEAM_ID), "Apple team ID is not configured")
    return (f'identifier "{identifier}" and anchor apple generic '
            'and certificate 1[field.1.2.840.113635.100.6.2.6] exists '
            'and certificate leaf[field.1.2.840.113635.100.6.1.13] exists '
            f'and certificate leaf[subject.OU] = "{TEAM_ID}"')


def run(command, timeout=60):
    # Apple tools need HOME for standard system services, never the caller's
    # credentials, provider environment, or arbitrary command search path.
    environment = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": os.environ["HOME"], "LC_ALL": "C"}
    try:
        result = subprocess.run([str(part) for part in command], capture_output=True,
                                text=True, timeout=timeout, env=environment, check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise RuntimeError(f"Apple tool failed or timed out: {Path(command[0]).name}") from None
    # Do not include argv or output: security takes passphrases on its command
    # line, and service errors can echo authentication inputs.
    require(result.returncode == 0, f"Apple tool failed: {Path(command[0]).name}")
    return result.stdout + result.stderr


def private_file(path, data):
    with path.open("xb") as output:
        path.chmod(0o600)
        output.write(data)


def cleanup_credentials(work):
    credentials = work / "credentials"
    if not credentials.exists():
        return
    require(credentials.is_dir() and not credentials.is_symlink(), "unsafe credential directory")
    keychain = credentials / "signing.keychain-db"
    # Never add this keychain to the user's search list. Delete through the
    # supported API before removing the exact private directory.
    try:
        if keychain.exists():
            run(["/usr/bin/security", "delete-keychain", keychain])
    finally:
        shutil.rmtree(credentials)


def checked_work(path):
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    require(path == runner_temp / "desktop-foundation-apple-signing" and not path.is_symlink(),
            "signing work directory must be the dedicated runner temporary path")
    return path


def cleanup(work):
    checked_work(work)
    if work.exists():
        require(work.is_dir(), "unsafe signing work directory")
        cleanup_credentials(work)
        shutil.rmtree(work)


def diagnostic(path, receipt):
    # This small allowlisted receipt survives credential/work cleanup. Atomic
    # replacement preserves the latest known submission ID even on TERM.
    require(not path.is_symlink(), "unsafe notarization diagnostic path")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf8", dir=path.parent,
                                         prefix=".desktop-foundation-notarization-", delete=False) as output:
            temporary = Path(output.name)
            json.dump(receipt, output, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        # Close first: Windows also runs the mocked signing tests and refuses
        # to replace an open temporary file. Replacement stays atomic.
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def sign(inputs, version, output, work):
    require(tuple(map(int, version_value(version).split("."))) >= (1, 1, 3), "first signed version is 1.1.3")
    checked_work(work)
    require(sys.platform == "darwin", "Developer ID signing requires macOS")
    require(not output.exists(), "final output directory already exists")
    values = {name: os.environ.pop(name, "") for name in SECRET_NAMES}
    require(all(values.values()), "Apple signing credentials are incomplete")
    require(re.fullmatch(r"[A-Z0-9]{10}", values["APPLE_NOTARY_KEY_ID"]), "invalid notary key ID")
    require(re.fullmatch(UUID_PATTERN, values["APPLE_NOTARY_ISSUER_ID"]), "invalid notary issuer ID")
    receipt_path = work.with_name("desktop-foundation-apple-notarization.json")
    require(not receipt_path.exists() and not receipt_path.is_symlink(), "notarization diagnostic already exists")
    work.mkdir(mode=0o700)
    try:
        binaries = prepare_binaries(inputs, work)
        credentials = work / "credentials"
        credentials.mkdir(mode=0o700)
        keychain = credentials / "signing.keychain-db"
        p12 = credentials / "identity.p12"
        key = credentials / "AuthKey.p8"
        private_file(p12, base64.b64decode(values["APPLE_DEVELOPER_ID_P12_BASE64"], validate=True))
        private_file(key, base64.b64decode(values["APPLE_NOTARY_KEY_P8_BASE64"], validate=True))
        password = secrets.token_hex(32)
        try:
            run(["/usr/bin/security", "create-keychain", "-p", password, keychain])
            keychain.chmod(0o600)
            run(["/usr/bin/security", "set-keychain-settings", "-lut", "21600", keychain])
            run(["/usr/bin/security", "unlock-keychain", "-p", password, keychain])
            run(["/usr/bin/security", "import", p12, "-k", keychain,
                 "-P", values["APPLE_DEVELOPER_ID_P12_PASSWORD"], "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"])
            run(["/usr/bin/security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
                 "-s", "-k", password, keychain])
            identities = run(["/usr/bin/security", "find-identity", "-v", "-p", "codesigning", keychain])
            matches = re.findall(r'\b([0-9A-Fa-f]{40}) "Developer ID Application: [^"\n]+ \(' + TEAM_ID + r'\)"', identities)
            require(len(matches) == 1, "keychain must contain exactly one expected Developer ID Application identity")
            unsigned_hashes = {binary.name: digest(regular_file(binary)) for binary, _ in binaries}
            for binary, identifier in binaries:
                requirement = apple_requirement(identifier)
                run(["/usr/bin/codesign", "--force", "--sign", matches[0], "--keychain", keychain,
                     "--identifier", identifier, "--options", "runtime", "--timestamp",
                     "--requirements", "designated => " + requirement, binary], timeout=180)
                run(["/usr/bin/codesign", "--verify", "--strict", "--test-requirement", requirement, binary])
                metadata = run(["/usr/bin/codesign", "--display", "--verbose=4", binary])
                require(f"Identifier={identifier}\n" in metadata and f"TeamIdentifier={TEAM_ID}\n" in metadata,
                        "signed binary identity mismatch")
                require(re.search(r"^CodeDirectory .*flags=.*\(.*runtime.*\)", metadata, re.M)
                        and re.search(r"^Timestamp=.+", metadata, re.M), "signature needs hardened runtime and secure timestamp")
            submitted = work / "notarization.zip"
            with zipfile.ZipFile(submitted, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for binary, _ in binaries:
                    bundle.write(binary, binary.name)
            receipt = {
                "schemaVersion": 1, "version": version_value(version), "teamId": TEAM_ID,
                "submissionId": None, "state": "submission-started", "status": None,
                "sourceCommit": os.environ["GITHUB_SHA"], "unsignedBinarySha256": unsigned_hashes,
                "identifiers": {binary.name: identifier for binary, identifier in binaries},
                "signedBinarySha256": {binary.name: digest(regular_file(binary)) for binary, _ in binaries},
                "submissionZipSha256": digest(regular_file(submitted, MAX_BYTES * 5)),
            }
            diagnostic(receipt_path, receipt)
            authentication = ["--key", key, "--key-id", values["APPLE_NOTARY_KEY_ID"],
                              "--issuer", values["APPLE_NOTARY_ISSUER_ID"], "--output-format", "json"]
            # Separate upload and wait so a first-time Apple review can time
            # out without losing the UUID. Never automatically resubmit.
            submission = json.loads(run(["/usr/bin/xcrun", "notarytool", "submit", submitted,
                                        *authentication], timeout=180))
            submission_id = str(submission.get("id", ""))
            require(re.fullmatch(UUID_PATTERN, submission_id), "missing notarization submission ID")
            receipt.update(submissionId=submission_id, state="submitted")
            diagnostic(receipt_path, receipt)
            print(f"Apple notarization submission {submission_id}; receipt desktop-foundation-apple-notarization.json", flush=True)
            try:
                response = json.loads(run(["/usr/bin/xcrun", "notarytool", "wait", submission_id,
                                          *authentication, "--timeout", "15m"], timeout=960))
                require(response.get("id") == submission_id, "notarization result is for another submission")
            except BaseException:
                receipt["state"] = "wait-incomplete"
                diagnostic(receipt_path, receipt)
                raise
            # Do not copy arbitrary service response strings or logs into the
            # receipt. Only these public status values are retained.
            status = response.get("status")
            receipt.update(state="wait-complete", status=status if status in (
                "Accepted", "Invalid", "Rejected", "In Progress") else "Unrecognized")
            diagnostic(receipt_path, receipt)
            require(status == "Accepted", "Apple notarization was not Accepted")
            # Raw executables cannot carry stapled tickets. Verify Apple's
            # online notarization recognition without executing any payload.
            for binary, identifier in binaries:
                run(["/usr/bin/codesign", "--verify", "--strict", "--check-notarization",
                     "--test-requirement", apple_requirement(identifier), binary], timeout=180)
            receipt["state"] = "verified"
            diagnostic(receipt_path, receipt)
        finally:
            values.clear()
            password = ""
            cleanup_credentials(work)
        # Credential removal precedes final packaging and the later workflow
        # smoke step. Neither packaging nor signing executes the payload.
        output.mkdir(mode=0o700)
        for binary, _ in binaries:
            require(digest(regular_file(binary)) == receipt["signedBinarySha256"][binary.name],
                    "signed bytes changed after notarization")
            shutil.copyfile(binary, output / binary.name)
            (output / binary.name).chmod(0o755)
        for target in TARGETS:
            name = f"hraness-companion-{target}.evidence.json"
            evidence = json.loads(regular_file(inputs / name, 65536))
            evidence.update(signing="developer-id", notarization="Accepted", teamId=TEAM_ID,
                            signedBinarySha256={name: value for name, value in receipt["signedBinarySha256"].items()
                                               if name.endswith(target)},
                            identifiers={name: value for name, value in receipt["identifiers"].items()
                                         if name.endswith(target)})
            (output / name).write_text(json.dumps(evidence, sort_keys=True) + "\n")
        (output / "macos-signing.json").write_text(json.dumps(receipt, sort_keys=True) + "\n")
        print(f"Signed and notarized four Mac binaries; submission {response['id']}")
    finally:
        cleanup(work)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    signing = commands.add_parser("sign")
    signing.add_argument("inputs", type=Path)
    signing.add_argument("version", type=version_value)
    signing.add_argument("output", type=Path)
    signing.add_argument("work", type=Path)
    cleaning = commands.add_parser("cleanup")
    cleaning.add_argument("work", type=Path)
    args = parser.parse_args()
    # GitHub cancellation sends TERM before KILL; unwind finally blocks while
    # possible. The workflow also has a separate always() cleanup step.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    os.umask(0o077)
    try:
        if args.command == "sign":
            sign(args.inputs, args.version, args.output, args.work)
        else:
            cleanup(args.work)
    except Exception as error:
        # Only local controlled errors may reach logs, never Apple tool output.
        message = str(error) if isinstance(error, (SigningError, RuntimeError)) else type(error).__name__
        print(f"error: {message}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
