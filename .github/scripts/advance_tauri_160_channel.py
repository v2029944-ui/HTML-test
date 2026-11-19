"""Verify the published Tauri 1.6.0 installer and advance its existing channel."""

import base64
from hashlib import sha256
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from time import sleep
from urllib.parse import urlencode
from urllib.request import Request, urlopen

from prepare_tauri_160_draft import (
    API, INSTALLER_BYTES, INSTALLER_NAME, INSTALLER_SHA256, RELEASE_API,
    RELEASE_ID, RELEASE_TAG, REPO, REPO_ID, RUN_ID, SOURCE_BRANCH, SOURCE_SHA,
    WORKFLOW_ID, gh_json, require_fields,
)


VERSION = "1.6.0"
PUBLISHED_AT = "2026-11-19T09:56:12Z"
TAG_API = f"{API}/git/ref/tags/{RELEASE_TAG}"
SIGNATURE_NAME = f"{INSTALLER_NAME}.sig"
RELEASE_DOWNLOAD = f"https://github.com/{REPO}/releases/download"
VERSION_URL = f"{RELEASE_DOWNLOAD}/{RELEASE_TAG}/"
OLD_INSTALLER_URL = f"{RELEASE_DOWNLOAD}/tauri-v1.5.1/{INSTALLER_NAME}"
CHANNEL_ID = 447297770
CHANNEL_API = f"{API}/releases/{CHANNEL_ID}"
MANIFEST_NAME = "tauri-latest.json"
CHANNEL_URL = f"{RELEASE_DOWNLOAD}/tauri-updates/{MANIFEST_NAME}"
OLD_BYTES = 5626
OLD_SHA256 = "3fcf2723cefff00430d63901b2554aa1c31f603a2993232e340798b5288d512e"
CI_RUNS = {
    RUN_ID: (SOURCE_BRANCH, "push"),
    44493389622: ("feat/sql-organization-filter", "pull_request"),
}
VERSION_ASSETS = {
    INSTALLER_NAME: (INSTALLER_BYTES, INSTALLER_SHA256),
    SIGNATURE_NAME: (456, "e80048a0e8d20ea8f7f3caddcd9d3c6ed05b1ea7448b2b1226e183bc80d4682a"),
    "SHA256SUMS": (310, "bcec5c2eb77602d6ff66c22264128a46aa9b6f652600f70da6a21222aa032bf4"),
    "windows-verification.json": (
        3493, "be719bcde2aee7b340116eb69d19c2bff361dd3258dbc8bc885e5ce8b9244f34",
    ),
}
NOTES = (
    "Единое рабочее пространство SQLite для аналитики, отчётов, групп и Excel. "
    "Общие фильтры подразделений и работ, дневная аналитика, сравнение периодов, "
    "постраничные отчёты, CSV и печать. Улучшены безопасный импорт Excel, "
    "резервное копирование и восстановление SQLite.\n\n"
    "Это предварительный выпуск. Проверки выполнены на синтетических данных, "
    "реальные рабочие книги не проверялись. Перед обновлением сделайте проверенную "
    "резервную копию SQLite и сохраните исходные файлы Excel. Совместимость со "
    "старыми экспериментальными форматами не гарантируется."
)


def check_ci():
    repo = {"id": REPO_ID, "full_name": REPO}
    for run_id, (branch, event) in CI_RUNS.items():
        run = gh_json(f"{API}/actions/runs/{run_id}")
        require_fields(run, {
            "id": run_id, "workflow_id": WORKFLOW_ID, "head_sha": SOURCE_SHA,
            "head_branch": branch, "event": event, "status": "completed",
            "conclusion": "success",
        }, "CI run")
        require_fields(run["repository"], repo, "CI run repository")
        require_fields(run["head_repository"], repo, "CI run head repository")


def check_published_release():
    release = gh_json(RELEASE_API)
    require_fields(release, {
        "id": RELEASE_ID, "tag_name": RELEASE_TAG, "target_commitish": SOURCE_SHA,
        "draft": False, "prerelease": True, "published_at": PUBLISHED_AT,
    }, "published release")
    require_fields(gh_json(TAG_API)["object"], {"type": "commit", "sha": SOURCE_SHA}, "release tag")
    assets = release["assets"]
    if len(assets) != len(VERSION_ASSETS) or {item["name"] for item in assets} != VERSION_ASSETS.keys():
        raise ValueError("Unexpected published release asset inventory")
    for asset in assets:
        name = asset["name"]
        size, checksum = VERSION_ASSETS[name]
        require_fields(asset, {
            "size": size, "digest": f"sha256:{checksum}", "state": "uploaded",
            "browser_download_url": VERSION_URL + name,
        }, f"published release asset {name}")


