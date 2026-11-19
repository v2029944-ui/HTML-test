"""Attach only the verified Tauri 1.6.0 assets to their approved draft."""

from hashlib import file_digest, sha256
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from urllib.parse import urlencode
from urllib.request import Request, urlopen
from zipfile import ZipFile


REPO = "v2029944-ui/HTML-test"
REPO_ID = 1456834275
SOURCE_SHA = "115d2d8cc8cea07c8711c470895790a1215eab5e"
SOURCE_BRANCH = "work/tauri-dropdown-updates-20261117"
RUN_ID = 44493830093
WORKFLOW_ID = 420138938
ARTIFACT_ID = 14515586229
ARCHIVE_BYTES = 6403449
ARCHIVE_SHA256 = "6957cdee3cbe85c9ae69741527d80e2c7fc46143ab8fe8bad839285f229d7d01"
INSTALLER_BYTES = 6060885
INSTALLER_SHA256 = "b6c552904c60015b0021905d8ca435463cef2de073bd34b00d94c274b5084ab8"
INSTALLER_NAME = "Analiz-rabot-Tauri-Windows-x64-setup.exe"
RELEASE_ID = 451126527
RELEASE_TAG = "tauri-v1.6.0"
API = f"repos/{REPO}"
ARTIFACT_API = f"{API}/actions/artifacts/{ARTIFACT_ID}"
RELEASE_API = f"{API}/releases/{RELEASE_ID}"


def gh_json(endpoint):
    response = subprocess.run(
        ["gh", "api", endpoint], check=True, stdout=subprocess.PIPE, text=True,
    )
    return json.loads(response.stdout)


def require_fields(document, expected, subject):
    for key, value in expected.items():
        actual = document.get(key)
        if key not in document or type(actual) is not type(value) or actual != value:
            raise ValueError(f"Unexpected {subject} field {key}")


def check_source():
    run = gh_json(f"{API}/actions/runs/{RUN_ID}")
    require_fields(run, {
        "id": RUN_ID,
        "workflow_id": WORKFLOW_ID,
        "head_sha": SOURCE_SHA,
        "head_branch": SOURCE_BRANCH,
        "event": "push",
        "status": "completed",
        "conclusion": "success",
    }, "workflow run")
    for field in ("repository", "head_repository"):
        require_fields(run[field], {"id": REPO_ID, "full_name": REPO}, "workflow run repository")

    artifact = gh_json(ARTIFACT_API)
    require_fields(artifact, {
        "id": ARTIFACT_ID,
        "name": "Tauri-Windows-preview",
        "size_in_bytes": ARCHIVE_BYTES,
        "digest": f"sha256:{ARCHIVE_SHA256}",
        "expired": False,
    }, "artifact metadata")
    require_fields(artifact["workflow_run"], {
        "id": RUN_ID,
        "head_sha": SOURCE_SHA,
        "head_branch": SOURCE_BRANCH,
        "repository_id": REPO_ID,
        "head_repository_id": REPO_ID,
    }, "artifact metadata workflow run")


def check_draft(expected_assets):
    release = gh_json(RELEASE_API)
    require_fields(release, {
        "id": RELEASE_ID,
        "tag_name": RELEASE_TAG,
        "target_commitish": SOURCE_SHA,
        "draft": True,
        "published_at": None,
        "prerelease": True,
    }, "draft release")
    assets = release["assets"]
    names = {asset["name"] for asset in assets}
    if len(assets) != len(expected_assets) or names != expected_assets.keys():
        raise ValueError("Unexpected draft assets")
    for asset in assets:
        size, digest = expected_assets[asset["name"]]
        require_fields(asset, {"size": size, "state": "uploaded"}, "draft asset")
        if asset.get("digest") != f"sha256:{digest}":
            raise ValueError(f"Unexpected draft asset digest for {asset['name']}")


def read_assets(archive):
    with archive.open("rb") as stream:
        archive_hash = file_digest(stream, "sha256").hexdigest()
    if archive.stat().st_size != ARCHIVE_BYTES or archive_hash != ARCHIVE_SHA256:
        raise ValueError("Unexpected preview archive hash or size")

    with ZipFile(archive) as bundle:
        installer = bundle.read("setup.exe")
        info = bundle.read("build-info.json")
        if "setup.exe.sig" not in bundle.namelist():
            raise ValueError("Missing installer signature")
        signature = bundle.read("setup.exe.sig")
    if len(installer) != INSTALLER_BYTES or sha256(installer).hexdigest() != INSTALLER_SHA256:
        raise ValueError("Unexpected installer hash or size")
    if not signature.strip():
        raise ValueError("Empty installer signature")
    require_fields(json.loads(info), {
        "mode": "signed preview",
        "bytes": INSTALLER_BYTES,
        "sha256": INSTALLER_SHA256,
    }, "build information")

    assets = {
        INSTALLER_NAME: installer,
        f"{INSTALLER_NAME}.sig": signature,
        "windows-verification.json": info,
    }
    checksums = "".join(f"{sha256(data).hexdigest()}  {name}\n" for name, data in assets.items())
    assets["SHA256SUMS"] = checksums.encode("ascii")
    return assets


def upload_request(name, data):
    query = urlencode({"name": name})
    request = Request(
        f"https://uploads.github.com/{RELEASE_API}/assets?{query}",
        data=data,
        headers={
            "Accept": "application/vnd.github+json",
            "Content-Type": "application/octet-stream",
            "X-GitHub-Api-Version": "2022-11-28",
        },
        method="POST",
    )
    request.add_unredirected_header("Authorization", f"Bearer {os.environ['GH_TOKEN']}")
    return request


def prepare_draft(directory):
    uploaded = {}
    check_draft(uploaded)
    check_source()
    archive = directory / "preview.zip"
    with archive.open("wb") as stream:
        # A failed redirect can include the temporary artifact download URL in stderr.
        subprocess.run(
            ["gh", "api", f"{ARTIFACT_API}/zip"],
            check=True, stdout=stream, stderr=subprocess.PIPE,
        )
    for name, data in read_assets(archive).items():
        request = upload_request(name, data)
        check_draft(uploaded)
        with urlopen(request, timeout=30) as response:
            if response.status != 201:
                raise ValueError(f"Unexpected status uploading {name}: {response.status}")
        uploaded[name] = len(data), sha256(data).hexdigest()

    check_draft(uploaded)
    print(f"Verified {len(uploaded)} assets on draft {RELEASE_TAG} ({RELEASE_ID}).")


if __name__ == "__main__":
    with TemporaryDirectory() as directory:
        prepare_draft(Path(directory))
