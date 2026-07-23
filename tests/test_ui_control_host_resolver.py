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
RUST_HOST_PATH = REPO_ROOT / "crates" / "dcc-mcp-computer-use" / "src" / "ui_control_host" / "windows.rs"
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


def test_slow_cold_resolver_gets_a_fresh_pipe_startup_window(monkeypatch: pytest.MonkeyPatch) -> None:
    client = _load_client()
    clock = {"now": 0.0, "host_ready": False}
    stream = object()
    attempts: list[float] = []
    endpoint = client._HostEndpoint(Path("host.exe"), VERSION, "a" * 64, r"\\.\pipe\slow-cold-host")

    def open_pipe(*_args, **_kwargs):
        attempts.append(clock["now"])
        if not clock["host_ready"]:
            raise FileNotFoundError("pipe is not ready")
        return stream

    def slow_launch(_binary: Path) -> None:
        clock["now"] += 60.0
        clock["host_ready"] = True

    monkeypatch.setattr(client, "open", open_pipe, raising=False)
    monkeypatch.setattr(client, "_host_endpoint", lambda: endpoint)
    monkeypatch.setattr(client, "_launch_host", slow_launch)
    monkeypatch.setattr(client, "_validate_server_binary", lambda _stream, _endpoint: None)
    monkeypatch.setattr(client.time, "monotonic", lambda: clock["now"])
    monkeypatch.setattr(client.time, "sleep", lambda seconds: clock.__setitem__("now", clock["now"] + seconds))

    assert client._connect_pipe() is stream
    assert attempts == [0.0, 60.05]


def test_cached_host_pipe_failure_keeps_the_original_five_second_timeout(monkeypatch: pytest.MonkeyPatch) -> None:
    client = _load_client()
    clock = {"now": 0.0}
    launch_count = 0
    endpoint = client._HostEndpoint(Path("host.exe"), VERSION, "a" * 64, r"\\.\pipe\cached-host")

    def missing_pipe(*_args, **_kwargs):
        raise FileNotFoundError("pipe is not ready")

    def launch_cached_host(_binary: Path) -> None:
        nonlocal launch_count
        launch_count += 1

    monkeypatch.setattr(client, "open", missing_pipe, raising=False)
    monkeypatch.setattr(client, "_host_endpoint", lambda: endpoint)
    monkeypatch.setattr(client, "_launch_host", launch_cached_host)
    monkeypatch.setattr(client.time, "monotonic", lambda: clock["now"])
    monkeypatch.setattr(client.time, "sleep", lambda seconds: clock.__setitem__("now", clock["now"] + seconds))

    with pytest.raises(client.UiControlHostError, match="named pipe is unavailable"):
        client._connect_pipe()
    assert launch_count == 1
    assert 5.0 <= clock["now"] < 5.1


def test_failed_cold_resolver_does_not_start_a_pipe_wait(monkeypatch: pytest.MonkeyPatch) -> None:
    client = _load_client()
    clock = {"now": 0.0}
    endpoint = client._HostEndpoint(Path("host.exe"), VERSION, "a" * 64, r"\\.\pipe\failed-cold-host")

    def missing_pipe(*_args, **_kwargs):
        raise FileNotFoundError("pipe is not ready")

    def failed_launch(_binary: Path) -> None:
        clock["now"] += 30.0
        raise client.UiControlHostError("backend_unavailable", "cold resolver failed")

    monkeypatch.setattr(client, "open", missing_pipe, raising=False)
    monkeypatch.setattr(client, "_host_endpoint", lambda: endpoint)
    monkeypatch.setattr(client, "_launch_host", failed_launch)
    monkeypatch.setattr(client.time, "monotonic", lambda: clock["now"])
    monkeypatch.setattr(client.time, "sleep", lambda _seconds: pytest.fail("failed resolver must not wait for a pipe"))

    with pytest.raises(client.UiControlHostError, match="cold resolver failed"):
        client._connect_pipe()
    assert clock["now"] == 30.0