def download(url, size):
    request = Request(url, headers={"Cache-Control": "no-cache"})
    with urlopen(request, timeout=25) as response:
        return response.read(size + 1)


def check_bytes(data, size, checksum, subject):
    if len(data) != size or sha256(data).hexdigest() != checksum:
        raise ValueError(f"Unexpected {subject} length or SHA-256")


def verify_signature(installer, signature, old_signature):
    public_key = os.environ["ANALIZ_UPDATER_PUBLIC_KEY"].strip().encode("ascii")
    if not public_key.startswith(b"untrusted comment:"):
        decoded = base64.b64decode(public_key, validate=True)
        if len(decoded) == 42 and decoded.startswith(b"Ed"):
            decoded = b"untrusted comment: Minisign public key\n" + public_key + b"\n"
        public_key = decoded

    checker = Path(__file__).resolve().parents[2] / "tauri/check-update-signing.cjs"
    with TemporaryDirectory() as directory:
        temporary = Path(directory)
        exe = temporary / INSTALLER_NAME
        sig = temporary / "installer.minisig"
        key = temporary / "updater.pub"
        exe.write_bytes(installer)
        sig.write_bytes(base64.b64decode(signature.strip(), validate=True))
        key.write_bytes(public_key)
        for name, encoded in (("current.sig", old_signature.encode("ascii")), ("candidate.sig", signature)):
            wrapped = temporary / name
            wrapped.write_bytes(encoded)
            subprocess.run(["node", str(checker), str(wrapped)], check=True)
        subprocess.run(["minisign", "-V", "-q", "-m", str(exe), "-x", str(sig), "-p", str(key)], check=True)


def channel_asset():
    release = gh_json(CHANNEL_API)
    require_fields(release, {
        "id": CHANNEL_ID, "tag_name": "tauri-updates", "draft": False, "prerelease": True,
    }, "channel release")
    assets = release["assets"]
    if not assets:
        return None
    if len(assets) != 1:
        raise ValueError("Unexpected channel asset inventory")
    asset, = assets
    require_fields(asset, {"name": MANIFEST_NAME, "browser_download_url": CHANNEL_URL}, "channel asset")
    if type(asset.get("id")) is not int or asset["id"] <= 0:
        raise ValueError("Unexpected channel asset ID")
    return asset


def reconcile_channel_asset():
    for attempt in range(3):
        try:
            return channel_asset()
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError):
            if attempt == 2:
                raise
            sleep(2)


def read_channel_asset(asset):
    command = ["gh", "api", "-H", "Accept: application/octet-stream", f"{API}/releases/assets/{asset['id']}"]
    data = subprocess.run(command, check=True, capture_output=True).stdout
    if not matches(asset, data):
        raise ValueError("Channel asset bytes do not match its metadata")
    return data


def matches(asset, data):
    return (
        asset is not None and asset.get("state") == "uploaded" and asset.get("size") == len(data)
        and asset.get("digest") == f"sha256:{sha256(data).hexdigest()}"
    )


def manifest_platform(data):
    manifest = json.loads(data)
    platform = manifest["platforms"]["windows-x86_64"]
    if not isinstance(platform.get("signature"), str) or not platform["signature"].strip():
        raise ValueError("Missing channel signature")
    return manifest, platform


def check_channel_manifest(data, signature):
    manifest, platform = manifest_platform(data)
    if manifest.get("version") == VERSION:
        require_fields(platform, {
            "url": VERSION_URL + INSTALLER_NAME, "signature": signature.decode("ascii"),
        }, "existing 1.6.0 channel platform")
    elif manifest.get("version") == "1.5.1":
        check_bytes(data, OLD_BYTES, OLD_SHA256, "previous channel manifest")
        require_fields(platform, {"url": OLD_INSTALLER_URL}, "previous channel platform")
    else:
        raise ValueError("Unexpected channel version")
    return manifest["version"], platform["signature"]


