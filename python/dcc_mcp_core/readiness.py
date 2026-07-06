"""Adapter-facing readiness helpers.

``ReadinessProbe`` is the low-level live bitset exposed by the Rust core.
``AdapterReadinessBinder`` adds the adapter lifecycle policy that otherwise
gets repeated in every DCC plugin: publish one probe to the server, mark inline
or headless execution ready, and optionally verify that a UI dispatcher has
pumped at least one scheduled callback before reporting DCC readiness.
"""

from __future__ import annotations

import logging
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Mapping

logger = logging.getLogger(__name__)

READINESS_BASE_BITS: tuple[str, ...] = ("process", "dcc", "skill_catalog", "dispatcher")
READINESS_EXECUTION_BITS: tuple[str, ...] = ("host_execution_bridge", "main_thread_executor")
READINESS_ALL_BITS: tuple[str, ...] = READINESS_BASE_BITS + READINESS_EXECUTION_BITS
DEFAULT_FIRST_PUMP_TIMEOUT_SECS = 2.0


def _new_probe() -> Any:
    try:
        from dcc_mcp_core._core import ReadinessProbe
    except ImportError:
        from dcc_mcp_core._py37_fallback import ReadinessProbe

    return ReadinessProbe()


def _report_mapping(report_or_probe: Any) -> Mapping[str, Any]:
    report = report_or_probe.report() if callable(getattr(report_or_probe, "report", None)) else report_or_probe
    if isinstance(report, Mapping):
        return report
    return dict(report)


def readiness_report_subset(
    report_or_probe: Any,
    keys: Iterable[str] = READINESS_ALL_BITS,
) -> dict[str, bool]:
    """Return a stable subset of readiness bits.

    Tests and downstream adapters can compare this helper instead of asserting
    against the entire ``/v1/readyz`` payload. When core adds a new readiness
    bit later, existing subset assertions keep their contract.
    """
    report = _report_mapping(report_or_probe)
    return {key: bool(report[key]) for key in keys if key in report}


