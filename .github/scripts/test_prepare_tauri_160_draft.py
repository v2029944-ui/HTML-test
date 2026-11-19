import hashlib
import importlib
import io
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import MagicMock, patch
from urllib.parse import parse_qs, urlsplit
from zipfile import ZipFile


REPO = "v2029944-ui/HTML-test"
SOURCE_SHA = "115d2d8cc8cea07c8711c470895790a1215eab5e"
SOURCE_BRANCH = "work/tauri-dropdown-updates-20261117"
REPO_ID = 1456834275
INSTALLER_NAME = "Analiz-rabot-Tauri-Windows-x64-setup.exe"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class PrepareDraftTest(unittest.TestCase):
    def setUp(self):
        self.script = importlib.import_module("prepare_tauri_160_draft")
        self.installer = b"test signed installer"
        self.signature = b"test signature"
        self.info = json.dumps({
            "mode": "signed preview",
            "bytes": len(self.installer),
            "sha256": sha256(self.installer),
        }).encode()
        self.run_metadata = {
            "id": 44493830093,
            "workflow_id": 420138938,
            "head_sha": SOURCE_SHA,
            "head_branch": SOURCE_BRANCH,
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "repository": {"id": REPO_ID, "full_name": REPO},
            "head_repository": {"id": REPO_ID, "full_name": REPO},
        }
        self.artifact = {
            "id": 14515586229,
            "name": "Tauri-Windows-preview",
            "expired": False,
            "workflow_run": {
                "id": 44493830093,
                "head_sha": SOURCE_SHA,
                "head_branch": SOURCE_BRANCH,
                "repository_id": REPO_ID,
                "head_repository_id": REPO_ID,
            },
        }
        self.release = {
            "id": 451126527,
            "tag_name": "tauri-v1.6.0",
            "target_commitish": SOURCE_SHA,
            "draft": True,
            "published_at": None,
            "prerelease": True,
            "assets": [],
        }
        self.uploads = {}
        self.operations = []
        self.archive = self.make_archive()
        self.enterContext(patch.dict(os.environ, {"GH_TOKEN": "fixture-token"}))
        self.enterContext(patch.object(self.script, "INSTALLER_SHA256", sha256(self.installer)))
        self.enterContext(patch.object(self.script, "INSTALLER_BYTES", len(self.installer)))
        self.enterContext(patch.object(self.script.subprocess, "run", side_effect=self.fake_gh))
        self.http = self.enterContext(patch.object(self.script, "urlopen", side_effect=self.fake_upload))

    def make_archive(self, *, installer=None, signature=None, include_signature=True, info=None):
        data = io.BytesIO()
        with ZipFile(data, "w") as artifact:
            artifact.writestr("setup.exe", self.installer if installer is None else installer)
            if include_signature:
                artifact.writestr("setup.exe.sig", self.signature if signature is None else signature)
            artifact.writestr("build-info.json", self.info if info is None else info)
            artifact.writestr("windows-webview2.png", b"test evidence")
        return data.getvalue()

    def fake_gh(self, argv, *, check, stdout, text=False, stderr=None):
        self.assertEqual(argv[:2], ["gh", "api"])
        self.assertTrue(check)
        endpoint = argv[2]
        self.operations.append(endpoint)
        prefix = f"repos/{REPO}/"
        documents = {
            prefix + "actions/runs/44493830093": self.run_metadata,
            prefix + "actions/artifacts/14515586229": self.artifact,
            prefix + "releases/451126527": self.release,
        }
        if endpoint == prefix + "actions/artifacts/14515586229/zip":
            self.assertFalse(text)
            self.assertEqual(stderr, subprocess.PIPE)
            stdout.write(self.archive)
            return subprocess.CompletedProcess(argv, 0)
        self.assertTrue(text)
        return subprocess.CompletedProcess(argv, 0, stdout=json.dumps(documents[endpoint]))

    def fake_upload(self, request, *, timeout):
        url = urlsplit(request.full_url)
        self.assertEqual(url.scheme, "https")
        self.assertEqual(url.netloc, "uploads.github.com")
        self.assertEqual(url.path, f"/repos/{REPO}/releases/451126527/assets")
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("Authorization"), "Bearer fixture-token")
        self.assertEqual(self.operations[-1], f"repos/{REPO}/releases/451126527")
        name, = parse_qs(url.query)["name"]
        self.assertNotIn(name, self.uploads)
        self.operations.append(f"upload {name}")
        self.uploads[name] = request.data
        self.release["assets"].append({
            "name": name,
            "size": len(request.data),
            "digest": f"sha256:{sha256(request.data)}",
            "state": "uploaded",
        })
        response = MagicMock()
        response.__enter__.return_value.status = 201
        return response

    def prepare(self, *, expected_archive=None):
        expected_archive = self.archive if expected_archive is None else expected_archive
        archive_hash = sha256(expected_archive)
        self.artifact["digest"] = f"sha256:{archive_hash}"
        self.artifact["size_in_bytes"] = len(expected_archive)
        with (
            patch.object(self.script, "ARCHIVE_SHA256", archive_hash),
            patch.object(self.script, "ARCHIVE_BYTES", len(expected_archive)),
            TemporaryDirectory() as directory,
        ):
            self.script.prepare_draft(Path(directory))

    def assert_refused(self, message, **options):
        with self.assertRaisesRegex(ValueError, message):
            self.prepare(**options)
        self.http.assert_not_called()

    def test_publication_or_wrong_target_are_refused(self):
        for field, value in (
            ("draft", False),
            ("published_at", "2026-11-19T12:00:00Z"),
            ("target_commitish", "main"),
            ("prerelease", False),
        ):
            with self.subTest(field=field), patch.dict(self.release, {field: value}):
                self.assert_refused("draft release")

    def test_preexisting_asset_is_refused(self):
        self.release["assets"] = [{"name": INSTALLER_NAME, "size": len(self.installer)}]
        self.assert_refused("Unexpected draft assets")

    def test_wrong_run_is_refused(self):
        for field, value in (
            ("conclusion", "failure"),
            ("event", "workflow_dispatch"),
            ("workflow_id", 123),
            ("head_sha", "other-commit"),
            ("head_repository", {"id": 123, "full_name": "someone/other"}),
        ):
            with self.subTest(field=field), patch.dict(self.run_metadata, {field: value}):
                self.assert_refused("workflow run")

    def test_wrong_artifact_is_refused(self):
        self.artifact["workflow_run"]["id"] = 123
        self.assert_refused("artifact metadata")

    def test_wrong_archive_hash_is_refused(self):
        expected_archive = self.archive
        self.archive = self.archive[:-1] + bytes([self.archive[-1] ^ 1])
        self.assert_refused("archive hash", expected_archive=expected_archive)

    def test_wrong_installer_hash_is_refused(self):
        self.archive = self.make_archive(installer=b"x" + self.installer[1:])
        self.assert_refused("installer hash")

    def test_wrong_build_mode_is_refused(self):
        self.archive = self.make_archive(info=self.info.replace(b"signed preview", b"CDP"))
        self.assert_refused("build information")

    def test_missing_or_empty_signature_is_refused(self):
        for archive in (self.make_archive(include_signature=False), self.make_archive(signature=b"")):
            with self.subTest(size=len(archive)):
                self.archive = archive
                self.assert_refused("signature")

    def test_draft_is_rechecked_between_uploads(self):
        def publish_after_upload(request, *, timeout):
            result = self.fake_upload(request, timeout=timeout)
            self.release["draft"] = False
            return result

        self.http.side_effect = publish_after_upload
        with self.assertRaisesRegex(ValueError, "draft release"):
            self.prepare()
        self.http.assert_called_once()

    def test_mismatched_github_digest_is_refused(self):
        def change_digest(request, *, timeout):
            result = self.fake_upload(request, timeout=timeout)
            self.release["assets"][-1]["digest"] = "sha256:wrong"
            return result

        self.http.side_effect = change_digest
        with self.assertRaisesRegex(ValueError, "asset digest"):
            self.prepare()
        self.http.assert_called_once()

    def test_missing_github_digest_is_refused(self):
        def omit_digest(request, *, timeout):
            result = self.fake_upload(request, timeout=timeout)
            del self.release["assets"][-1]["digest"]
            return result

        self.http.side_effect = omit_digest
        with self.assertRaisesRegex(ValueError, "asset digest"):
            self.prepare()
        self.http.assert_called_once()

    def test_uploads_only_approved_assets_then_checks_draft(self):
        self.prepare()
        expected = {
            INSTALLER_NAME: self.installer,
            INSTALLER_NAME + ".sig": self.signature,
            "windows-verification.json": self.info,
        }
        checksums = "".join(f"{sha256(data)}  {name}\n" for name, data in expected.items())
        self.assertEqual(self.uploads, {**expected, "SHA256SUMS": checksums.encode("ascii")})
        self.assertEqual(self.operations[-1], f"repos/{REPO}/releases/451126527")


class WorkflowTest(unittest.TestCase):
    def test_writer_gate_requires_approved_manual_dispatch(self):
        workflow = Path(__file__).parents[1] / "workflows" / "tauri-signed-release.yml"
        lines = workflow.read_text(encoding="utf-8").splitlines()
        self.assertIn("        default: check-keys", lines)
        gate = lines[lines.index("  prepare-draft:") + 1]
        self.assertEqual(
            gate,
            "    if: github.event_name == 'workflow_dispatch'"
            " && github.repository == 'v2029944-ui/HTML-test'"
            " && github.ref == 'refs/heads/work/sql-160-draft-assets'"
            " && inputs.operation == 'prepare-draft' && inputs.version == '1.6.0'",
        )


if __name__ == "__main__":
    unittest.main()
