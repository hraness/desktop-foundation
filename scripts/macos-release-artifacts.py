#!/usr/bin/env python3
"""Exact-source and immutable-artifact admission. No Apple credentials or payload execution."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("signing", Path(__file__).with_name("sign-macos-release.py"))
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)
require = signing.require
REPOSITORY = "hraness/desktop-foundation"


def command(args):
    result = subprocess.run(args, capture_output=True, timeout=180, check=False)
    require(result.returncode == 0, "release authority command failed")
    require(len(result.stdout) <= 1024 * 1024, "release authority response too large")
    return result.stdout.decode("utf-8").strip()


def context():
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY, "unexpected repository")
    sha = os.environ.get("GITHUB_SHA", "")
    require(re.fullmatch(r"[a-f0-9]{40}", sha), "invalid source SHA")
    for name in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        require(re.fullmatch(r"[1-9][0-9]*", os.environ.get(name, "")), "invalid workflow run identity")
    return sha


def qualify():
    sha = context()
    version = signing.version_value(json.loads(Path("package.json").read_text())["version"])
    require(tuple(map(int, version.split("."))) >= (1, 1, 3), "first signed release is 1.1.3")
    require(os.environ.get("GITHUB_EVENT_NAME") == "push" and os.environ.get("GITHUB_REF_TYPE") == "tag"
            and os.environ.get("GITHUB_REF") == "refs/tags/v" + version
            and os.environ.get("GITHUB_REF_NAME") == "v" + version, "expected exact version tag push")
    require(command(["git", "rev-parse", "HEAD"]) == sha, "checkout source mismatch")
    # Fetch only governed main, retaining ancestry rather than importing tags
    # or unrelated concurrent branches. No credential is available here.
    args = ["git", "fetch", "--no-tags"]
    if command(["git", "rev-parse", "--is-shallow-repository"]) == "true":
        args.append("--unshallow")
    command(args + ["origin", "refs/heads/main:refs/remotes/origin/main"])
    command(["git", "merge-base", "--is-ancestor", sha, "refs/remotes/origin/main"])
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"sha={sha}\ntag=v{version}\n")


def verify_tag():
    sha = context()
    version = signing.version_value(json.loads(Path('package.json').read_text())['version'])
    tag = 'refs/tags/v' + version
    require(os.environ.get('GITHUB_REF') == tag, 'release tag mismatch')
    rows = command(['git', 'ls-remote', '--exit-code', 'origin', tag, tag + '^{}']).splitlines()
    refs = {}
    for row in rows:
        fields = row.split('\t')
        require(len(fields) == 2 and fields[1] in (tag, tag + '^{}') and fields[1] not in refs
                and re.fullmatch(r'[a-f0-9]{40}', fields[0]), 'unexpected remote tag response')
        refs[fields[1]] = fields[0]
    require(tag in refs and refs.get(tag + '^{}', refs[tag]) == sha, 'remote release tag moved to another commit')


def api(path):
    return json.loads(command(["gh", "api", f"repos/{REPOSITORY}/{path}"]))


def admit_metadata(metadata, name, sha, expected_id=None, expected_digest=None):
    artifact_id = metadata.get("id")
    digest = metadata.get("digest", "")
    require(type(artifact_id) is int and artifact_id > 0 and metadata.get("name") == name,
            "artifact identity mismatch")
    require(metadata.get("expired") is False and 0 < metadata.get("size_in_bytes", 0) <= signing.MAX_BYTES * 5,
            "artifact expired or unbounded")
    require(re.fullmatch(r"sha256:[a-f0-9]{64}", digest), "artifact digest missing")
    run = metadata.get("workflow_run", {})
    require(str(run.get("id")) == os.environ["GITHUB_RUN_ID"] and run.get("head_sha") == sha,
            "artifact belongs to another workflow run or source")
    require(expected_id is None or str(artifact_id) == expected_id, "signing output artifact ID mismatch")
    require(expected_digest is None or digest == "sha256:" + expected_digest, "signing output artifact digest mismatch")
    return artifact_id, digest[7:]


def fetch_artifact(metadata, name, names, destination, sha, expected_id=None, expected_digest=None):
    artifact_id, digest = admit_metadata(metadata, name, sha, expected_id, expected_digest)
    with tempfile.TemporaryDirectory(prefix="desktop-foundation-artifact-", dir=os.environ["RUNNER_TEMP"]) as temp:
        archive = Path(temp) / "artifact.zip"
        with archive.open("xb") as output:
            result = subprocess.run(["gh", "api", f"repos/{REPOSITORY}/actions/artifacts/{artifact_id}/zip"],
                                    stdout=output, stderr=subprocess.PIPE, timeout=180, check=False)
        require(result.returncode == 0, "artifact download failed")
        signing.unpack_artifact(archive, digest, names, destination)


def fetch_native(destination):
    sha = context()
    require(not destination.exists(), "native staging already exists")
    listing = api(f"actions/runs/{os.environ['GITHUB_RUN_ID']}/artifacts?per_page=100")
    require(listing.get("total_count", 101) <= 100, "artifact inventory exceeds one page")
    destination.mkdir(mode=0o700)
    for target in signing.TARGETS:
        name = f"native-{target}-{os.environ['GITHUB_RUN_ATTEMPT']}"
        matches = [a for a in listing.get("artifacts", []) if a.get("name") == name]
        require(len(matches) == 1, "native artifact is missing or ambiguous")
        metadata = api(f"actions/artifacts/{matches[0]['id']}")
        stage = destination / target
        fetch_artifact(metadata, name, signing.native_names(target), stage, sha)
        for path in stage.iterdir():
            path.rename(destination / path.name)
        stage.rmdir()


def check_signed(directory, version, sha):
    receipt = json.loads(signing.regular_file(directory / "macos-signing.json", 65536))
    require(receipt.get("sourceCommit") == sha and receipt.get("version") == version
            and receipt.get("teamId") == signing.TEAM_ID and receipt.get("state") == "verified"
            and receipt.get("status") == "Accepted", "signed receipt identity mismatch")
    expected = {f"{stem}-{target}": identifier for target in signing.TARGETS
                for stem, identifier in signing.IDENTIFIERS.items()}
    require(receipt.get("identifiers") == expected and set(receipt.get("signedBinarySha256", {})) == set(expected),
            "signed receipt binary inventory mismatch")
    for name in expected:
        require(signing.digest(signing.regular_file(directory / name)) == receipt["signedBinarySha256"][name],
                "signed binary digest mismatch")


def fetch_signed(destination):
    sha = context()
    artifact_id, digest = os.environ.get("SIGNED_ARTIFACT_ID", ""), os.environ.get("SIGNED_ARTIFACT_DIGEST", "")
    require(re.fullmatch(r"[1-9][0-9]*", artifact_id) and re.fullmatch(r"[a-f0-9]{64}", digest),
            "missing exact signing job outputs")
    name = f"macos-signed-{os.environ['GITHUB_RUN_ATTEMPT']}"
    require(destination.is_dir() and not destination.is_symlink(), "unsafe package artifact destination")
    with tempfile.TemporaryDirectory(prefix="desktop-foundation-signed-", dir=os.environ["RUNNER_TEMP"]) as temp:
        stage = Path(temp) / "signed"
        fetch_artifact(api(f"actions/artifacts/{artifact_id}"), name, signing.signed_names(), stage, sha, artifact_id, digest)
        version = signing.version_value(json.loads(Path("package.json").read_text())["version"])
        check_signed(stage, version, sha)
        for name in signing.signed_names():
            target = destination / name
            require(not target.is_symlink(), "unsafe package artifact path")
            shutil.copyfile(stage / name, target)
            target.chmod(0o755 if name in signing.asset_names() else 0o644)
        check_signed(destination, version, sha)


def verify_stage(directory):
    sha = context()
    expected = os.environ.get('SIGNED_RECEIPT_SHA256', '')
    require(re.fullmatch(r'[a-f0-9]{64}', expected), 'missing signing-job receipt digest')
    require(signing.digest(signing.regular_file(directory / 'macos-signing.json', 65536)) == expected,
            'signing receipt changed after signing job')
    version = signing.version_value(json.loads(Path('package.json').read_text())['version'])
    check_signed(directory, version, sha)


def fetch_distribution(destination):
    sha = context()
    workflow_sha = os.environ.get('ARTIFACT_WORKFLOW_SHA', sha)
    require(re.fullmatch(r'[a-f0-9]{40}', workflow_sha), 'invalid artifact workflow SHA')
    artifact_id = os.environ.get('DISTRIBUTION_ARTIFACT_ID', '')
    digest = os.environ.get('DISTRIBUTION_ARTIFACT_DIGEST', '')
    require(re.fullmatch(r'[1-9][0-9]*', artifact_id) and re.fullmatch(r'[a-f0-9]{64}', digest),
            'missing immutable package artifact identity')
    version = signing.version_value(json.loads(Path('package.json').read_text())['version'])
    targets = (*signing.TARGETS, 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
               'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu')
    names = {f'{stem}-{target}' + ('.exe' if 'windows' in target else '')
             for target in targets for stem in signing.IDENTIFIERS}
    names |= {f'hraness-companion-{target}.evidence.json' for target in targets}
    names |= {'release-manifest.json', 'SHA256SUMS', f'hraness-desktop-foundation-{version}.tgz'}
    if os.environ.get('GITHUB_EVENT_NAME') == 'push' and os.environ.get('GITHUB_REF_TYPE') == 'tag':
        names.add('macos-signing.json')
    name = f'distribution-{os.environ["GITHUB_RUN_ATTEMPT"]}'
    fetch_artifact(api(f'actions/artifacts/{artifact_id}'), name, names, destination, workflow_sha, artifact_id, digest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["qualify", "verify-tag", "fetch-native", "fetch-signed", "verify-stage", "fetch-distribution"])
    parser.add_argument("destination", type=Path, nargs="?")
    args = parser.parse_args()
    if args.command in ("qualify", "verify-tag"):
        (qualify if args.command == "qualify" else verify_tag)()
    else:
        require(args.destination is not None, "destination required")
        {'fetch-native': fetch_native, 'fetch-signed': fetch_signed, 'verify-stage': verify_stage, 'fetch-distribution': fetch_distribution}[args.command](args.destination)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        message = str(error) if isinstance(error, signing.SigningError) else type(error).__name__
        raise SystemExit("error: " + message) from None
