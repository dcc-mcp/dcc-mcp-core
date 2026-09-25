"""Patrol loop for the standalone gateway guardian.

Split out of ``gateway_guardian.py`` so the guardian keeps one responsibility:
deciding *how* to (re)start the gateway. Running that decision on a timer --
interval wait, re-ensure jitter, failure counting, crash reporting, and status
publishing -- is a separate concern and lives here.
"""

from __future__ import annotations

import contextlib
import logging
import random
import threading
import time
from typing import Any
from typing import Callable

from dcc_mcp_core.constants import ENV_GATEWAY_ENSURE_TIMEOUT_SECS
from dcc_mcp_core.constants import ENV_GATEWAY_GUARDIAN_FAILURES
from dcc_mcp_core.constants import ENV_GATEWAY_GUARDIAN_INTERVAL
from dcc_mcp_core.constants import ENV_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX
from dcc_mcp_core.constants import ENV_GATEWAY_GUARDIAN_RESTART_TIMEOUT
from dcc_mcp_core.constants import ENV_GATEWAY_GUARDIAN_TIMEOUT
from dcc_mcp_core.env import env_float
from dcc_mcp_core.env import env_int

logger = logging.getLogger(__name__)

_ENSURE_TIMEOUT_DEFAULT = 15.0
_REENSURE_JITTER_DEFAULT = 2.0


def _guardian():
    """Return the :mod:`gateway_guardian` module.

    Imported lazily because ``gateway_guardian`` re-exports this module's
    :class:`GatewayDaemonGuardian`, so a module-level import would be circular.
    Resolving the module on each call also keeps ``monkeypatch.setattr`` on the
    guardian module effective for the helpers the patrol calls.
    """
    from dcc_mcp_core._server import gateway_guardian

    return gateway_guardian


