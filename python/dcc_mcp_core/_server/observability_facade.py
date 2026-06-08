"""Observability facade for :class:`DccServerBase`.

Extracted from ``server_base.py`` (PIP-688) to own file logging, job
persistence, telemetry, resource publishing, readiness probes, lifecycle
hook dispatch, and skill load persistence.

Delegates to the existing ``FileLoggingManager``, ``JobPersistenceManager``,
and ``TelemetryManager`` from ``_server/observability.py``, and to
``LifecycleEventDispatcher`` from ``_lifecycle_events.py``.

``DccServerBase`` keeps thin public wrappers that delegate here.
"""

from __future__ import annotations

import json as _json
import logging
from typing import Any
from typing import Callable

from dcc_mcp_core._lifecycle_events import LifecycleEventDispatcher
from dcc_mcp_core._server.observability import FileLoggingManager
from dcc_mcp_core._server.observability import JobPersistenceManager
from dcc_mcp_core._server.observability import TelemetryManager
from dcc_mcp_core.adapter_context import append_context_snapshot
from dcc_mcp_core.adapter_context import register_adapter_instruction_resources

logger = logging.getLogger(__name__)


class ObservabilityFacade:
    """Owns observability, resources, readiness, and lifecycle hooks for one server."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    # -- file logging ---------------------------------------------------------

    def init_file_logging(self, dcc_name: str) -> str:
        owner = self._owner
        manager = FileLoggingManager(dcc_name, enabled=owner._enable_file_logging)
        return manager.init()

    # -- job persistence ------------------------------------------------------

    def init_job_persistence(self, dcc_name: str) -> None:
        owner = self._owner
        manager = JobPersistenceManager(
            dcc_name,
            enabled=owner._enable_job_persistence,
            log_dir=owner._log_dir,
        )
        manager.init(owner._config)

    # -- telemetry ------------------------------------------------------------

    def init_telemetry(self) -> None:
        owner = self._owner
        if not owner._enable_telemetry:
            return
        TelemetryManager(owner._dcc_name, owner._dcc_pid, enabled=True).init()

    # -- observability summary ------------------------------------------------

    @property
    def observability_summary(self) -> dict[str, Any]:
        owner = self._owner
        return {
            "file_logging": owner._enable_file_logging,
            "log_dir": owner._log_dir or None,
            "job_persistence": owner._enable_job_persistence,
            "job_db": getattr(owner._config, "job_storage_path", None),
            "telemetry": owner._enable_telemetry,
        }

    # -- resources ------------------------------------------------------------

    def resources(self) -> Any:
        owner = self._owner
        get_resources = getattr(owner._server, "resources", None)
        if not callable(get_resources):
            raise RuntimeError("inner MCP server does not expose resources()")
        return get_resources()

    def register_resource_producer(self, scheme_or_uri: str, producer: Callable[[str], Any]) -> None:
        self.resources().register_producer(scheme_or_uri, producer)

    def set_scene_resource(self, snapshot: Any) -> None:
        self.resources().set_scene(snapshot)

    def notify_resource_updated(self, uri: str) -> None:
        self.resources().notify_updated(uri)

    # -- readiness probe ------------------------------------------------------

    def set_readiness_probe(self, probe: Any) -> bool:
        owner = self._owner
        setter = getattr(owner._server, "set_readiness_probe", None)
        if not callable(setter):
            logger.debug("[%s] set_readiness_probe unavailable on inner server", owner._dcc_name)
            return False
        try:
            setter(probe)
            return True
        except Exception as exc:
            logger.debug("[%s] set_readiness_probe failed: %s", owner._dcc_name, exc)
            return False

    # -- context snapshot -----------------------------------------------------

    def set_context_snapshot_provider(self, provider: Any | None) -> None:
        self._owner._snapshot_provider = provider

    def append_context_snapshot(self, result: dict[str, Any], *, policy: Any | None = None) -> dict[str, Any]:
        if self._owner._snapshot_provider is None:
            return dict(result)
        return append_context_snapshot(result, self._owner._snapshot_provider, policy=policy)

    # -- adapter instructions -------------------------------------------------

    def register_adapter_instructions(self, instruction_set: Any) -> list[str]:
        return register_adapter_instruction_resources(self._owner._server, instruction_set)

    # -- lifecycle hooks ------------------------------------------------------

    def register_lifecycle_hooks(self, hooks: Any) -> Any:
        from dcc_mcp_core.lifecycle_hooks import HookContext
        from dcc_mcp_core.lifecycle_hooks import HookEvent

        owner = self._owner
        owner._lifecycle_hooks = hooks

        def _bridge_before_load(skill: Any) -> Any:
            hooks.dispatch(
                HookContext(
                    event=HookEvent.BEFORE_SKILL_LOAD,
                    dcc_name=owner._dcc_name,
                    payload={"skill_name": getattr(skill, "name", None)},
                )
            )
            return None

        def _bridge_after_load(skill: Any, registered: list[str]) -> None:
            hooks.dispatch(
                HookContext(
                    event=HookEvent.AFTER_SKILL_LOAD,
                    dcc_name=owner._dcc_name,
                    payload={
                        "skill_name": getattr(skill, "name", None),
                        "registered_actions": list(registered),
                    },
                )
            )

        owner.set_skill_load_transform(_bridge_before_load)
        owner.set_after_load_skill_hook(_bridge_after_load)
        return hooks

    def lifecycle_hooks(self) -> Any | None:
        return getattr(self._owner, "_lifecycle_hooks", None)

    def dispatch_lifecycle_event(
        self,
        event: Any,
        payload: dict[str, Any] | None = None,
        *,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        owner = self._owner
        dispatcher = getattr(owner, "_lifecycle_events", None)
        if dispatcher is None:
            dispatcher = LifecycleEventDispatcher(
                owner._dcc_name,
                lambda: getattr(owner, "_lifecycle_hooks", None),
            )
            owner._lifecycle_events = dispatcher
        return dispatcher.dispatch(event, payload=payload, session_id=session_id)

    def dispatch_session_start(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return self.dispatch_lifecycle_event("on_session_start", payload, session_id=session_id)

    def dispatch_session_end(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return self.dispatch_lifecycle_event("on_session_end", payload, session_id=session_id)

    def dispatch_before_tool_call(
        self,
        tool_name: str,
        *,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        event_payload = {"tool_name": tool_name, **(payload or {})}
        return self.dispatch_lifecycle_event("before_tool_call", event_payload, session_id=session_id)

    def dispatch_after_tool_call(
        self,
        tool_name: str,
        *,
        ok: bool,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        event_payload = {"tool_name": tool_name, "ok": bool(ok), **(payload or {})}
        return self.dispatch_lifecycle_event("after_tool_call", event_payload, session_id=session_id)

    # -- skill load persistence -----------------------------------------------

    def enable_skill_load_persistence(
        self,
        *,
        path: Any | None = None,
        sqlite_mirror: bool = True,
        policy: str = "skip_on_drift",
    ) -> dict[str, Any]:
        """Persist + replay ``SkillCatalog.loaded`` across restarts (#1405)."""
        from dcc_mcp_core.loaded_state_store import LoadedStateStore

        owner = self._owner
        store = LoadedStateStore(owner._dcc_name, path=path, sqlite_mirror=sqlite_mirror)
        owner._loaded_state_store = store

        def _on_after_load(skill: Any, registered: list[str]) -> None:
            name = getattr(skill, "name", None)
            if not name:
                return
            version = getattr(skill, "version", None) or None
            skill_path = getattr(skill, "skill_path", None) or None
            store.record_loaded(name, version=version, skill_path=skill_path)

        def _on_after_unload(skill_name: str, _unregistered: list[str]) -> None:
            store.record_unloaded(skill_name)

        def _on_group_change(group_name: str, activated: bool) -> None:
            store.record_group_change(group_name, activated=activated)

        owner.set_after_load_skill_hook(_on_after_load)
        owner.set_after_unload_skill_hook(_on_after_unload)
        owner.set_after_group_change_hook(_on_group_change)

        snapshot = store.snapshot()
        if not snapshot.skills and not snapshot.active_groups:
            return {
                "store_path": str(store.path),
                "replayed": False,
                "reason": "empty_state",
            }

        report_json = owner._skill_client.replay_loaded_skills(
            _json.dumps(snapshot.to_json()),
            policy=policy,
        )
        if report_json is None:
            return {
                "store_path": str(store.path),
                "replayed": False,
                "reason": "binding_unavailable",
            }
        try:
            report = _json.loads(report_json)
        except _json.JSONDecodeError as exc:
            logger.warning(
                "[%s] enable_skill_load_persistence: failed to parse replay report: %s",
                owner._dcc_name,
                exc,
            )
            report = {}
        return {
            "store_path": str(store.path),
            "replayed": True,
            "policy": policy,
            "report": report,
        }
