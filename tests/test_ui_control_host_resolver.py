from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import errno
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import traceback
from types import SimpleNamespace
from typing import Any
from urllib.error import URLError

import pytest

from conftest import REPO_ROOT

SCRIPTS = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "scripts"
RESOLVER_PATH = SCRIPTS / "_ui_control_host_resolver.py"
CLIENT_PATH = SCRIPTS / "_ui_control_host_client.py"
VERSION = "0.19.65"


def _load(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_resolver() -> Any:
    return _load(RESOLVER_PATH, "_test_ui_control_host_resolver")


def _load_client() -> Any:
    return _load(CLIENT_PATH, "_test_ui_control_host_client_mapping")


def _manifest(resolver: Any, host: bytes, *, version: str = VERSION, url: str | None = None) -> bytes:
    entry = {
        "version": version,
        "url": url or resolver._release_url(version, resolver.RELEASE_HOST_ASSET),
        "sha256": hashlib.sha256(host).hexdigest(),
    }
    return json.dumps({resolver.RELEASE_MANIFEST_KEY: entry}).encode("utf-8")


def _configure_download(resolver: Any, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(resolver.HOST_ENV, raising=False)
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(resolver, "_cache_directory", lambda version: tmp_path / version)
    monkeypatch.setattr(resolver, "_validate_host_version", lambda path, version: None)


def test_client_maps_resolver_failure_to_backend_unavailable(monkeypatch: pytest.MonkeyPatch) -> None:
    client = _load_client()

    def fail() -> None:
        raise client._RESOLVER.HostResolutionError("Safe resolver failure.")

    monkeypatch.setattr(client._RESOLVER, "resolve_ui_control_host", fail)
    with pytest.raises(client.UiControlHostError) as failure:
        client._host_binary()
    assert failure.value.code == "backend_unavailable"
    assert str(failure.value) == "Safe resolver failure."


def test_environment_host_requires_the_exact_client_version(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    host = tmp_path / "dcc-mcp-ui-control-host.exe"
    host.write_bytes(b"MZhost")
    monkeypatch.setenv(resolver.HOST_ENV, str(host.resolve()))
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(resolver, "_downloaded_host", lambda _version: pytest.fail("env path must win"))

    monkeypatch.setattr(
        resolver.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout=f"{VERSION}\n".encode()),
    )
    assert resolver.resolve_ui_control_host() == host.resolve()

    monkeypatch.setattr(
        resolver.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout=b"0.19.64\n"),
    )
    with pytest.raises(resolver.HostResolutionError, match=r"0\.19\.65 accepts only host 0\.19\.65"):
        resolver.resolve_ui_control_host()


@pytest.mark.parametrize("configured", ["", "relative\\dcc-mcp-ui-control-host.exe"])
def test_invalid_environment_path_never_downloads(
    configured: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()
    monkeypatch.setenv(resolver.HOST_ENV, configured)
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(resolver, "_downloaded_host", lambda _version: pytest.fail("must not download"))

    with pytest.raises(resolver.HostResolutionError, match=r"must (name an absolute|be an absolute)"):
        resolver.resolve_ui_control_host()


def test_no_environment_has_only_the_release_download_route(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    expected = tmp_path / "downloaded-host.exe"
    monkeypatch.delenv(resolver.HOST_ENV, raising=False)
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(resolver, "_downloaded_host", lambda version: expected if version == VERSION else None)
    monkeypatch.setattr(resolver, "_validate_host_file", lambda *_args: pytest.fail("no package/repo fallback"))

    assert resolver.resolve_ui_control_host() == expected


def test_download_uses_fixed_release_assets_and_verified_offline_cache(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    host = b"MZversion-0.19.65"
    downloads: list[str] = []

    def read_release_asset(version: str, asset: str, _max_bytes: int) -> bytes:
        assert version == VERSION
        downloads.append(asset)
        return _manifest(resolver, host) if asset == resolver.RELEASE_MANIFEST_ASSET else host

    monkeypatch.setattr(resolver, "_read_release_asset", read_release_asset)
    resolved = resolver.resolve_ui_control_host()
    assert resolved.read_bytes() == host
    assert downloads == [resolver.RELEASE_MANIFEST_ASSET, resolver.RELEASE_HOST_ASSET]

    monkeypatch.setattr(resolver, "_read_release_asset", lambda *_args: pytest.fail("cache must work offline"))
    assert resolver.resolve_ui_control_host() == resolved


@pytest.mark.parametrize("manifest_state", ["missing", "damaged"])
def test_official_manifest_recovers_existing_verified_host_without_replacing_it(
    manifest_state: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    cache = tmp_path / VERSION
    cache.mkdir(parents=True)
    host = b"MZexisting-version-0.19.65"
    host_path = cache / resolver.HOST_NAME
    host_path.write_bytes(host)
    manifest_path = cache / resolver.RELEASE_MANIFEST_ASSET
    if manifest_state == "damaged":
        manifest_path.write_bytes(b"{damaged")

    version_probes: list[tuple[Path, str]] = []
    monkeypatch.setattr(
        resolver,
        "_validate_host_version",
        lambda path, version: version_probes.append((path, version)),
    )
    downloads: list[str] = []

    def read_release_asset(_version: str, asset: str, _max_bytes: int) -> bytes:
        downloads.append(asset)
        if asset != resolver.RELEASE_MANIFEST_ASSET:
            pytest.fail("a verified existing Host must not be downloaded or replaced")
        return _manifest(resolver, host)

    monkeypatch.setattr(resolver, "_read_release_asset", read_release_asset)
    monkeypatch.setattr(resolver, "_promote_host", lambda *_args: pytest.fail("Host replacement must be skipped"))

    assert resolver.resolve_ui_control_host() == host_path.resolve()
    assert host_path.read_bytes() == host
    assert downloads == [resolver.RELEASE_MANIFEST_ASSET]
    assert version_probes == [(host_path, VERSION)]
    recovered = json.loads(manifest_path.read_text(encoding="utf-8"))[resolver.RELEASE_MANIFEST_KEY]
    assert recovered["sha256"] == hashlib.sha256(host).hexdigest()


@pytest.mark.skipif(os.name != "nt", reason="exercises the Windows msvcrt byte-range lock")
def test_windows_cache_lock_initializes_the_first_locked_byte(tmp_path: Path) -> None:
    resolver = _load_resolver()
    lock_path = tmp_path / ".download.lock"

    with resolver._cache_lock(lock_path):
        assert lock_path.stat().st_size >= 1

    assert lock_path.read_bytes()[:1] == b"\0"


@pytest.mark.skipif(os.name != "nt", reason="exercises the Windows msvcrt byte-range lock")
def test_windows_cache_lock_blocks_another_process(tmp_path: Path) -> None:
    resolver = _load_resolver()
    lock_path = tmp_path / ".download.lock"
    ready_path = tmp_path / "child-ready"
    acquired_path = tmp_path / "child-acquired"
    child_code = "\n".join(
        [
            "import importlib.util",
            "from pathlib import Path",
            "import sys",
            "spec = importlib.util.spec_from_file_location('_child_host_resolver', sys.argv[1])",
            "module = importlib.util.module_from_spec(spec)",
            "spec.loader.exec_module(module)",
            "Path(sys.argv[3]).write_text('ready', encoding='ascii')",
            "with module._cache_lock(Path(sys.argv[2])):",
            "    Path(sys.argv[4]).write_text('acquired', encoding='ascii')",
        ]
    )
    process = None
    try:
        with resolver._cache_lock(lock_path):
            process = subprocess.Popen(
                [
                    sys.executable,
                    "-c",
                    child_code,
                    str(RESOLVER_PATH),
                    str(lock_path),
                    str(ready_path),
                    str(acquired_path),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            deadline = time.monotonic() + 5.0
            while not ready_path.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.01)
            assert ready_path.exists(), "child process did not reach the lock attempt"
            time.sleep(0.1)
            assert process.poll() is None
            assert not acquired_path.exists()

        stdout, stderr = process.communicate(timeout=5.0)
        assert process.returncode == 0, f"stdout={stdout!r} stderr={stderr!r}"
        assert acquired_path.read_text(encoding="ascii") == "acquired"
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=5.0)


def test_manifest_cannot_redirect_the_host_download(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    host = b"MZhost"
    monkeypatch.setattr(
        resolver,
        "_read_release_asset",
        lambda _version, asset, _max_bytes: (
            _manifest(resolver, host, url="https://example.invalid/host.exe")
            if asset == resolver.RELEASE_MANIFEST_ASSET
            else pytest.fail("untrusted host URL must not be downloaded")
        ),
    )

    with pytest.raises(resolver.HostResolutionError, match="untrusted asset URL"):
        resolver.resolve_ui_control_host()


def test_manifest_version_must_match_the_client(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    host = b"MZhost"
    monkeypatch.setattr(
        resolver,
        "_read_release_asset",
        lambda _version, asset, _max_bytes: (
            _manifest(resolver, host, version="0.19.64")
            if asset == resolver.RELEASE_MANIFEST_ASSET
            else pytest.fail("wrong-version host must not be downloaded")
        ),
    )

    with pytest.raises(resolver.HostResolutionError, match=r"0\.19\.64 does not match dcc-mcp-core 0\.19\.65"):
        resolver.resolve_ui_control_host()


def test_checksum_failure_never_promotes_download(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    expected_host = b"MZexpected"
    monkeypatch.setattr(
        resolver,
        "_read_release_asset",
        lambda _version, asset, _max_bytes: (
            _manifest(resolver, expected_host) if asset == resolver.RELEASE_MANIFEST_ASSET else b"MZtampered"
        ),
    )

    with pytest.raises(resolver.HostResolutionError, match="failed SHA-256"):
        resolver.resolve_ui_control_host()
    assert not (tmp_path / VERSION / resolver.HOST_NAME).exists()


def test_bad_cache_and_offline_network_fail_closed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    cache = tmp_path / VERSION
    cache.mkdir(parents=True)
    expected_host = b"MZexpected"
    corrupt_host = b"MZcorrupt"
    (cache / resolver.RELEASE_MANIFEST_ASSET).write_bytes(_manifest(resolver, expected_host))
    host_path = cache / resolver.HOST_NAME
    host_path.write_bytes(corrupt_host)

    def offline(*_args) -> None:
        raise resolver.HostResolutionError("Official release is unavailable.")

    monkeypatch.setattr(resolver, "_read_release_asset", offline)
    with pytest.raises(resolver.HostResolutionError, match="Official release is unavailable"):
        resolver.resolve_ui_control_host()
    assert host_path.read_bytes() == corrupt_host


def test_concurrent_resolvers_share_one_download(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    _configure_download(resolver, tmp_path, monkeypatch)
    host = b"MZconcurrent-host"
    downloads: list[str] = []
    calls_lock = threading.Lock()

    def read_release_asset(_version: str, asset: str, _max_bytes: int) -> bytes:
        with calls_lock:
            downloads.append(asset)
        time.sleep(0.05)
        return _manifest(resolver, host) if asset == resolver.RELEASE_MANIFEST_ASSET else host

    monkeypatch.setattr(resolver, "_read_release_asset", read_release_asset)
    with ThreadPoolExecutor(max_workers=2) as executor:
        resolved = list(executor.map(lambda _index: resolver.resolve_ui_control_host(), range(2)))

    assert resolved[0] == resolved[1]
    assert downloads == [resolver.RELEASE_MANIFEST_ASSET, resolver.RELEASE_HOST_ASSET]


def test_proxy_error_is_redacted(monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    monkeypatch.setattr(resolver, "urlopen", lambda *_args, **_kwargs: (_ for _ in ()).throw(URLError("secret")))

    with pytest.raises(resolver.HostResolutionError) as failure:
        resolver._read_release_asset(VERSION, resolver.RELEASE_MANIFEST_ASSET, resolver.MAX_MANIFEST_BYTES)
    assert "secret" not in str(failure.value)
    rendered = "".join(traceback.format_exception(type(failure.value), failure.value, failure.value.__traceback__))
    assert "secret" not in rendered
    assert "official GitHub Release" in str(failure.value)


@pytest.mark.parametrize(
    ("asset", "limit"),
    [("RELEASE_MANIFEST_ASSET", "MAX_MANIFEST_BYTES"), ("RELEASE_HOST_ASSET", "MAX_HOST_BYTES")],
)
def test_release_asset_size_limits(asset: str, limit: str, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    maximum = getattr(resolver, limit)

    class OversizedResponse:
        def __enter__(self):
            return self

        def __exit__(self, *_args):
            return False

        @staticmethod
        def read(length: int):
            class OversizedPayload:
                @staticmethod
                def __len__() -> int:
                    return length

            return OversizedPayload()

    monkeypatch.setattr(resolver, "urlopen", lambda *_args, **_kwargs: OversizedResponse())
    with pytest.raises(resolver.HostResolutionError, match="is too large"):
        resolver._read_release_asset(VERSION, getattr(resolver, asset), maximum)


@pytest.mark.parametrize("failure_site", ["mkstemp", "fsync", "replace"])
def test_cache_write_errors_are_stable_and_redacted(
    failure_site: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()

    def fail(*_args, **_kwargs) -> None:
        raise PermissionError(r"C:\Users\secret\cache")

    owner = resolver.tempfile if failure_site == "mkstemp" else Path if failure_site == "replace" else resolver.os
    monkeypatch.setattr(owner, failure_site, fail)
    with pytest.raises(resolver.HostResolutionError) as failure:
        resolver._atomic_write(tmp_path / "cache.json", b"payload")
    assert str(failure.value) == "Cannot atomically write the UI Control host cache."
    assert "secret" not in str(failure.value)
    rendered = "".join(traceback.format_exception(type(failure.value), failure.value, failure.value.__traceback__))
    assert "secret" not in rendered


def test_oversized_cached_assets_are_not_read(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    manifest = tmp_path / "manifest.json"
    manifest.write_bytes(b"x")
    original_stat = Path.stat

    def stat(path: Path, *args, **kwargs):
        if path == manifest:
            return SimpleNamespace(st_size=resolver.MAX_MANIFEST_BYTES + 1)
        return original_stat(path, *args, **kwargs)

    monkeypatch.setattr(Path, "stat", stat)
    monkeypatch.setattr(Path, "read_bytes", lambda _path: pytest.fail("oversized manifest must not be read"))
    assert resolver._cached_manifest(manifest, VERSION) is None

    class OversizedHost:
        @staticmethod
        def stat():
            return SimpleNamespace(st_size=resolver.MAX_HOST_BYTES + 1)

        @staticmethod
        def open(*_args, **_kwargs):
            pytest.fail("oversized host must not be read")

    with pytest.raises(resolver.HostResolutionError, match="exceeds the release size limit"):
        resolver._sha256_file(OversizedHost())


def test_in_use_host_replace_preserves_previous_verified_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    staged = tmp_path / "staged.exe"
    target = tmp_path / resolver.HOST_NAME
    staged.write_bytes(b"MZnew")
    target.write_bytes(b"MZprevious-verified")
    original_replace = Path.replace

    def replace(path: Path, destination: Path):
        if path == staged and Path(destination) == target:
            raise PermissionError(r"C:\Users\secret\locked.exe")
        return original_replace(path, destination)

    monkeypatch.setattr(Path, "replace", replace)
    with pytest.raises(resolver.HostResolutionError) as failure:
        resolver._promote_host(staged, target)
    assert "may still be in use" in str(failure.value)
    assert "secret" not in str(failure.value)
    assert target.read_bytes() == b"MZprevious-verified"
    assert staged.read_bytes() == b"MZnew"


def test_cache_read_error_is_stable_and_redacted(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    manifest = tmp_path / "manifest.json"
    manifest.write_bytes(b"{}")
    original_read_bytes = Path.read_bytes

    def read_bytes(path: Path) -> bytes:
        if path == manifest:
            raise PermissionError(r"C:\Users\secret\manifest.json")
        return original_read_bytes(path)

    monkeypatch.setattr(Path, "read_bytes", read_bytes)
    with pytest.raises(resolver.HostResolutionError) as failure:
        resolver._cached_manifest(manifest, VERSION)
    assert str(failure.value) == "Cannot read the cached UI Control host manifest."
    assert "secret" not in str(failure.value)


@pytest.mark.parametrize("failure_site", ["acquire", "release"])
def test_cache_lock_errors_are_stable_and_redacted(
    failure_site: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()

    def fail(_stream) -> None:
        error = OSError(errno.EBADF, r"C:\Users\secret\lock")
        raise error

    if failure_site == "acquire":
        monkeypatch.setattr(resolver, "_lock_file", fail)
    else:
        monkeypatch.setattr(resolver, "_lock_file", lambda _stream: None)
        monkeypatch.setattr(resolver, "_unlock_file", fail)

    with pytest.raises(resolver.HostResolutionError) as failure:
        with resolver._cache_lock(tmp_path / ".lock"):
            pass
    assert "secret" not in str(failure.value)
    assert "cache lock" in str(failure.value)


def test_version_decode_failure_is_stable_and_redacted(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    resolver = _load_resolver()
    host = tmp_path / resolver.HOST_NAME
    host.write_bytes(b"MZhost")
    monkeypatch.setenv(resolver.HOST_ENV, str(host.resolve()))
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(
        resolver.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout=b"\xffsecret"),
    )

    with pytest.raises(resolver.HostResolutionError) as failure:
        resolver.resolve_ui_control_host()
    assert str(failure.value) == "UI Control host returned an invalid version response."
    assert "secret" not in str(failure.value)
