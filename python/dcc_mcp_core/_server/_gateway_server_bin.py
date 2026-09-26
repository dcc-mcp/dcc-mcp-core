"""Locate the managed ``dcc-mcp-server`` binary.

Split out of ``gateway_guardian.py`` so the guardian keeps one responsibility:
deciding *whether* to (re)start the gateway. Finding the binary to start, and
deciding whether that binary belongs to the same release batch as the core, is
a separate concern and lives here.
"""

from __future__ import annotations

import importlib
import logging
import os
import re
import shutil
import sys
from typing import Any

from dcc_mcp_core._install_lifecycle_sidecar import _probe_server_binary_version
from dcc_mcp_core._version_util import parse_semver as _parse_semver
from dcc_mcp_core.constants import ENV_CORE_VERSION
from dcc_mcp_core.constants import ENV_SERVER_BIN

logger = logging.getLogger(__name__)

# Once-per-process warning keys. The guardian patrol re-resolves the binary on
# a timer (``probe_interval_secs``, 5s by default), so an unmanaged or missing
# server repeated these lines on every cycle -- roughly 24 records per minute
# per host. One report is enough for an operator to act on.
_WARN_NOT_ON_PATH = "not-on-path"
_WARN_UNAVAILABLE = "unavailable"
_WARN_MODULE_BINARY_PATH_UNAVAILABLE = "module-binary-path-unavailable"
_WARN_MODULE_BINARY_PATH_FAILED = "module-binary-path-failed"

# Why a mismatch is worth acting on. Both are appended to one shared warning
# line so the operator is told *which* signal fired, not just that versions
# differ -- the two probes point at different root causes.
_DRIFT_REASON_MODULE = (
    "This server binary was imported from the dcc_mcp_server package rather "
    "than taken from PATH, so it most likely comes from an unmanaged location "
    "(for example user-level site-packages) instead of the resolved "
    "environment."
)
_DRIFT_REASON_PATH = (
    "This version was reported by the binary found on PATH, so the mismatch is "
    "measured against the executable that will actually run: PATH is resolving "
    "a dcc-mcp-server from a different release batch than this dcc-mcp-core. A "
    "stale install shadowing the resolved one is the usual cause."
)