def candidate_manifest(signature):
    check_bytes(signature, *VERSION_ASSETS[SIGNATURE_NAME], "installer signature")
    manifest = {
        "version": VERSION,
        "notes": NOTES,
        "pub_date": PUBLISHED_AT,
        "platforms": {"windows-x86_64": {
            "signature": signature.decode("ascii"),
            "url": VERSION_URL + INSTALLER_NAME,
        }},
    }
    return (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def prepare(directory):
    check_ci()
    check_published_release()
    installer = download(VERSION_URL + INSTALLER_NAME, VERSION_ASSETS[INSTALLER_NAME][0])
    signature = download(VERSION_URL + SIGNATURE_NAME, VERSION_ASSETS[SIGNATURE_NAME][0])
    check_bytes(installer, *VERSION_ASSETS[INSTALLER_NAME], "published installer")
    candidate = candidate_manifest(signature)
    asset = channel_asset()
    if asset is None:
        raise ValueError("Channel has no current manifest")
    previous = read_channel_asset(asset)
    _, old_signature = check_channel_manifest(previous, signature)
    verify_signature(installer, signature, old_signature)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "previous.json").write_bytes(previous)
    (directory / "candidate.json").write_bytes(candidate)
    (directory / "previous-asset-id").write_text(str(asset["id"]), encoding="ascii")


def write_api(url, method, status, data=None):
    request = Request(url, method=method, data=data, headers={
        "Accept": "application/vnd.github+json",
        "Content-Type": "application/json",
        "X-GitHub-Api-Version": "2022-11-28",
    })
    request.add_unredirected_header("Authorization", f"Bearer {os.environ['GH_TOKEN']}")
    with urlopen(request, timeout=25) as response:
        if response.status != status:
            raise ValueError(f"Unexpected HTTP status for channel {method}: {response.status}")


def delete_old(asset_id):
    write_api(f"https://api.github.com/{API}/releases/assets/{asset_id}", "DELETE", 204)


def post_manifest(data):
    query = urlencode({"name": MANIFEST_NAME})
    write_api(f"https://uploads.github.com/{CHANNEL_API}/assets?{query}", "POST", 201, data)


def verify_api(asset, expected):
    if not matches(asset, expected) or read_channel_asset(asset) != expected:
        raise RuntimeError("GitHub channel asset does not match the expected bytes")


def replace_manifest(asset_id, previous, candidate):
    try:
        delete_old(asset_id)
        if reconcile_channel_asset() is not None:
            raise ValueError("Channel changed during replacement")
        post_manifest(candidate)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        observed = reconcile_channel_asset()
        if matches(observed, candidate):
            verify_api(observed, candidate)
            return
        if observed is not None:
            raise RuntimeError("Channel update failed; current asset preserved") from error
        try:
            post_manifest(previous)
        except (OSError, ValueError):
            # A lost response can still mean GitHub accepted the restored asset.
            pass
        verify_api(reconcile_channel_asset(), previous)
        raise RuntimeError("Channel update failed; previous manifest restored") from error
    verify_api(reconcile_channel_asset(), candidate)


def verify_public(expected):
    for attempt in range(8):
        try:
            if download(CHANNEL_URL, len(expected)) == expected:
                return
        except OSError:
            pass
        if attempt < 7:
            sleep(10)
    raise RuntimeError("GitHub API verified 1.6.0; the public endpoint did not converge within the wait limit")


def advance(directory):
    check_published_release()
    candidate = (directory / "candidate.json").read_bytes()
    _, platform = manifest_platform(candidate)
    signature = platform["signature"].encode("ascii")
    if candidate != candidate_manifest(signature):
        raise ValueError("Prepared candidate differs from the expected manifest")
    asset = channel_asset()
    if asset is None:
        raise ValueError("Channel manifest disappeared after preparation")
    current = read_channel_asset(asset)
    version, _ = check_channel_manifest(current, signature)
    if version == VERSION:
        verify_public(current)
        print(f"Channel already serves verified {VERSION}.")
        return
    previous = (directory / "previous.json").read_bytes()
    prepared_id = int((directory / "previous-asset-id").read_text(encoding="ascii"))
    if current != previous or asset["id"] != prepared_id:
        raise ValueError("Channel content changed after preparation")
    latest = channel_asset()
    if not matches(latest, previous) or latest["id"] != prepared_id:
        raise ValueError("Channel asset changed before replacement")
    replace_manifest(prepared_id, previous, candidate)
    verify_public(candidate)
    print(f"Verified {VERSION} in the GitHub API and public channel endpoint.")


if __name__ == "__main__":
    mode, directory = sys.argv[1:]
    {"prepare": prepare, "advance": advance}[mode](Path(directory))
