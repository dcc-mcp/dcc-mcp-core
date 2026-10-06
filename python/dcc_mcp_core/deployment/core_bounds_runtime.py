"""Startup-time core requirement check for :class:`~dcc_mcp_core.server_base.DccServerBase`.

Why this module exists
----------------------
:mod:`dcc_mcp_core.deployment.core_bounds` defines the version-bound contract
and :func:`~dcc_mcp_core.deployment.core_bounds.check_runtime` compares a
declaration against the running core — but nothing inside a DCC host ever
called it, so the runtime half of the contract was dead code. The incident the
contract was written for is a *runtime* failure: an adapter ships
``>=0.19.3,<0.19.5``, the environment resolves a ``0.20.28`` core, top-level
imports keep working, and the break only surfaces several modules deep at
``dcc_mcp_core.dcc_server`` without naming a single version number.

This module closes that gap. It reads the range the *adapter* declares through
:func:`~dcc_mcp_core.deployment.core_bounds.installed_core_requirement`, feeds
it to :func:`~dcc_mcp_core.deployment.core_bounds.check_runtime`, and turns an
out-of-range verdict into one warning that names the adapter, the declared
range, the running core, and the fix.

No second parser, no second contract file
-----------------------------------------
Every comparison is delegated to :mod:`core_bounds`, so violation codes stay
the ones already published in ``compatibility/core-bounds.json``
(``upper_bound_too_wide`` and friends). This module only decides *what to do*
with a verdict: format a message, and warn or raise.

Default is warn, never raise
---------------------------
The :mod:`core_bounds` contract never raises, and a DCC host is the worst
place to start: raising during import would abort an adapter that is already
running inside a live artist session. Operators who want a hard failure opt in
with ``DCC_MCP_CORE_REQUIREMENT_ENFORCE=1``
(:data:`~dcc_mcp_core.constants.ENV_CORE_REQUIREMENT_ENFORCE`).

Missing or unreadable adapter metadata is not a finding. Adapters resolved by
a package environment (Rez) declare their core request outside Python
metadata, and a source checkout has no ``.dist-info`` at all — both must start
silently rather than warn about metadata that was never expected to exist.

Python 3.7 support (Maya 2022)
------------------------------
Stdlib-only, no 3.8+ syntax, and no import-time side effects: the module
touches ``sys.path`` only when a caller asks for a check.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import logging
from typing import Any
from typing import Dict
from typing import Optional

# Import local modules
from dcc_mcp_core.constants import ENV_CORE_REQUIREMENT_ENFORCE
from dcc_mcp_core.deployment.core_bounds import VERDICT_CORE_NEWER_THAN_DECLARED
from dcc_mcp_core.deployment.core_bounds import VERDICT_CORE_OLDER_THAN_DECLARED
from dcc_mcp_core.deployment.core_bounds import VERDICT_DECLARATION_UNUSABLE
from dcc_mcp_core.deployment.core_bounds import VERDICT_SUPPORTED
from dcc_mcp_core.deployment.core_bounds import check_runtime
from dcc_mcp_core.deployment.core_bounds import installed_core_requirement
from dcc_mcp_core.env import env_flag

__all__ = [
    "CoreRequirementCheck",
    "check_adapter_core_requirement",
    "core_requirement_enforced",
    "describe_core_requirement",
]

logger = logging.getLogger(__name__)


class CoreRequirementCheck:
    """The outcome of one startup core requirement check.

    ``enforce`` makes the check fatal; by default the caller only logs
    :attr:`message`.
    """

    __slots__ = ("adapter", "declaration", "enforce", "message", "report", "running", "verdict")

    def __init__(
        self,
        *,
        adapter: str,
        declaration: Optional[str],
        running: str,
        report: Optional[Dict[str, Any]],
        enforce: bool,
    ) -> None:
        self.adapter = adapter
        self.declaration = declaration
        self.running = running
        self.report = report
        self.enforce = enforce
        self.verdict = str(report["verdict"]) if report else None
        self.message = describe_core_requirement(self)

    @property
    def skipped(self) -> bool:
        """True when there was no declaration to check."""
        return self.report is None

    @property
    def ok(self) -> bool:
        """True when the running core is inside the declared range."""
        return bool(self.report and self.report["ok"])

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-serialisable summary for diagnostics."""
        return {
            "adapter": self.adapter,
            "declaration": self.declaration,
            "running": self.running,
            "verdict": self.verdict,
            "ok": self.ok,
            "skipped": self.skipped,
            "enforce": self.enforce,
            "message": self.message,
            "report": self.report,
        }

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"CoreRequirementCheck(adapter={self.adapter!r}, verdict={self.verdict!r}, ok={self.ok!r})"


