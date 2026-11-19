import base64
from copy import deepcopy
from hashlib import sha256
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import advance_tauri_160_channel as job


def digest(data):
    return f"sha256:{sha256(data).hexdigest()}"


class AdvanceChannelTest(unittest.TestCase):
    def setUp(self):
        self.installer = b"fixture installer"
        self.signature = base64.b64encode(b"untrusted comment: fixture signature\nfixture\n")
        self.old = self.old_manifest("1.5.1")
        self.asset_id = 765813882
        self.current = self.old
        self.release = {
            "id": job.RELEASE_ID,
            "tag_name": job.RELEASE_TAG,
            "target_commitish": job.SOURCE_SHA,
            "published_at": job.PUBLISHED_AT,
            "draft": False,
            "prerelease": True,
            "assets": [
                self.asset(name, size, checksum) | {"browser_download_url": job.VERSION_URL + name}
                for name, (size, checksum) in job.VERSION_ASSETS.items()
            ],
        }
        self.operations = []
        self.download_count = 0
        self.source_sha = job.SOURCE_SHA
        self.downloads = {
            job.VERSION_URL + job.INSTALLER_NAME: self.installer,
            job.VERSION_URL + job.SIGNATURE_NAME: self.signature,
        }
        self.enterContext(patch.object(job, "OLD_SHA256", sha256(self.old).hexdigest()))
        self.enterContext(patch.object(job, "OLD_BYTES", len(self.old)))
        self.enterContext(patch.object(job, "gh_json", side_effect=self.metadata))
        self.enterContext(patch.object(job, "download", side_effect=self.download))
        self.enterContext(patch.object(job, "read_channel_asset", side_effect=self.read_channel))
        self.verify = self.enterContext(patch.object(job, "verify_signature"))
        self.enterContext(patch.object(job, "delete_old", side_effect=self.delete))
        self.post = self.enterContext(patch.object(job, "post_manifest", side_effect=self.post_new))
        replacements = job.VERSION_ASSETS | {
            job.INSTALLER_NAME: (len(self.installer), sha256(self.installer).hexdigest()),
            job.SIGNATURE_NAME: (len(self.signature), sha256(self.signature).hexdigest()),
        }
        self.enterContext(patch.object(job, "VERSION_ASSETS", replacements))
        for asset in self.release["assets"]:
            size, checksum = replacements[asset["name"]]
            asset.update(size=size, digest=f"sha256:{checksum}")
        self.directory = Path(self.enterContext(TemporaryDirectory()))

    def old_manifest(self, version):
        return json.dumps({
            "version": version,
            "platforms": {"windows-x86_64": {
                "signature": self.signature.decode("ascii"),
                "url": job.OLD_INSTALLER_URL,
            }},
        }).encode()

    def asset(self, name, size, checksum):
        return {"name": name, "size": size, "digest": f"sha256:{checksum}", "state": "uploaded"}

    def metadata(self, endpoint):
        if endpoint == job.RELEASE_API:
            return deepcopy(self.release)
        if endpoint == job.TAG_API:
            return {"object": {"type": "commit", "sha": self.source_sha}}
        if endpoint == job.CHANNEL_API:
            assets = [] if self.current is None else [self.asset(
                job.MANIFEST_NAME, len(self.current), sha256(self.current).hexdigest(),
            ) | {"id": self.asset_id, "browser_download_url": job.CHANNEL_URL,
                 "download_count": self.download_count}]
            return {"id": job.CHANNEL_ID, "tag_name": "tauri-updates", "draft": False,
                    "prerelease": True, "assets": assets}
        run_id = int(endpoint.rsplit("/", 1)[1])
        branch, event = job.CI_RUNS[run_id]
        repo = {"full_name": job.REPO, "id": job.REPO_ID}
        return {"id": run_id, "head_sha": self.source_sha, "head_branch": branch, "event": event,
                "status": "completed", "conclusion": "success", "workflow_id": job.WORKFLOW_ID,
                "repository": repo, "head_repository": repo}

    def download(self, url, size):
        return self.current if url == job.CHANNEL_URL else self.downloads[url]

    def read_channel(self, asset):
        self.assertEqual(asset["id"], self.asset_id)
        self.download_count += 1
        return self.current

    def delete(self, asset_id):
        self.assertEqual(self.asset_id, asset_id)
        self.operations.append("delete old")
        self.current = None

    def post_new(self, data):
        self.assertIsNone(self.current)
        self.current = data
        self.asset_id = 999
        self.operations.append(f"post {json.loads(data)['version']}")

    def run_update(self):
        job.prepare(self.directory)
        job.advance(self.directory)

    def assert_refused(self, message):
        with self.assertRaisesRegex(ValueError, message):
            self.run_update()
        self.assertEqual(self.operations, [])

    def test_wrong_source_does_not_delete(self):
        self.source_sha = "unexpected"
        self.assert_refused("CI run")

    def test_wrong_actual_installer_hash_does_not_delete(self):
        self.downloads[job.VERSION_URL + job.INSTALLER_NAME] = b"x" * len(self.installer)
        self.assert_refused("installer")
        self.verify.assert_not_called()

    def test_invalid_signature_does_not_delete(self):
        self.verify.side_effect = subprocess.CalledProcessError(1, ["minisign"])
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_update()
        self.assertEqual(self.operations, [])

    def test_future_version_does_not_delete(self):
        self.current = self.old_manifest("2.0.0")
        self.assert_refused("channel version")

    def test_post_failure_restores_exact_old_bytes(self):
        def fail_first_post(data):
            if data != self.old:
                raise HTTPError(job.CHANNEL_URL, 502, "Fixture failure", None, None)
            self.post_new(data)

        self.post.side_effect = fail_first_post
        with self.assertRaisesRegex(RuntimeError, "previous manifest restored"):
            self.run_update()
        self.assertEqual(self.current, self.old)
        self.assertEqual(self.operations, ["delete old", "post 1.5.1"])
        self.assertEqual((self.directory / "previous.json").read_bytes(), self.old)

    def test_transient_metadata_failures_after_delete_reconcile_and_restore(self):
        failed_phases = set()

        def flaky_metadata(endpoint):
            phase = self.post.call_count
            if endpoint == job.CHANNEL_API and self.current is None and phase not in failed_phases:
                failed_phases.add(phase)
                raise subprocess.CalledProcessError(1, ["gh", "api"])
            return self.metadata(endpoint)

        def fail_candidate(data):
            if data != self.old:
                raise HTTPError(job.CHANNEL_URL, 502, "Fixture failure", None, None)
            self.post_new(data)

        self.post.side_effect = fail_candidate
        with patch.object(job, "gh_json", side_effect=flaky_metadata), patch.object(job, "sleep"):
            with self.assertRaisesRegex(RuntimeError, "previous manifest restored"):
                self.run_update()
        self.assertEqual(failed_phases, {0, 1})
        self.assertEqual(self.current, self.old)
        self.assertEqual(self.operations, ["delete old", "post 1.5.1"])

    def test_exact_restored_manifest_can_be_retried_by_its_new_numeric_id(self):
        self.asset_id = 777
        self.run_update()
        self.assertEqual(self.operations, ["delete old", "post 1.6.0"])

    def test_already_updated_is_read_only(self):
        self.current = job.candidate_manifest(self.signature)
        self.asset_id = 999
        self.run_update()
        self.assertEqual(self.operations, [])
        self.verify.assert_called_once()

    def test_ambiguous_post_failure_keeps_uploaded_candidate(self):
        def fail_after_post(data):
            self.post_new(data)
            raise TimeoutError("Response was lost")

        self.post.side_effect = fail_after_post
        self.run_update()
        self.assertEqual(self.operations, ["delete old", "post 1.6.0"])

    def test_external_state_kept_after_failed_post(self):
        future = self.old_manifest("2.0.0")

        def fail_with_external_asset(data):
            self.current = future
            self.asset_id = 1000
            raise HTTPError(job.CHANNEL_URL, 422, "Name occupied", None, None)

        self.post.side_effect = fail_with_external_asset
        with self.assertRaisesRegex(RuntimeError, "current asset preserved"):
            self.run_update()
        self.assertEqual(self.current, future)
        self.assertEqual(self.operations, ["delete old"])
        self.post.assert_called_once()

    def test_stale_public_cache_keeps_verified_new_manifest(self):
        def stale_public(url, size):
            return self.old if url == job.CHANNEL_URL else self.download(url, size)

        with patch.object(job, "download", side_effect=stale_public), patch.object(job, "sleep") as pause:
            with self.assertRaisesRegex(RuntimeError, "public endpoint did not converge"):
                self.run_update()
        self.assertEqual(pause.call_count, 7)
        self.assertEqual(self.current, (self.directory / "candidate.json").read_bytes())
        self.assertEqual(self.operations, ["delete old", "post 1.6.0"])

    def test_replaces_only_manifest_and_verifies_exact_bytes(self):
        self.run_update()
        self.assertEqual(self.operations, ["delete old", "post 1.6.0"])
        self.assertEqual(self.current, (self.directory / "candidate.json").read_bytes())
        platform = json.loads(self.current)["platforms"]["windows-x86_64"]
        self.assertEqual(platform["signature"].encode(), self.signature)
        self.assertEqual(platform["url"], job.VERSION_URL + job.INSTALLER_NAME)
        self.assertEqual(digest((self.directory / "previous.json").read_bytes()), digest(self.old))


if __name__ == "__main__":
    unittest.main()
