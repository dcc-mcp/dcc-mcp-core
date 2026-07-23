"""Resolve one exact-version Windows UI Control host executable."""

from __future__ import annotations

from contextlib import contextmanager
from contextlib import suppress
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
from typing import Dict
from typing import Iterator
from typing import Optional
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import Request
from urllib.request import urlopen

HOST_ENV = "DCC_MCP_UI_CONTROL_HOST"
HOST_NAME = "dcc-mcp-ui-control-host.exe"
RELEASE_REPO = "dcc-mcp/dcc-mcp-core"
RELEASE_HOST_ASSET = "dcc-mcp-ui-control-host-windows-x86_64.exe"
RELEASE_MANIFEST_ASSET = "dcc-mcp-update-manifest-windows-x86_64.json"
RELEASE_MANIFEST_KEY = "dcc-mcp-ui-control-host"
MAX_MANIFEST_BYTES = 64 * 1024
MAX_HOST_BYTES = 128 * 1024 * 1024
DOWNLOAD_TIMEOUT_SECONDS = 30.0
LOCK_TIMEOUT_SECONDS = 90.0
MAX_VERSION_CHARS = 64
_VERSION_PATTERN = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$",
    re.ASCII,
)


class HostResolutionError(RuntimeError):
    """Fail-closed Host resolution error safe for an agent-facing response."""


def _strict_release_version(version: str) -> str:
    if len(version) > MAX_VERSION_CHARS:
        raise HostResolutionError("The running dcc-mcp-core package version is too long.")
    match = _VERSION_PATTERN.fullmatch(version)
    if match is None:
        raise HostResolutionError("The running dcc-mcp-core package does not expose a released version.")
    prerelease = match.group(4)
    if prerelease is not None and any(
        len(identifier) > 1 and identifier.startswith("0") and identifier.isdigit()
        for identifier in prerelease.split(".")
    ):
        raise HostResolutionError("The running dcc-mcp-core package does not expose a released version.")
    return version


def _package_version() -> str:
    try:
        from dcc_mcp_core import __version__

        version = str(__version__)
    except Exception:
        raise HostResolutionError("Cannot resolve the running dcc-mcp-core version.") from None
    return _strict_release_version(version)


def ui_control_host_version() -> str:
    """Return the strict release version used in the Host discovery endpoint."""
    return _package_version()


def _release_url(version: str, asset: str) -> str:
    return f"https://github.com/{RELEASE_REPO}/releases/download/v{version}/{asset}"


def _cache_directory(version: str) -> Path:
    try:
        from dcc_mcp_core import get_platform_dir

        return Path(str(get_platform_dir("cache"))) / "ui-control-host" / version
    except Exception:
        raise HostResolutionError("Cannot resolve the per-user UI Control host cache.") from None


def _ensure_directory(path: Path) -> None:
    try:
        path.mkdir(parents=True, exist_ok=True)
    except (OSError, ValueError):
        raise HostResolutionError("Cannot prepare the per-user UI Control host cache.") from None


def _is_file(path: Path) -> bool:
    try:
        return path.is_file()
    except (OSError, ValueError):
        raise HostResolutionError("Cannot inspect the UI Control host cache.") from None


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        if path.stat().st_size > MAX_HOST_BYTES:
            raise HostResolutionError("The cached UI Control host exceeds the release size limit.")
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    except HostResolutionError:
        raise
    except (OSError, ValueError):
        raise HostResolutionError("Cannot verify the cached UI Control host.") from None
    return digest.hexdigest()


