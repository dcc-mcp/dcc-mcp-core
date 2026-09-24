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

from dcc_mcp_core._version_util import parse_semver as _parse_semver
from dcc_mcp_core.constants import ENV_CORE_VERSION
from dcc_mcp_core.constants import ENV_SERVER_BIN

logger = logging.getLogger(__name__)


def _server_bin_from_module() -> str:
    """Last-resort lookup through the importable ``dcc_mcp_server`` package.

    This path can resolve a copy that is *not* part of the current environment
    (a stale user-level install, for example), so callers treat it as a
    degraded outcome and log at WARNING level.
    """
    try:
        binary_path = importlib.import_module("dcc_mcp_server").binary_path
    except Exception as exc:
        logger.warning("dcc_mcp_server.binary_path unavailable: %s", exc)
        return ""
    try:
        return str(binary_path())
    except Exception as exc:
        logger.warning("dcc_mcp_server.binary_path failed: %s", exc)
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


# Version pairs already reported, so a periodic re-ensure does not flood logs.
_SERVER_VERSION_DRIFT_WARNED: set = set()


def _warn_on_server_version_drift() -> None:
    """Warn once per process when ``dcc_mcp_server`` drifts from core.

    A managed deployment resolves ``dcc-mcp-core`` and ``dcc-mcp-server`` from
    the same batch. A major.minor mismatch means at least one of the two came
    from outside the resolve (typically a user-level site-packages copy), which
    used to fail silently.
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
        "is required because both ship from one release. The server most likely "
        "comes from an unmanaged location (for example user-level "
        "site-packages) instead of the resolved environment.",
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
    is observable instead of silent.
    """
    explicit = (os.environ.get(ENV_SERVER_BIN) or "").strip()
    if explicit:
        return explicit
    found = shutil.which("dcc-mcp-server")
    if found:
        _warn_on_server_version_drift()
        return found
    logger.warning(
        "dcc-mcp-server is not on PATH; falling back to the dcc_mcp_server "
        "Python package, which may resolve to an unmanaged install."
    )
    from_module = _server_bin_from_module()
    if from_module:
        _warn_on_server_version_drift()
        return from_module
    logger.warning(
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
