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
import shutil
import sys
from typing import Any

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

# Already-reported ledgers, so the guardian's periodic re-ensure does not
# flood logs: version pairs once per (server, core) pair, resolution outcomes
# once per key above.
_SERVER_VERSION_DRIFT_WARNED: set = set()
_SERVER_BIN_WARNED: set = set()


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


def _warn_on_server_version_drift() -> None:
    """Warn once per process when ``dcc_mcp_server`` drifts from core.

    A managed deployment resolves ``dcc-mcp-core`` and ``dcc-mcp-server`` from
    the same batch. A major.minor mismatch means at least one of the two came
    from outside the resolve (typically a user-level site-packages copy), which
    used to fail silently.

    Only meaningful when the binary came from the importable
    ``dcc_mcp_server`` package: that is the one case where the package version
    describes the binary that will actually run. A binary resolved from PATH or
    from ``DCC_MCP_SERVER_BIN`` carries no importable version at all.
    """
    server_version = _server_module_version()
    if not server_version:
        return
    core_version = _get_core_version()
    server_semver = _parse_semver(server_version)
    core_semver = _parse_semver(core_version)
    if server_semver is None or core_semver is None:
        return
    if server_semver[:2] == core_semver[:2]:
        return
    key = (server_version, core_version)
    if key in _SERVER_VERSION_DRIFT_WARNED:
        return
    _SERVER_VERSION_DRIFT_WARNED.add(key)
    logger.warning(
        "dcc_mcp_server %s does not match dcc-mcp-core %s: the same major.minor "
        "is required because both ship from one release. This server binary was "
        "imported from the dcc_mcp_server package rather than taken from PATH, "
        "so it most likely comes from an unmanaged location (for example "
        "user-level site-packages) instead of the resolved environment.",
        server_version,
        core_version,
    )


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
        # PATH is the resolved environment, so this binary *is* the managed
        # one. The importable dcc_mcp_server package may be an unrelated copy
        # that nothing executes, so comparing its version here would report
        # drift that does not exist and misdirect troubleshooting.
        return found
    _warn_once(
        _WARN_NOT_ON_PATH,
        "dcc-mcp-server is not on PATH; falling back to the dcc_mcp_server "
        "Python package, which may resolve to an unmanaged install.",
    )
    from_module = _server_bin_from_module()
    if from_module:
        # The package supplies the binary here, so its version is evidence.
        _warn_on_server_version_drift()
        return from_module
    _warn_once(
        _WARN_UNAVAILABLE,
        "dcc-mcp-server binary unavailable; set %s to an explicit path.",
        ENV_SERVER_BIN,
    )
    return "dcc-mcp-server"


def _get_core_version() -> str:
    """Return the dcc-mcp-core version string.

    Checks ``DCC_MCP_CORE_VERSION`` env var first, then tries to read from the
    installed ``dcc_mcp_core`` package metadata.
    """
    env_version = (os.environ.get(ENV_CORE_VERSION) or "").strip()
    if env_version:
        return env_version
    try:
        from importlib.metadata import version as _pkg_version

        return _pkg_version("dcc-mcp-core")
    except Exception:
        return "0.0.0-dev"
