#!/usr/bin/env python3
"""Validate curated install artifacts and prepare the exact payload to attest.

This script never chooses a newer package version, imports an adapter, signs a
payload, or publishes anything. Strict mode requires every active entry to pass.
Refreshes may quarantine individual failures so revocations are still published.
The caller must attest the resulting exact bytes before release.
"""

from __future__ import annotations

import argparse
import copy
from email.parser import Parser
import hashlib
from http.client import HTTPException
import io
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.parse import quote
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler
from urllib.request import Request
from urllib.request import build_opener
import zipfile
import zlib

try:
    from .archive_payload_policy import archive_member_errors
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from archive_payload_policy import archive_member_errors


MAX_JSON_BYTES = 4 * 1024 * 1024
MAX_ARTIFACT_BYTES = 128 * 1024 * 1024
MAX_EXPANDED_BYTES = 512 * 1024 * 1024
MAX_ARCHIVE_MEMBERS = 20000
MAX_INSTRUCTIONS_BYTES = 2 * 1024 * 1024
MAX_SOP_HEADER_BYTES = 4 * 1024
MAX_SOP_VERSION = 999999
VALIDITY_SECONDS = 7 * 24 * 60 * 60
QUARANTINE_REASON = "Release validation failed; installation is unavailable until a successful catalog refresh."
_SHA = re.compile(r"[0-9a-fA-F]{40}\Z")
_DIGEST = re.compile(r"(?:sha256:)?([0-9a-fA-F]{64})\Z")
_NAME = re.compile(r"[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?\Z")
_SOP_DECLARED = re.compile(rb"<!--\s*install-sop-version\s*:", re.IGNORECASE)
_SOP_VERSION = re.compile(rb"<!--\s*install-sop-version\s*:\s*([0-9]{1,9})\s*-->", re.IGNORECASE)
_HOSTS = frozenset(
    (
        "api.github.com",
        "github.com",
        "raw.githubusercontent.com",
        "codeload.github.com",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
        "pypi.org",
        "files.pythonhosted.org",
    )
)


class CatalogPreparationError(ValueError):
    """The complete catalog cannot be promoted safely."""


def _https_url(url: str) -> None:
    try:
        parsed = urlsplit(url)
        port = parsed.port
    except (TypeError, ValueError) as error:
        raise CatalogPreparationError("download URL is invalid") from error
    if (
        parsed.scheme != "https"
        or parsed.hostname not in _HOSTS
        or parsed.username
        or parsed.password
        or port not in (None, 443)
        or parsed.fragment
    ):
        raise CatalogPreparationError("download URL must use an approved HTTPS host")


class _SafeRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        _https_url(newurl)
        if request.has_header("Authorization"):
            raise CatalogPreparationError("authenticated GitHub API requests must not redirect")
        return super().redirect_request(request, fp, code, message, headers, newurl)


class HttpTransport:
    """Bound every HTTP read and restrict credentials to the GitHub API."""

    def __init__(self, token=None):
        self.token = token
        self.opener = build_opener(_SafeRedirects())

    def fetch(self, url: str, limit: int, optional: bool = False):
        _https_url(url)
        headers = {"User-Agent": "dcc-mcp-install-catalog-verifier"}
        if urlsplit(url).hostname == "api.github.com":
            headers["Accept"] = "application/vnd.github+json"
            headers["X-GitHub-Api-Version"] = "2022-11-28"
            if self.token:
                headers["Authorization"] = "Bearer " + self.token
        try:
            with self.opener.open(Request(url, headers=headers), timeout=30) as response:
                data = response.read(limit + 1)
        except HTTPError as error:
            if optional and error.code == 404:
                return None
            raise CatalogPreparationError(f"HTTP verification failed with status {error.code}") from error
        except (URLError, OSError, HTTPException) as error:
            raise CatalogPreparationError("HTTP verification failed") from error
        if len(data) > limit:
            raise CatalogPreparationError("download exceeds the size limit")
        return data