def _validate_host_version(path: Path, expected_version: str) -> None:
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    try:
        result = subprocess.run(
            [str(path), "--version"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=5.0,
            creationflags=creationflags,
            check=False,
        )
    except (OSError, ValueError, subprocess.TimeoutExpired):
        raise HostResolutionError(
            "UI Control host version probe failed; "
            f"dcc-mcp-core {expected_version} accepts only host {expected_version}.",
        ) from None
    try:
        actual_version = result.stdout.decode("utf-8").strip()
    except (AttributeError, UnicodeDecodeError):
        raise HostResolutionError("UI Control host returned an invalid version response.") from None
    if result.returncode != 0 or actual_version != expected_version:
        raise HostResolutionError(
            f"UI Control host version mismatch; dcc-mcp-core {expected_version} accepts only host {expected_version}.",
        )


def _validate_host_file(path: Path, expected_version: str, expected_sha256: Optional[str] = None) -> Path:
    if not _is_file(path):
        raise HostResolutionError("The configured UI Control host does not exist.")
    if expected_sha256 is not None and _sha256_file(path).lower() != expected_sha256.lower():
        raise HostResolutionError("The cached UI Control host failed SHA-256 verification.")
    try:
        with path.open("rb") as stream:
            is_windows_pe = stream.read(2) == b"MZ"
    except (OSError, ValueError):
        raise HostResolutionError("Cannot read the configured UI Control host.") from None
    if not is_windows_pe:
        raise HostResolutionError("The configured UI Control host is not a Windows PE executable.")
    _validate_host_version(path, expected_version)
    try:
        return path.resolve()
    except (OSError, RuntimeError, ValueError):
        raise HostResolutionError("Cannot resolve the configured UI Control host path.") from None


def ui_control_host_identity(path: Path) -> str:
    """Return the full SHA-256 identity used in the Host discovery endpoint."""
    return _sha256_file(path).lower()


def validate_ui_control_host_image(path: Path, expected_version: str, expected_sha256: str) -> Path:
    """Validate a connected server image against one frozen discovery identity."""
    version = _strict_release_version(expected_version)
    sha256 = expected_sha256.lower()
    if len(sha256) != 64 or any(character not in "0123456789abcdef" for character in sha256):
        raise HostResolutionError("The UI Control host binary identity is invalid.")
    return _validate_host_file(path, version, sha256)


def _read_release_asset(version: str, asset: str, max_bytes: int) -> bytes:
    if asset not in {RELEASE_MANIFEST_ASSET, RELEASE_HOST_ASSET}:
        raise HostResolutionError("Refusing an untrusted UI Control release asset.")
    request = Request(
        _release_url(version, asset),
        headers={"User-Agent": f"dcc-mcp-core/{version} ui-control-host-resolver"},
    )
    try:
        with urlopen(request, timeout=DOWNLOAD_TIMEOUT_SECONDS) as response:
            payload = response.read(max_bytes + 1)
    except HTTPError as exc:
        raise HostResolutionError(
            f"Official GitHub Release returned HTTP {exc.code} for UI Control host {version}.",
        ) from None
    except URLError as exc:
        reason = type(exc.reason).__name__
        raise HostResolutionError(
            f"Cannot reach the official GitHub Release for UI Control host {version} ({reason}).",
        ) from None
    except Exception:
        raise HostResolutionError(
            f"Cannot download UI Control host {version} from the official GitHub Release.",
        ) from None
    if len(payload) > max_bytes:
        raise HostResolutionError(f"Official UI Control release asset {asset} is too large.")
    return payload


def _parse_manifest(payload: bytes, version: str) -> Dict[str, str]:
    try:
        manifest = json.loads(payload.decode("utf-8"))
        entry = manifest[RELEASE_MANIFEST_KEY]
        manifest_version = str(entry["version"])
        asset_url = str(entry["url"])
        sha256 = str(entry["sha256"]).lower()
    except (KeyError, TypeError, UnicodeError, ValueError):
        raise HostResolutionError("The official UI Control update manifest is invalid.") from None
    expected_url = _release_url(version, RELEASE_HOST_ASSET)
    if manifest_version != version:
        raise HostResolutionError(
            f"UI Control manifest version {manifest_version} does not match dcc-mcp-core {version}.",
        )
    if asset_url != expected_url:
        raise HostResolutionError("The UI Control manifest contains an untrusted asset URL.")
    if len(sha256) != 64 or any(character not in "0123456789abcdef" for character in sha256):
        raise HostResolutionError("The UI Control manifest requires a valid SHA-256.")
    return {"version": version, "url": expected_url, "sha256": sha256}


def _cached_manifest(path: Path, version: str) -> Optional[Dict[str, str]]:
    try:
        if path.stat().st_size > MAX_MANIFEST_BYTES:
            return None
        payload = path.read_bytes()
    except FileNotFoundError:
        return None
    except (OSError, ValueError):
        raise HostResolutionError("Cannot read the cached UI Control host manifest.") from None
    if len(payload) > MAX_MANIFEST_BYTES:
        return None
    try:
        return _parse_manifest(payload, version)
    except HostResolutionError:
        return None


def _atomic_write(path: Path, payload: bytes) -> None:
    _ensure_directory(path.parent)
    descriptor = -1
    temporary: Optional[Path] = None
    try:
        descriptor, temporary_name = tempfile.mkstemp(
            prefix=f".{path.name}.",
            suffix=".tmp",
            dir=str(path.parent),
        )
        temporary = Path(temporary_name)
        with os.fdopen(descriptor, "wb") as stream:
            descriptor = -1
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    except (OSError, ValueError):
        raise HostResolutionError("Cannot atomically write the UI Control host cache.") from None
    finally:
        if descriptor >= 0:
            with suppress(OSError):
                os.close(descriptor)
        if temporary is not None:
            with suppress(OSError):
                temporary.unlink()


def _lock_file(stream) -> None:
    if os.name == "nt":
        import msvcrt

        msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
    else:
        import fcntl

        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)