def test_01965_ignores_legacy_pipe_and_launches_identity_endpoint_on_first_call(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _load_client()
    binary = tmp_path / "dcc-mcp-ui-control-host.exe"
    binary.write_bytes(b"MZ0.19.65-host")
    sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    monkeypatch.setattr(client, "_windows_session_id", lambda: 42)
    monkeypatch.setattr(client, "_host_version", lambda: VERSION)
    monkeypatch.setattr(client, "_host_binary", lambda: binary)
    monkeypatch.setattr(client, "_host_identity", lambda _binary: sha256)
    endpoint = client._host_endpoint()
    legacy_pipe = r"\\.\pipe\dcc-mcp-ui-control-host-v2-session-42"
    legacy_stream = object()
    new_stream = object()
    available = {legacy_pipe: legacy_stream}
    opened: list[str] = []

    def open_pipe(path: str, *_args, **_kwargs):
        opened.append(path)
        if path not in available:
            raise FileNotFoundError("new Host is not ready")
        return available[path]

    def launch_new_host(selected: Path) -> None:
        assert selected == binary
        available[endpoint.pipe_path] = new_stream

    monkeypatch.setattr(client, "_host_endpoint", lambda: endpoint)
    monkeypatch.setattr(client, "open", open_pipe, raising=False)
    monkeypatch.setattr(client, "_launch_host", launch_new_host)
    monkeypatch.setattr(
        client,
        "_validate_server_binary",
        lambda stream, selected: (
            (stream is new_stream and selected == endpoint) or pytest.fail("legacy pipe must never be authenticated")
        ),
    )
    monkeypatch.setattr(client.time, "sleep", lambda _seconds: None)

    assert client._connect_pipe() is new_stream
    assert endpoint.pipe_path != legacy_pipe
    assert opened == [endpoint.pipe_path, endpoint.pipe_path]
    assert available[legacy_pipe] is legacy_stream


def test_same_version_binary_digest_separates_env_and_fallback_endpoints(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _load_client()
    fallback = tmp_path / "fallback.exe"
    configured = tmp_path / "configured.exe"
    copied = tmp_path / "copied.exe"
    fallback.write_bytes(b"MZfallback-0.19.65")
    configured.write_bytes(b"MZconfigured-0.19.65")
    copied.write_bytes(fallback.read_bytes())
    selected = {"path": fallback}
    monkeypatch.setattr(client, "_windows_session_id", lambda: 42)
    monkeypatch.setattr(client, "_host_version", lambda: VERSION)
    monkeypatch.setattr(client, "_host_binary", lambda: selected["path"])
    monkeypatch.setattr(client, "_host_identity", client._RESOLVER.ui_control_host_identity)

    fallback_endpoint = client._host_endpoint()
    selected["path"] = configured
    configured_endpoint = client._host_endpoint()
    selected["path"] = copied
    copied_endpoint = client._host_endpoint()

    assert fallback_endpoint.pipe_path != configured_endpoint.pipe_path
    assert fallback_endpoint.sha256 != configured_endpoint.sha256
    assert fallback_endpoint.pipe_path == copied_endpoint.pipe_path
    assert fallback_endpoint.sha256 == copied_endpoint.sha256
    assert len(fallback_endpoint.sha256) == 64
    assert len(fallback_endpoint.pipe_path) < 256


def test_python_and_rust_discovery_endpoint_contracts_are_isomorphic() -> None:
    source = RUST_HOST_PATH.read_text(encoding="utf-8")

    assert "v{UI_CONTROL_HOST_PROTOCOL_VERSION}-version-{version}-sha256-{sha256}-session-{session_id}" in source
    assert r'format!(r"\\.\pipe\dcc-mcp-ui-control-host-{suffix}")' in source
    assert r'format!(r"Local\dcc-mcp-ui-control-host-{suffix}")' in source
    assert "_PROTOCOL_VERSION}-version-{version}" in CLIENT_PATH.read_text(encoding="utf-8")


def test_connected_server_image_accepts_same_digest_copy_and_rejects_squatting(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _load_client()
    expected = tmp_path / "expected.exe"
    copied = tmp_path / "copied.exe"
    squatter = tmp_path / "squatter.exe"
    expected.write_bytes(b"MZsame-version-image")
    copied.write_bytes(expected.read_bytes())
    squatter.write_bytes(b"MZdifferent-image")
    sha256 = client._RESOLVER.ui_control_host_identity(expected)
    endpoint = client._HostEndpoint(expected, VERSION, sha256, r"\\.\pipe\identity")
    probes: list[tuple[Path, str]] = []
    monkeypatch.setattr(
        client._RESOLVER,
        "_validate_host_version",
        lambda path, version: probes.append((path, version)),
    )

    client._validate_server_image(copied, endpoint)
    assert probes == [(copied, VERSION)]

    with pytest.raises(client.UiControlHostError, match="does not match the resolved"):
        client._validate_server_image(squatter, endpoint)

    def wrong_version(_path: Path, _version: str) -> None:
        raise client._RESOLVER.HostResolutionError("version mismatch")

    monkeypatch.setattr(client._RESOLVER, "_validate_host_version", wrong_version)
    with pytest.raises(client.UiControlHostError, match="does not match the resolved"):
        client._validate_server_image(copied, endpoint)


@pytest.mark.parametrize(
    "version",
    ["0.19.65", "1.2.3-alpha.1+build.5", "10.20.30-0", "1.0.0+001"],
)
def test_discovery_version_accepts_strict_semver(version: str) -> None:
    resolver = _load_resolver()
    assert resolver._strict_release_version(version) == version


@pytest.mark.parametrize(
    "version",
    [
        "01.19.65",
        "0.019.65",
        "0.19.065",
        "0.19.65-01",
        "0.19.65/other",
        "0.19.65\\other",
        "v0.19.65",
        "0.19.65 ",
        "0.19.65+",
        "0.19.65-alpha..1",
    ],
)
def test_discovery_version_rejects_noncanonical_or_unsafe_semver(version: str) -> None:
    resolver = _load_resolver()
    with pytest.raises(resolver.HostResolutionError):
        resolver._strict_release_version(version)


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


@pytest.mark.parametrize("configured", ["relative\\dcc-mcp-ui-control-host.exe"])
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


@pytest.mark.parametrize("configured", ["", "  "])
def test_blank_environment_uses_the_release_download_route(
    configured: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    resolver = _load_resolver()
    expected = tmp_path / "downloaded-host.exe"
    monkeypatch.setenv(resolver.HOST_ENV, configured)
    monkeypatch.setattr(resolver, "_package_version", lambda: VERSION)
    monkeypatch.setattr(resolver, "_downloaded_host", lambda version: expected if version == VERSION else None)
    monkeypatch.setattr(resolver, "_validate_host_file", lambda *_args: pytest.fail("blank env is not an override"))

    assert resolver.resolve_ui_control_host() == expected


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