def core_requirement_enforced() -> bool:
    """Return whether an out-of-range core must fail startup instead of warn."""
    return env_flag(ENV_CORE_REQUIREMENT_ENFORCE, False)


def describe_core_requirement(check: CoreRequirementCheck) -> Optional[str]:
    """Return the operator-facing message for a check, or ``None`` when quiet.

    The message always names the adapter, the range it declared, and the core
    actually running, so it can be acted on without another lookup. A verdict
    of :data:`~dcc_mcp_core.deployment.core_bounds.VERDICT_DECLARATION_UNUSABLE`
    is a policy problem in the adapter's own metadata rather than a version
    mismatch, so it points at the declaration instead of the core install.
    """
    if check.report is None:
        return None
    verdict = check.verdict
    if verdict == VERDICT_SUPPORTED:
        return None

    declared = check.declaration or "no dcc-mcp-core requirement"
    if verdict == VERDICT_DECLARATION_UNUSABLE:
        bound = check.report.get("bound") or {}
        detail = bound.get("message") or "does not declare a usable bounded range"
        suggestion = bound.get("suggestion")
        message = (
            f"adapter '{check.adapter}' declares '{declared}' on dcc-mcp-core, which {detail}. "
            f"dcc-mcp-core {check.running} is running. Narrow the adapter requirement"
        )
        if suggestion:
            message = f"{message} to '{suggestion}'"
        return message + "."

    if verdict == VERDICT_CORE_NEWER_THAN_DECLARED:
        direction = "newer than"
        fix = (
            f"upgrade the adapter to a release whose requirement admits {check.running}, "
            f"or pin dcc-mcp-core to a version inside '{declared}'"
        )
    elif verdict == VERDICT_CORE_OLDER_THAN_DECLARED:
        direction = "older than"
        fix = f"install dcc-mcp-core inside '{declared}' (currently {check.running})"
    else:
        direction = "unverifiable against"
        fix = "report the adapter, declared range, and running core version in a bug report"

    return (
        f"adapter '{check.adapter}' declares '{declared}' on dcc-mcp-core, but dcc-mcp-core {check.running} "
        f"is running, which is {direction} that range. Every core minor bump may break an adapter; {fix}"
    )


def check_adapter_core_requirement(
    adapter: str,
    running_core: str,
    *,
    enforce: Optional[bool] = None,
) -> CoreRequirementCheck:
    """Compare the adapter's declared core range against the running core.

    Args:
        adapter: Installed distribution name of the adapter (``dcc-mcp-maya``).
        running_core: Version of the core actually executing.
        enforce: Force the warn/raise decision; defaults to
            :func:`core_requirement_enforced`.

    Returns:
        A :class:`CoreRequirementCheck`. Missing metadata or an unreadable
        declaration yields ``skipped=True`` and no message.

    Raises:
        RuntimeError: Only when the verdict is a mismatch and enforcement is
            on. Skipped and in-range checks never raise.

    """
    try:
        declaration = installed_core_requirement(adapter)
    except Exception as exc:
        # Metadata reading walks `sys.path`; a broken entry must not take the
        # DCC host down with it.
        logger.debug("[%s] core requirement metadata unavailable: %s", adapter, exc)
        declaration = None

    if not declaration:
        return CoreRequirementCheck(
            adapter=adapter,
            declaration=declaration,
            running=running_core,
            report=None,
            enforce=bool(enforce if enforce is not None else core_requirement_enforced()),
        )

    report = check_runtime(declaration, running_core)
    check = CoreRequirementCheck(
        adapter=adapter,
        declaration=declaration,
        running=running_core,
        report=report,
        enforce=bool(enforce if enforce is not None else core_requirement_enforced()),
    )
    return check


def run_startup_core_requirement_check(
    adapter: str,
    running_core: str,
    logger_: Optional[logging.Logger] = None,
) -> CoreRequirementCheck:
    """Run :func:`check_adapter_core_requirement` and log or raise the outcome.

    This is the single call :class:`DccServerBase` makes on its startup path.
    It never raises for a skipped or in-range check; an enforced mismatch
    raises :class:`RuntimeError`.
    """
    sink = logger_ if logger_ is not None else logger
    check = check_adapter_core_requirement(adapter, running_core)
    if check.message is None:
        return check
    if check.enforce:
        raise RuntimeError(check.message)
    sink.warning("%s", check.message)
    return check