def _unlock_file(stream) -> None:
    if os.name == "nt":
        import msvcrt

        msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
    else:
        import fcntl

        fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


@contextmanager
def _cache_lock(path: Path) -> Iterator[None]:
    _ensure_directory(path.parent)
    try:
        stream = path.open("a+b")
    except (OSError, ValueError):
        raise HostResolutionError("Cannot open the UI Control host cache lock.") from None
    acquired = False
    try:
        try:
            stream.seek(0, os.SEEK_END)
            if stream.tell() == 0:
                stream.write(b"\0")
                stream.flush()
        except (OSError, ValueError):
            raise HostResolutionError("Cannot initialize the UI Control host cache lock.") from None

        deadline = time.monotonic() + LOCK_TIMEOUT_SECONDS
        while not acquired:
            try:
                stream.seek(0)
                _lock_file(stream)
                acquired = True
            except OSError as exc:
                if exc.errno not in {errno.EACCES, errno.EAGAIN}:
                    raise HostResolutionError("Cannot acquire the UI Control host cache lock.") from None
                if time.monotonic() >= deadline:
                    raise HostResolutionError("Timed out waiting for another UI Control host download.") from None
                time.sleep(0.05)
            except Exception:
                raise HostResolutionError("Cannot acquire the UI Control host cache lock.") from None
        yield
    finally:
        cleanup_failure = None
        if acquired:
            try:
                stream.seek(0)
                _unlock_file(stream)
            except Exception:
                cleanup_failure = "Cannot release the UI Control host cache lock."
        try:
            stream.close()
        except (OSError, ValueError):
            if cleanup_failure is None:
                cleanup_failure = "Cannot close the UI Control host cache lock."
        if cleanup_failure is not None:
            raise HostResolutionError(cleanup_failure) from None


def _promote_host(staged: Path, target: Path) -> None:
    try:
        staged.replace(target)
    except (OSError, ValueError):
        raise HostResolutionError(
            "Cannot replace the cached UI Control host because it may still be in use; "
            "the existing cache file was left unchanged.",
        ) from None


def _downloaded_host(version: str) -> Path:
    cache = _cache_directory(version)
    manifest_path = cache / RELEASE_MANIFEST_ASSET
    host_path = cache / HOST_NAME
    with _cache_lock(cache / ".download.lock"):
        manifest = _cached_manifest(manifest_path, version)
        if manifest is not None and _is_file(host_path) and _sha256_file(host_path).lower() == manifest["sha256"]:
            return _validate_host_file(host_path, version, manifest["sha256"])

        manifest_payload = _read_release_asset(version, RELEASE_MANIFEST_ASSET, MAX_MANIFEST_BYTES)
        manifest = _parse_manifest(manifest_payload, version)
        cached_manifest_payload = (
            json.dumps({RELEASE_MANIFEST_KEY: manifest}, sort_keys=True, separators=(",", ":")) + "\n"
        ).encode("utf-8")
        if _is_file(host_path) and _sha256_file(host_path).lower() == manifest["sha256"]:
            verified_host = _validate_host_file(host_path, version, manifest["sha256"])
            _atomic_write(manifest_path, cached_manifest_payload)
            return verified_host

        host_payload = _read_release_asset(version, RELEASE_HOST_ASSET, MAX_HOST_BYTES)
        if hashlib.sha256(host_payload).hexdigest().lower() != manifest["sha256"]:
            raise HostResolutionError("Downloaded UI Control host failed SHA-256 verification.")
        if host_payload[:2] != b"MZ":
            raise HostResolutionError("Downloaded UI Control host is not a Windows PE executable.")

        temporary_host = cache / f".{HOST_NAME}.{os.getpid()}.{threading.get_ident()}.tmp"
        try:
            _atomic_write(temporary_host, host_payload)
            _validate_host_file(temporary_host, version, manifest["sha256"])
            _promote_host(temporary_host, host_path)
        finally:
            with suppress(OSError):
                temporary_host.unlink()
        _atomic_write(manifest_path, cached_manifest_payload)
        return _validate_host_file(host_path, version, manifest["sha256"])


def resolve_ui_control_host() -> Path:
    """Return an exact-version Host from trusted env configuration or GitHub Release."""
    version = _package_version()
    configured = os.environ.get(HOST_ENV, "").strip()
    if configured:
        try:
            candidate = Path(configured)
        except (OSError, ValueError):
            raise HostResolutionError(f"{HOST_ENV} must name an absolute Host executable.") from None
        if not candidate.is_absolute():
            raise HostResolutionError(f"{HOST_ENV} must be an absolute path.")
        return _validate_host_file(candidate, version)
    return _downloaded_host(version)