# ``dcc-mcp-server --version`` prints ``dcc-mcp-server 0.20.36``; keep only the
# version token so it can be parsed as semver.
_VERSION_IN_OUTPUT = re.compile(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?")

# Already-reported ledgers, so the guardian's periodic re-ensure does not
# flood logs: version pairs once per (server, core) pair, resolution outcomes
# once per key above.
_SERVER_VERSION_DRIFT_WARNED: set = set()
_SERVER_BIN_WARNED: set = set()

# Probe results for this process, keyed by resolved path. Resolution runs on the
# guardian's patrol timer, so an unhealthy host would otherwise re-spawn
# ``--version`` every few seconds for the same answer.
_SERVER_BINARY_VERSION_CACHE: dict = {}


def _warn_once(key: str, message: str, *args: Any) -> None:
    """Log *message* at WARNING, at most once per process for a given *key*.

    Resolution runs on the guardian's patrol timer rather than once at startup,
    so a permanently degraded host would otherwise log the same line forever.
    """
    if key in _SERVER_BIN_WARNED:
        return
    _SERVER_BIN_WARNED.add(key)
    logger.warning(message, *args)


def _server_bin_from_module() -> str:
    """Last-resort lookup through the importable ``dcc_mcp_server`` package.

    This path can resolve a copy that is *not* part of the current environment
    (a stale user-level install, for example), so callers treat it as a
    degraded outcome and log at WARNING level.
    """
    try:
        binary_path = importlib.import_module("dcc_mcp_server").binary_path
    except Exception as exc:
        _warn_once(
            _WARN_MODULE_BINARY_PATH_UNAVAILABLE,
            "dcc_mcp_server.binary_path unavailable: %s",
            exc,
        )
        return ""
    try:
        return str(binary_path())
    except Exception as exc:
        _warn_once(
            _WARN_MODULE_BINARY_PATH_FAILED,
            "dcc_mcp_server.binary_path failed: %s",
            exc,
        )
        return ""


def _server_module_version() -> str:
    """Return ``dcc_mcp_server.__version__`` when the package is importable."""
    module = sys.modules.get("dcc_mcp_server")
    if module is None:
        try:
            module = importlib.import_module("dcc_mcp_server")
        except Exception:
            return ""
    return str(getattr(module, "__version__", "") or "")


def _server_binary_version(command: str) -> str:
    """Return the version *command* reports about itself, or ``""``.

    Runs ``<command> --version`` at most once per resolved path per process
    (see ``_SERVER_BINARY_VERSION_CACHE``). Best effort by design: a binary
    that cannot be executed, that predates ``--version``, or that times out
    must not add noise or latency to the launch path -- it just leaves the
    version unknown, which is the same information state as before this probe
    existed.
    """
    try:
        return _SERVER_BINARY_VERSION_CACHE[command]
    except KeyError:
        pass
    version, _error = _probe_server_binary_version(command, os.environ.copy())
    match = _VERSION_IN_OUTPUT.search(version) if version else None
    resolved = match.group(0) if match else ""
    _SERVER_BINARY_VERSION_CACHE[command] = resolved
    return resolved


def _warn_on_version_mismatch(server_version: str, reason: str) -> None:
    """Warn once per process when *server_version* drifts from core.

    A managed deployment resolves ``dcc-mcp-core`` and ``dcc-mcp-server`` from
    the same batch. A major.minor mismatch means at least one of the two came
    from outside the resolve, which used to fail silently.

    ``reason`` names which measurement produced *server_version*, because the
    two measurements point at different root causes and an operator reading the
    log has to know which one fired.

    An unknown core version is not a mismatch: it is absent evidence, so no
    line is logged (see ``_get_core_version``).
    """
    core_version = _get_core_version()
    server_semver = _parse_semver(server_version)
    core_semver = _parse_semver(core_version)
    if server_semver is None or core_semver is None:
        return
    if server_semver[:2] == core_semver[:2]:
        return
    key = (server_version, core_version, reason)
    if key in _SERVER_VERSION_DRIFT_WARNED:
        return
    _SERVER_VERSION_DRIFT_WARNED.add(key)
    logger.warning(
        "dcc-mcp-server %s does not match dcc-mcp-core %s: the same major.minor "
        "is required because both ship from one release. %s",
        server_version,
        core_version,
        reason,
    )


def _warn_on_module_version_drift() -> None:
    """Compare the importable ``dcc_mcp_server`` package against core.

    Only meaningful when the binary came from that package: it is the one case
    where the package version describes the binary that will actually run.
    """
    server_version = _server_module_version()
    if not server_version:
        return
    _warn_on_version_mismatch(server_version, _DRIFT_REASON_MODULE)


def _warn_on_path_binary_version_drift(command: str) -> None:
    """Compare the PATH binary against core by asking the binary itself.

    The importable ``dcc_mcp_server`` package is not evidence here -- under a
    managed resolve it can be a stale copy that nothing executes, which is why
    comparing it reports drift that does not exist. ``--version`` measures the
    executable that is about to be launched instead.
    """
    server_version = _server_binary_version(command)
    if not server_version:
        return
    _warn_on_version_mismatch(server_version, _DRIFT_REASON_PATH)


def _resolve_server_bin() -> str:
    """Locate the ``dcc-mcp-server`` binary, preferring managed locations.

    Resolution order, deliberately putting the unmanaged lookup last:

    1. ``DCC_MCP_SERVER_BIN`` — explicit operator override.
    2. ``shutil.which("dcc-mcp-server")`` — the PATH of the current environment.
       Under a managed (Rez/pip) deployment this is the resolved binary.
    3. ``dcc_mcp_server.binary_path()`` — a Python import, which can silently
       hit a copy that is not part of the resolve.

    Reaching step 3, or finding nothing at all, is logged at WARNING so the gap
    is observable instead of silent. The guardian re-runs this on every patrol
    cycle, so each outcome is reported once per process (see ``_warn_once``)
    rather than once per cycle.
    """
    explicit = (os.environ.get(ENV_SERVER_BIN) or "").strip()
    if explicit:
        return explicit
    found = shutil.which("dcc-mcp-server")
    if found:
        # PATH decides *which* binary runs, but not which release batch it came
        # from -- a stale install earlier on PATH wins the lookup silently. Ask
        # the binary itself rather than the importable dcc_mcp_server package,
        # which under a managed resolve may be an unrelated copy that nothing
        # executes (comparing it reports drift that does not exist).
        _warn_on_path_binary_version_drift(found)
        return found
    _warn_once(
        _WARN_NOT_ON_PATH,
        "dcc-mcp-server is not on PATH; falling back to the dcc_mcp_server "
        "Python package, which may resolve to an unmanaged install.",
    )
    from_module = _server_bin_from_module()
    if from_module:
        # The package supplies the binary here, so its version is evidence.
        _warn_on_module_version_drift()
        return from_module
    _warn_once(
        _WARN_UNAVAILABLE,
        "dcc-mcp-server binary unavailable; set %s to an explicit path.",
        ENV_SERVER_BIN,
    )
    return "dcc-mcp-server"


def _get_core_version() -> str:
    """Return the dcc-mcp-core version string, or ``""`` when unknown.

    Checks ``DCC_MCP_CORE_VERSION`` env var first, then tries to read from the
    installed ``dcc_mcp_core`` package metadata.

    An unresolvable version is reported as empty rather than as a ``0.0.0-dev``
    placeholder. A placeholder parses as ``(0, 0, 0)``, so every drift
    comparison measured a real server binary against a version that was never
    claimed -- and reported a mismatch that does not exist on every host whose
    core ships without distribution metadata. Callers treat ``""`` as unknown
    and skip the comparison instead.
    """
    env_version = (os.environ.get(ENV_CORE_VERSION) or "").strip()
    if env_version:
        return env_version
    try:
        from importlib.metadata import version as _pkg_version

        return _pkg_version("dcc-mcp-core")
    except Exception:
        return ""