class GatewayDaemonGuardian:
    """Background guardian that re-ensures the standalone gateway after crashes."""

    def __init__(
        self,
        *,
        gateway_host: str,
        gateway_port: int,
        registry_dir: str | None,
        dcc_type: str,
        probe_interval_secs: float | None = None,
        probe_timeout_secs: float | None = None,
        restart_timeout_secs: float | None = None,
        reensure_jitter_max_secs: float | None = None,
        failure_threshold: int | None = None,
        status_callback: Callable[[dict[str, Any]], None] | None = None,
    ) -> None:
        self.gateway_host = gateway_host
        self.gateway_port = gateway_port
        self.registry_dir = registry_dir
        self.dcc_type = dcc_type
        self.probe_interval_secs = probe_interval_secs or env_float(
            ENV_GATEWAY_GUARDIAN_INTERVAL,
            5.0,
            minimum=0.1,
        )
        self.probe_timeout_secs = probe_timeout_secs or env_float(
            ENV_GATEWAY_GUARDIAN_TIMEOUT,
            0.5,
            minimum=0.1,
        )
        self.restart_timeout_secs = restart_timeout_secs or env_float(
            ENV_GATEWAY_GUARDIAN_RESTART_TIMEOUT,
            env_float(ENV_GATEWAY_ENSURE_TIMEOUT_SECS, _ENSURE_TIMEOUT_DEFAULT, minimum=0.1),
            minimum=0.1,
        )
        self.reensure_jitter_max_secs = max(
            0.0,
            reensure_jitter_max_secs
            if reensure_jitter_max_secs is not None
            else env_float(
                ENV_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX,
                _REENSURE_JITTER_DEFAULT,
                minimum=0.1,
            ),
        )
        self.failure_threshold = max(
            1,
            failure_threshold or env_int(ENV_GATEWAY_GUARDIAN_FAILURES, 2),
        )
        self.status_callback = status_callback
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()
        self._consecutive_failures = 0
        self._restart_attempts = 0
        self._crash_count = 0
        self._last_status: dict[str, Any] = {
            "ok": False,
            "reason": "not_started",
            "guardian_running": False,
            "consecutive_failures": 0,
            "restart_attempts": 0,
            "gateway_host": gateway_host,
            "gateway_port": gateway_port,
        }

    def start(self) -> bool:
        if self.gateway_port <= 0:
            self._publish({"ok": False, "reason": "gateway_port_not_configured"})
            return False
        if self._thread is not None and self._thread.is_alive():
            return True
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._run,
            name=f"dcc-mcp-gateway-guardian-{self.dcc_type}",
            daemon=True,
        )
        self._thread.start()
        self._publish({"ok": True, "reason": "guardian_started", "guardian_running": True})
        return True

    def stop(self, timeout: float = 1.0) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=max(timeout, 0.0))
            self._thread = None
        self._publish({"ok": True, "reason": "guardian_stopped", "guardian_running": False})

    def status(self) -> dict[str, Any]:
        with self._lock:
            status = dict(self._last_status)
        status["guardian_running"] = bool(self._thread is not None and self._thread.is_alive())
        return status

    def probe_once(self, *, apply_reensure_jitter: bool = False) -> dict[str, Any]:
        if self.gateway_port <= 0:
            return self._publish({"ok": False, "reason": "gateway_port_not_configured"})

        if _guardian()._is_application_ready(self.gateway_host, self.gateway_port, timeout=self.probe_timeout_secs):
            self._consecutive_failures = 0
            takeover_result = _guardian()._try_version_takeover(
                gateway_host=self.gateway_host,
                gateway_port=self.gateway_port,
                registry_dir=self.registry_dir,
                dcc_type=self.dcc_type,
                timeout_secs=self.restart_timeout_secs,
                gateway_persist=None,
                gateway_idle_timeout_secs=None,
                server_bin=None,
            )
            if takeover_result is not None:
                self._restart_attempts += 1
                return self._publish(takeover_result)
            return self._publish({"ok": True, "reason": "healthy", "consecutive_failures": 0})

        self._consecutive_failures += 1
        if self._consecutive_failures < self.failure_threshold:
            return self._publish(
                {
                    "ok": False,
                    "reason": "probe_failed",
                    "consecutive_failures": self._consecutive_failures,
                }
            )

        if apply_reensure_jitter:
            jitter = random.uniform(0.0, self.reensure_jitter_max_secs)
            if jitter > 0.0 and self._stop.wait(jitter):
                return self._publish({"ok": False, "reason": "guardian_stopped"})
            if _guardian()._is_application_ready(self.gateway_host, self.gateway_port, timeout=self.probe_timeout_secs):
                self._consecutive_failures = 0
                return self._publish(
                    {
                        "ok": True,
                        "reason": "healthy_after_jitter",
                        "consecutive_failures": 0,
                    }
                )

        self._restart_attempts += 1
        result = _guardian().ensure_gateway_daemon(
            gateway_host=self.gateway_host,
            gateway_port=self.gateway_port,
            registry_dir=self.registry_dir,
            dcc_type=self.dcc_type,
            timeout_secs=self.restart_timeout_secs,
        )
        if result.get("ok"):
            self._consecutive_failures = 0
        return self._publish(result)

    def _run(self) -> None:
        while not self._stop.wait(max(self.probe_interval_secs, 0.1)):
            try:
                self.probe_once(apply_reensure_jitter=True)
            except Exception:
                self._crash_count += 1
                logger.exception(
                    "[gateway_guardian:%s] probe_once crashed (crash #%d)",
                    self.dcc_type,
                    self._crash_count,
                )
                self._publish(
                    {
                        "ok": False,
                        "reason": "guardian_crash",
                        "crash_count": self._crash_count,
                    }
                )

    def _publish(self, update: dict[str, Any]) -> dict[str, Any]:
        payload = {
            "gateway_host": self.gateway_host,
            "gateway_port": self.gateway_port,
            "guardian_running": bool(self._thread is not None and self._thread.is_alive()),
            "consecutive_failures": self._consecutive_failures,
            "restart_attempts": self._restart_attempts,
            "crash_count": self._crash_count,
            "timestamp_ms": int(time.time() * 1000),
            **update,
        }
        with self._lock:
            self._last_status = payload
        if self.status_callback is not None:
            with contextlib.suppress(Exception):
                self.status_callback(dict(payload))
        return payload