def _fetch(transport, url: str, limit: int, optional: bool = False):
    _https_url(url)
    data = transport.fetch(url, limit, optional=optional)
    if data is None and optional:
        return None
    if not isinstance(data, bytes) or len(data) > limit:
        raise CatalogPreparationError("download exceeds the size limit or has no content")
    return data


def _json(transport, url: str) -> dict:
    try:
        result = json.loads(_fetch(transport, url, MAX_JSON_BYTES).decode("utf-8"))
    except (UnicodeError, ValueError) as error:
        raise CatalogPreparationError("invalid remote JSON metadata") from error
    if not isinstance(result, dict):
        raise CatalogPreparationError("remote metadata must be an object")
    return result


def _project_name(value: str) -> str:
    return re.sub(r"[-_.]+", "-", value).lower()


def _checksum(value) -> str:
    match = _DIGEST.fullmatch(value) if isinstance(value, str) else None
    if not match:
        raise CatalogPreparationError("exact SHA-256 is required")
    return match.group(1).lower()


def _verified_archive(transport, install: dict) -> bytes:
    expected = _checksum(install.get("sha256"))
    data = _fetch(transport, install["url"], MAX_ARTIFACT_BYTES)
    if hashlib.sha256(data).hexdigest() != expected:
        raise CatalogPreparationError("artifact SHA-256 mismatch")
    return data


def _open_archive(data: bytes) -> zipfile.ZipFile:
    try:
        archive = zipfile.ZipFile(io.BytesIO(data))
        members = archive.infolist()
        if len(members) > MAX_ARCHIVE_MEMBERS or sum(item.file_size for item in members) > MAX_EXPANDED_BYTES:
            raise CatalogPreparationError("archive exceeds the expansion limit")
        errors = archive_member_errors(members)
        if errors:
            raise CatalogPreparationError("unsafe archive payload: " + "; ".join(errors[:3]))
        if archive.testzip() is not None:
            raise CatalogPreparationError("archive CRC mismatch")
        return archive
    except (zipfile.BadZipFile, RuntimeError, NotImplementedError, OSError, EOFError, zlib.error) as error:
        raise CatalogPreparationError("invalid ZIP artifact") from error


def _requires_python(value) -> str:
    if not isinstance(value, str) or not value.strip():
        raise CatalogPreparationError("Requires-Python is required")
    return re.sub(r"\s+", "", value)