class AdapterReadinessBinder:
    """Bind a shared ``ReadinessProbe`` to an adapter server.

    The binder owns no server lifecycle; it only publishes one probe and flips
    bits according to adapter-observable facts. The same probe is then consumed
    by MCP ``tools/call`` gating and REST ``/v1/readyz`` / ``/v1/call``.
    """

    def __init__(
        self,
        server: Any,
        *,
        probe: Any | None = None,
        dcc_ready_probe: Callable[[], bool] | None = None,
        publish: bool = True,
    ) -> None:
        self.server = server
        self.probe = probe if probe is not None else _new_probe()
        self.dcc_ready_probe = dcc_ready_probe
        self.published = False
        self.first_pump_observed = False
        if publish:
            self.publish()

    @classmethod
    def bind_inline(
        cls,
        server: Any,
        *,
        probe: Any | None = None,
        dcc_ready_probe: Callable[[], bool] | None = None,
    ) -> AdapterReadinessBinder:
        """Publish a probe for inline execution and mark routing ready."""
        binder = cls(server, probe=probe, dcc_ready_probe=dcc_ready_probe)
        binder.mark_inline_ready()
        return binder

    @classmethod
    def bind_headless(
        cls,
        server: Any,
        *,
        probe: Any | None = None,
        dcc_ready_probe: Callable[[], bool] | None = None,
    ) -> AdapterReadinessBinder:
        """Publish a probe for headless execution and mark routing ready."""
        binder = cls(server, probe=probe, dcc_ready_probe=dcc_ready_probe)
        binder.mark_headless_ready()
        return binder

    @classmethod
    def bind_queue_dispatcher(
        cls,
        server: Any,
        dispatcher: Any,
        *,
        probe: Any | None = None,
        dcc_ready_probe: Callable[[], bool] | None = None,
        require_first_pump: bool = False,
        first_pump_timeout_secs: float = DEFAULT_FIRST_PUMP_TIMEOUT_SECS,
    ) -> AdapterReadinessBinder:
        """Publish a probe for a queue-backed UI dispatcher.

        When ``require_first_pump`` is true, DCC and main-thread executor bits
        stay red until a callback posted to ``dispatcher`` actually runs.
        """
        binder = cls(server, probe=probe, dcc_ready_probe=dcc_ready_probe)
        binder.mark_dispatcher_ready(
            True,
            host_execution_bridge_ready=True,
            main_thread_executor_ready=not require_first_pump,
            dcc_ready=False if require_first_pump else None,
        )
        if require_first_pump:
            binder.wait_for_first_pump(dispatcher, timeout_secs=first_pump_timeout_secs)
        else:
            binder.refresh_dcc_ready(default=True)
        return binder

    def publish(self) -> bool:
        """Install the probe on ``server`` if it exposes ``set_readiness_probe``."""
        setter = getattr(self.server, "set_readiness_probe", None)
        if not callable(setter):
            inner = getattr(self.server, "_server", None)
            setter = getattr(inner, "set_readiness_probe", None)
        if not callable(setter):
            logger.debug("readiness probe cannot be published: server has no set_readiness_probe")
            return False
        try:
            setter(self.probe)
        except Exception as exc:
            logger.debug("readiness probe publication failed: %s", exc)
            return False
        self.published = True
        return True

    def mark_inline_ready(self) -> None:
        """Mark inline execution ready on the current thread."""
        self.mark_dispatcher_ready(True, host_execution_bridge_ready=True, main_thread_executor_ready=True)
        self.refresh_dcc_ready(default=True)

    def mark_headless_ready(self) -> None:
        """Mark headless execution ready on the current thread."""
        self.mark_inline_ready()

    def mark_dispatcher_ready(
        self,
        ready: bool,
        *,
        host_execution_bridge_ready: bool | None = None,
        main_thread_executor_ready: bool | None = None,
        dcc_ready: bool | None = None,
    ) -> None:
        """Set dispatcher and bridge bits consistently."""
        self.probe.set_dispatcher_ready(bool(ready))
        if host_execution_bridge_ready is not None:
            self.probe.set_host_execution_bridge_ready(bool(host_execution_bridge_ready))
        if main_thread_executor_ready is not None:
            self.probe.set_main_thread_executor_ready(bool(main_thread_executor_ready))
        if dcc_ready is not None:
            self.probe.set_dcc_ready(bool(dcc_ready))

    def mark_execution_ready(self, ready: bool = True) -> None:
        """Flip both host execution bridge bits together."""
        self.probe.set_host_execution_bridge_ready(bool(ready))
        self.probe.set_main_thread_executor_ready(bool(ready))

    def refresh_dcc_ready(self, *, default: bool = True) -> bool:
        """Evaluate the optional adapter DCC-ready callable and update the bit."""
        ready = self._resolve_dcc_ready(default)
        self.probe.set_dcc_ready(ready)
        return ready

    def wait_for_first_pump(
        self,
        dispatcher: Any,
        *,
        timeout_secs: float = DEFAULT_FIRST_PUMP_TIMEOUT_SECS,
    ) -> bool:
        """Wait for one dispatcher-posted callback to execute.

        Interactive DCC adapters call this during startup when the presence of
        a queue object is not enough: the UI/main-thread pump must have drained
        at least one callback. The method does not drive the pump itself.
        """
        post = getattr(dispatcher, "post", None)
        if not callable(post):
            raise TypeError("dispatcher must expose post(callable)")

        handle = post(lambda: True)
        wait = getattr(handle, "wait", None)
        if not callable(wait):
            raise TypeError("dispatcher.post(callable) must return an object with wait(timeout=...)")

        try:
            wait(timeout=timeout_secs)
        except Exception as exc:
            logger.debug("readiness first-pump check did not complete: %s", exc)
            self.first_pump_observed = False
            self.probe.set_main_thread_executor_ready(False)
            self.probe.set_dcc_ready(False)
            return False

        self.first_pump_observed = True
        self.probe.set_main_thread_executor_ready(True)
        self.refresh_dcc_ready(default=True)
        return True

    def report_subset(self, keys: Iterable[str] = READINESS_ALL_BITS) -> dict[str, bool]:
        """Return a stable subset of this binder's live readiness report."""
        return readiness_report_subset(self.probe, keys=keys)

    def _resolve_dcc_ready(self, default: bool) -> bool:
        if self.dcc_ready_probe is None:
            return bool(default)
        try:
            return bool(self.dcc_ready_probe())
        except Exception as exc:
            logger.warning("adapter DCC readiness probe failed: %s", exc)
            return False


__all__ = [
    "DEFAULT_FIRST_PUMP_TIMEOUT_SECS",
    "READINESS_ALL_BITS",
    "READINESS_BASE_BITS",
    "READINESS_EXECUTION_BITS",
    "AdapterReadinessBinder",
    "readiness_report_subset",
]
