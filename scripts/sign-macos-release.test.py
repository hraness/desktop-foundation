#!/usr/bin/env python3
"""Credential-free behavioral checks. Apple tools and service responses are mocked."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("signing", Path(__file__).with_name("sign-macos-release.py"))
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)
ART = importlib.util.spec_from_file_location("artifacts", Path(__file__).with_name("macos-release-artifacts.py"))
artifacts = importlib.util.module_from_spec(ART)
ART.loader.exec_module(artifacts)
SHA = "a" * 40
UUID = "12345678-1234-1234-1234-123456789abc"


class SigningTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="desktop-foundation-signing-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.inputs = self.root / "inputs"
        self.inputs.mkdir()
        for target, cpu in signing.TARGETS.items():
            for stem in signing.IDENTIFIERS:
                (self.inputs / f"{stem}-{target}").write_bytes(struct.pack("<IIIIIIII", 0xFEEDFACF, cpu, 0, 2, 0, 0, 0, 0) + b"fixture-never-executed")
            (self.inputs / f"hraness-companion-{target}.evidence.json").write_text(json.dumps({"target": target, "sourceCommit": SHA}))
        self.output = self.root / "output"
        self.work = self.root / "desktop-foundation-apple-signing"
        self.calls = []
        self.status = "Accepted"
        self.failure = None
        env = {"HOME": str(self.root), "RUNNER_TEMP": str(self.root), "GITHUB_SHA": SHA,
               "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_REPOSITORY": artifacts.REPOSITORY,
               "APPLE_DEVELOPER_ID_P12_BASE64": base64.b64encode(b"fake-p12").decode(),
               "APPLE_DEVELOPER_ID_P12_PASSWORD": "never-print-this",
               "APPLE_NOTARY_KEY_P8_BASE64": base64.b64encode(b"fake-p8").decode(),
               "APPLE_NOTARY_KEY_ID": "ABCDE12345", "APPLE_NOTARY_ISSUER_ID": UUID}
        for active in (patch.dict(os.environ, env), patch.object(signing.sys, "platform", "darwin"),
                       patch.object(signing, "run", self.tool)):
            active.start()
            self.addCleanup(active.stop)

    def tool(self, args, timeout=60):
        args = list(map(str, args))
        self.calls.append(args)
        self.assertTrue(all(name not in os.environ for name in signing.SECRET_NAMES))
        if self.failure and self.failure in args:
            raise signing.SigningError("mock tool failure")
        if "create-keychain" in args:
            Path(args[-1]).touch(mode=0o600)
        if "find-identity" in args:
            return f'1) {"A" * 40} "Developer ID Application: Example ({signing.TEAM_ID})"'
        if "--sign" in args:
            binary = Path(args[-1])
            binary.write_bytes(binary.read_bytes() + b"signed")
        if "--display" in args:
            stem = "hraness-helper" if Path(args[-1]).name.startswith("hraness-helper-") else "hraness-companion"
            return f"Identifier={signing.IDENTIFIERS[stem]}\nTeamIdentifier={signing.TEAM_ID}\nCodeDirectory flags=0x10000(runtime) hashes=1\nTimestamp=valid timestamp\n"
        if "notarytool" in args:
            return json.dumps({"id": UUID, "status": self.status})
        return ""

    def sign(self):
        signing.sign(self.inputs, "1.1.3", self.output, self.work)

    def test_all_four_identities_are_signed_and_exact_final_bytes_bound(self):
        self.sign()
        self.assertEqual({p.name for p in self.output.iterdir()}, signing.signed_names())
        self.assertFalse(self.work.exists())
        signatures = [call for call in self.calls if "--sign" in call]
        self.assertEqual(len(signatures), 4)
        self.assertEqual({call[call.index("--identifier") + 1] for call in signatures}, set(signing.IDENTIFIERS.values()))
        self.assertTrue(all(call[0] in ("/usr/bin/security", "/usr/bin/codesign", "/usr/bin/xcrun") for call in self.calls))
        artifacts.check_signed(self.output, "1.1.3", SHA)
        binary = self.output / next(iter(signing.asset_names()))
        binary.write_bytes(binary.read_bytes() + b"tamper")
        with self.assertRaises(artifacts.signing.SigningError):
            artifacts.check_signed(self.output, "1.1.3", SHA)

    def test_rejected_notarization_preserves_receipt_and_never_publishes(self):
        self.status = "Invalid"
        with self.assertRaises(signing.SigningError):
            self.sign()
        self.assertFalse(self.work.exists())
        self.assertFalse(self.output.exists())
        receipt = json.loads((self.root / "desktop-foundation-apple-notarization.json").read_text())
        self.assertEqual(receipt["submissionId"], UUID)
        self.assertEqual(receipt["status"], "Invalid")

    def test_wait_failure_keeps_submission_without_resubmitting(self):
        self.failure = "wait"
        with self.assertRaises(signing.SigningError):
            self.sign()
        receipt = json.loads((self.root / "desktop-foundation-apple-notarization.json").read_text())
        self.assertEqual(receipt["state"], "wait-incomplete")
        self.assertEqual(sum("submit" in call for call in self.calls), 1)
        self.assertFalse(self.work.exists())

    def test_wrong_architecture_or_foreign_source_rejected_before_key_import(self):
        for path in self.inputs.iterdir():
            if path.name.endswith(".json"):
                path.write_text(json.dumps({"target": path.name.removeprefix("hraness-companion-").removesuffix(".evidence.json"), "sourceCommit": "b" * 40}))
        with self.assertRaises(signing.SigningError):
            self.sign()
        self.assertFalse(self.calls)

    def test_payload_symlink_rejected_before_key_import(self):
        path = self.inputs / next(iter(signing.asset_names()))
        path.unlink()
        path.symlink_to("/bin/echo")
        with self.assertRaises(signing.SigningError):
            self.sign()
        self.assertFalse(self.calls)

    def test_wrong_macho_architecture_rejected_before_key_import(self):
        path = self.inputs / 'hraness-helper-aarch64-apple-darwin'
        path.write_bytes(struct.pack('<IIIIIIII', 0xFEEDFACF, 0x01000007, 0, 2, 0, 0, 0, 0))
        with self.assertRaises(signing.SigningError):
            self.sign()
        self.assertFalse(self.calls)

    def test_release_floor_preserves_historical_version(self):
        with self.assertRaises(signing.SigningError):
            signing.sign(self.inputs, "1.1.2", self.output, self.work)
        self.assertFalse(self.calls)

    def test_zip_digest_inventory_and_regular_members(self):
        archive = self.root / "source.zip"
        name = "hraness-helper-aarch64-apple-darwin"
        with zipfile.ZipFile(archive, "w") as z:
            z.writestr(name, b"data")
        digest = signing.digest(archive.read_bytes())
        signing.unpack_artifact(archive, digest, {name}, self.root / "ok")
        for wrong_digest, names in [("0" * 64, {name}), (digest, {"unexpected"})]:
            with self.assertRaises(signing.SigningError):
                signing.unpack_artifact(archive, wrong_digest, names, self.root / "bad")
        with zipfile.ZipFile(archive, "w") as z:
            entry = zipfile.ZipInfo(name)
            entry.external_attr = (stat.S_IFLNK | 0o777) << 16
            z.writestr(entry, "/bin/echo")
        with self.assertRaises(signing.SigningError):
            signing.unpack_artifact(archive, signing.digest(archive.read_bytes()), {name}, self.root / "link")

    def test_artifact_metadata_requires_run_source_digest_and_exact_id(self):
        metadata = {"id": 7, "name": "macos-signed-2", "digest": "sha256:" + "b" * 64,
                    "expired": False, "size_in_bytes": 123, "workflow_run": {"id": 123, "head_sha": SHA}}
        self.assertEqual(artifacts.admit_metadata(metadata, "macos-signed-2", SHA, "7", "b" * 64), (7, "b" * 64))
        for key, value in [("name", "macos-signed-1"), ("id", 8), ("expired", True), ("digest", "sha256:" + "c" * 64),
                           ("workflow_run", {"id": 456, "head_sha": SHA}), ("workflow_run", {"id": 123, "head_sha": "c" * 40})]:
            with self.assertRaises(artifacts.signing.SigningError):
                artifacts.admit_metadata({**metadata, key: value}, "macos-signed-2", SHA, "7", "b" * 64)

    def test_remote_tag_recheck_accepts_lightweight_and_annotated_and_rejects_move(self):
        tag = 'refs/tags/v1.1.3'
        with patch.dict(os.environ, {'GITHUB_REF': tag}), patch.object(artifacts.Path, 'read_text', return_value='{"version":"1.1.3"}'):
            for response in [f'{SHA}\t{tag}', f'{"b" * 40}\t{tag}\n{SHA}\t{tag}^{{}}']:
                with patch.object(artifacts, 'command', return_value=response):
                    artifacts.verify_tag()
            with patch.object(artifacts, 'command', return_value=f'{"b" * 40}\t{tag}'):
                with self.assertRaises(artifacts.signing.SigningError):
                    artifacts.verify_tag()

    def test_receipt_digest_from_signing_job_prevents_rewriting_both_receipt_and_binary(self):
        self.sign()
        receipt_path = self.output / 'macos-signing.json'
        expected = signing.digest(receipt_path.read_bytes())
        with patch.dict(os.environ, {'SIGNED_RECEIPT_SHA256': expected}), patch.object(artifacts.Path, 'read_text', return_value='{"version":"1.1.3"}'):
            artifacts.verify_stage(self.output)
            receipt = json.loads(receipt_path.read_bytes())
            name = next(iter(signing.asset_names()))
            (self.output / name).write_bytes(b'altered binary')
            receipt['signedBinarySha256'][name] = signing.digest(b'altered binary')
            receipt_path.write_text(json.dumps(receipt))
            with self.assertRaises(artifacts.signing.SigningError):
                artifacts.verify_stage(self.output)


if __name__ == "__main__":
    unittest.main()