def _verify_wheel(data: bytes, package: str, version: str, requires_python: str, entry_point=None) -> None:
    with _open_archive(data) as archive:
        names = archive.namelist()
        metadata_names = [name for name in names if name.endswith(".dist-info/METADATA")]
        if len(metadata_names) != 1:
            raise CatalogPreparationError("wheel must contain exactly one METADATA file")
        try:
            metadata = Parser().parsestr(archive.read(metadata_names[0]).decode("utf-8"))
        except UnicodeError as error:
            raise CatalogPreparationError("wheel METADATA must be UTF-8") from error
        for field in ("Name", "Version", "Requires-Python"):
            if len(metadata.get_all(field, [])) != 1:
                raise CatalogPreparationError(f"wheel requires exactly one {field} field")
        if _project_name(metadata["Name"]) != _project_name(package) or metadata["Version"] != version:
            raise CatalogPreparationError("wheel METADATA name/version mismatch")
        if _requires_python(metadata["Requires-Python"]) != requires_python:
            raise CatalogPreparationError("wheel Requires-Python mismatch")
        if entry_point:
            module = entry_point.split(":", 1)[0]
            if not re.fullmatch(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*", module):
                raise CatalogPreparationError("invalid adapter entry point")
            path = module.replace(".", "/")
            if path + ".py" not in names and path + "/__init__.py" not in names:
                raise CatalogPreparationError("wheel is missing its declared entry-point module")


def _verify_pip(transport, entry: dict) -> None:
    install = entry["install"]
    package, version = install.get("pip_package"), entry.get("version")
    if not isinstance(package, str) or not _NAME.fullmatch(package) or not isinstance(version, str) or not version:
        raise CatalogPreparationError("pip entry requires an exact package and version")
    url = install.get("url", "")
    filename = "{}-{}-py3-none-any.whl".format(re.sub(r"[-_.]+", "_", package).lower(), version)
    parsed = urlsplit(url)
    if (
        parsed.hostname != "files.pythonhosted.org"
        or parsed.query
        or parsed.fragment
        or parsed.path.rsplit("/", 1)[-1] != filename
    ):
        raise CatalogPreparationError("pip artifact must be the declared official universal wheel")
    expected = _checksum(install.get("sha256"))
    metadata = _json(
        transport, "https://pypi.org/pypi/{}/{}/json".format(quote(package, safe=""), quote(version, safe=""))
    )
    info = metadata.get("info", {})
    if (
        not isinstance(info, dict)
        or _project_name(info.get("name", "")) != _project_name(package)
        or info.get("version") != version
    ):
        raise CatalogPreparationError("PyPI package identity mismatch")
    if info.get("yanked"):
        raise CatalogPreparationError("PyPI release is yanked")
    files = metadata.get("urls", [])
    if not isinstance(files, list):
        raise CatalogPreparationError("PyPI artifact list is invalid")
    matches = [
        item for item in files if isinstance(item, dict) and item.get("url") == url and item.get("filename") == filename
    ]
    if len(matches) != 1:
        raise CatalogPreparationError("curated wheel is absent or ambiguous in PyPI version metadata")
    artifact = matches[0]
    if artifact.get("yanked") is not False or artifact.get("packagetype") != "bdist_wheel":
        raise CatalogPreparationError("PyPI wheel is yanked or has an invalid package type")
    if _checksum(artifact.get("digests", {}).get("sha256")) != expected:
        raise CatalogPreparationError("PyPI artifact SHA-256 differs from the curated catalog")
    requires_python = _requires_python(info.get("requires_python"))
    if _requires_python(artifact.get("requires_python")) != requires_python:
        raise CatalogPreparationError("PyPI Requires-Python mismatch")
    data = _verified_archive(transport, install)
    if artifact.get("size") != len(data):
        raise CatalogPreparationError("PyPI artifact size mismatch")
    _verify_wheel(data, package, version, requires_python, install.get("entry_point"))


def _official_repo(entry: dict) -> str:
    match = re.fullmatch(r"https://github\.com/(dcc-mcp/[A-Za-z0-9_.-]+)/?", entry.get("url", ""))
    if not match:
        raise CatalogPreparationError("install entry must own an official dcc-mcp repository")
    return match.group(1)


def _release_commit(transport, repo: str, version: str) -> str:
    ref = _json(
        transport, "https://api.github.com/repos/{}/git/ref/tags/{}".format(repo, quote("v" + version, safe=""))
    )
    for _ in range(5):
        obj = ref.get("object", {})
        if not isinstance(obj, dict) or not isinstance(obj.get("sha"), str) or not _SHA.fullmatch(obj["sha"]):
            raise CatalogPreparationError("release tag has no immutable object")
        if obj.get("type") == "commit":
            return obj["sha"].lower()
        if obj.get("type") != "tag":
            break
        ref = _json(transport, "https://api.github.com/repos/{}/git/tags/{}".format(repo, obj["sha"]))
    raise CatalogPreparationError("release tag does not resolve to a commit")


def _declared_sop_version(data: bytes):
    """Return the Install SOP version declared by a runbook, or None when absent.

    The declaration is a lightweight HTML comment marker in the runbook header, so the
    pin stage never needs another adapter-owned document to record it.
    """
    header = data[:MAX_SOP_HEADER_BYTES]
    declared = _SOP_DECLARED.findall(header)
    if not declared:
        return None
    parsed = _SOP_VERSION.findall(header)
    values = {int(value) for value in parsed}
    if len(values) != 1 or len(parsed) != len(declared):
        raise CatalogPreparationError("install SOP version declaration is malformed or inconsistent")
    version = values.pop()
    if not 0 < version <= MAX_SOP_VERSION:
        raise CatalogPreparationError("install SOP version declaration is out of range")
    return version


def _record_sop_version(entry: dict, data: bytes) -> None:
    """Record the runbook's declared Install SOP version on the install entry."""
    install = entry["install"]
    declared = _declared_sop_version(data)
    if "sop_version" in install:
        curated = install["sop_version"]
        if isinstance(curated, bool) or not isinstance(curated, int) or curated < 1:
            raise CatalogPreparationError("curated install SOP version must be a positive integer")
        if curated > MAX_SOP_VERSION:
            raise CatalogPreparationError("curated install SOP version is out of range")
        if declared is not None and curated != declared:
            raise CatalogPreparationError("install SOP version declaration differs from the catalog")
        return
    if declared is not None:
        install["sop_version"] = declared


def _pin_instructions(transport, entry: dict, commit=None) -> None:
    repo = _official_repo(entry)
    if commit is None:
        commit = _release_commit(transport, repo, entry["version"])
    base = f"https://raw.githubusercontent.com/{repo}/{commit}/"
    canonical = base + "install.md"
    data = _fetch(transport, canonical, MAX_INSTRUCTIONS_BYTES, optional=True)
    if data is not None:
        if not data.strip():
            raise CatalogPreparationError("canonical install instructions are empty")
        entry["install"]["instructions_url"] = canonical
        _record_sop_version(entry, data)
        return
    current = entry["install"].get("instructions_url", "")
    prefix = "https://raw.githubusercontent.com/" + repo + "/"
    if not current.startswith(prefix):
        raise CatalogPreparationError("fallback instructions must belong to the adapter repository")
    suffix = current[len(prefix) :].split("/", 1)
    if len(suffix) != 2 or not re.fullmatch(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\.md", suffix[1]):
        raise CatalogPreparationError("fallback instructions must identify a Markdown file")
    if any(part in (".", "..") for part in suffix[1].split("/")):
        raise CatalogPreparationError("invalid instructions path")
    pinned = base + suffix[1]
    fallback = _fetch(transport, pinned, MAX_INSTRUCTIONS_BYTES)
    if not fallback.strip():
        raise CatalogPreparationError("release-pinned instructions are empty")
    entry["install"]["instructions_url"] = pinned
    _record_sop_version(entry, fallback)


def prepare_catalog(
    document: dict, source_revision: str, now: int, transport, quarantine_invalid: bool = False
) -> dict:
    """Validate all active artifacts before returning a new unsigned payload."""
    if not _SHA.fullmatch(source_revision):
        raise CatalogPreparationError("source revision must be a full commit SHA")
    if isinstance(now, bool) or not isinstance(now, int) or now < 0:
        raise CatalogPreparationError("issued_at must be a nonnegative Unix timestamp")
    entries = document.get("entries") if isinstance(document, dict) else None
    if not isinstance(entries, list) or not entries:
        raise CatalogPreparationError("catalog must contain entries")
    if set(document) - {"version", "entries"} or document.get("version", "1") != "1":
        raise CatalogPreparationError("unsupported catalog document schema")
    entries = copy.deepcopy(entries)
    names = set()
    for entry in entries:
        if (
            not isinstance(entry, dict)
            or not isinstance(entry.get("name"), str)
            or not entry["name"]
            or not isinstance(entry.get("description"), str)
            or not entry["description"]
        ):
            raise CatalogPreparationError("catalog entry requires a name and description")
        if entry["name"] in names:
            raise CatalogPreparationError("duplicate catalog entry")
        names.add(entry["name"])
        install = entry.get("install")
        policy = entry.get("policy", {})
        if (
            not isinstance(policy, dict)
            or any(key not in ("installation", "reason") for key in policy)
            or any(not isinstance(value, str) for value in policy.values())
            or ("policy" in entry and "installation" not in policy)
            or ("reason" in policy and not policy["reason"])
        ):
            raise CatalogPreparationError("invalid installation policy")
        if install is not None and (
            not isinstance(install, dict) or install.get("type") not in ("pip", "git", "zip", "path")
        ):
            raise CatalogPreparationError("invalid install metadata schema")
        if policy.get("installation") == "not_available":
            if quarantine_invalid:
                entry.pop("install", None)
            continue
        if install is None:
            continue
        try:
            if not isinstance(install, dict):
                raise CatalogPreparationError("invalid install metadata")
            install_type = install.get("type")
            if install_type == "pip":
                _verify_pip(transport, entry)
                _pin_instructions(transport, entry)
            elif install_type == "zip":
                with _open_archive(_verified_archive(transport, install)):
                    pass
                _pin_instructions(transport, entry)
            elif install_type == "git":
                commit = install.get("ref", "")
                if not _SHA.fullmatch(commit):
                    raise CatalogPreparationError("git installation must pin a full commit SHA")
                repo = _official_repo(entry)
                git_url = install.get("url", "").rstrip("/")
                if git_url.endswith(".git"):
                    git_url = git_url[:-4]
                if git_url != "https://github.com/" + repo:
                    raise CatalogPreparationError("git install source must match the adapter repository")
                resolved = _json(transport, f"https://api.github.com/repos/{repo}/git/commits/{commit}")
                if resolved.get("sha", "").lower() != commit.lower():
                    raise CatalogPreparationError("git commit verification failed")
                _pin_instructions(transport, entry, commit.lower())
            else:
                raise CatalogPreparationError("public feed does not support this install type")
        except (CatalogPreparationError, KeyError, TypeError, AttributeError) as error:
            if quarantine_invalid:
                entry.pop("install", None)
                entry["policy"] = {"installation": "not_available", "reason": QUARANTINE_REASON}
                print("Quarantined {} after release validation failed.".format(entry["name"]), file=sys.stderr)
                continue
            raise CatalogPreparationError("{}: {}".format(entry["name"], error)) from error
    return {
        "schema_version": 1,
        "source_revision": source_revision.lower(),
        "issued_at": now,
        "expires_at": now + VALIDITY_SECONDS,
        "entries": entries,
    }


def write_payload(payload: dict, output: Path) -> None:
    """Atomically replace the local payload only after successful validation."""
    data = json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=str(output.parent), delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(output)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def _load_document(text: str):
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    try:
        import yaml
    except ImportError as error:
        raise CatalogPreparationError(
            "YAML input requires PyYAML; JSON input uses only the standard library"
        ) from error
    try:
        return yaml.safe_load(text)
    except yaml.YAMLError as error:
        raise CatalogPreparationError("invalid YAML catalog") from error


def main(argv=None) -> int:
    """Prepare a payload for a separate protected attestation/publish step."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, default=Path("dcc-mcp-catalog.yml"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument(
        "--quarantine-invalid",
        action="store_true",
        help="Publish invalid entries as unavailable so unrelated failures cannot block revocations",
    )
    parser.add_argument("--now", type=int, default=None, help="Explicit Unix timestamp for reproducible verification")
    args = parser.parse_args(argv)
    try:
        document = _load_document(args.catalog.read_text(encoding="utf-8"))
        transport = HttpTransport(os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN"))
        payload = prepare_catalog(
            document,
            args.source_revision,
            int(time.time()) if args.now is None else args.now,
            transport,
            quarantine_invalid=args.quarantine_invalid,
        )
        write_payload(payload, args.output)
    except (CatalogPreparationError, OSError) as error:
        print(f"Install catalog verification failed: {error}", file=sys.stderr)
        return 1
    print("Prepared {} curated entries; payload still requires attestation.".format(len(payload["entries"])))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
