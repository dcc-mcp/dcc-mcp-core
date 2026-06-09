"""Lifecycle controller for :class:`DccServerBase`.

Extracted from ``server_base.py`` (PIP-688) to own start/stop, gateway
failover election, gateway metadata management, gateway promotion, and
plugin manifest generation.

Coordinates the existing ``ServerLifecycleController`` (quit hooks) and
``ServerRuntimeController`` (gateway daemon/election).

``DccServerBase`` keeps thin public wrappers that delegate here.
"""

from __future__ import annotations

import atexit
import contextlib
import json
import logging
from typing import Any
import weakref

from dcc_mcp_core._server.lifecycle import ServerLifecycleController
from dcc_mcp_core._server.runtime import ServerRuntimeController
from dcc_mcp_core.plugin_manifest import build_plugin_manifest

logger = logging.getLogger(__name__)

_PKG_VERSION: str = "0.0.0-dev"


class LifecycleController:
    """Owns start/stop/gateway lifecycle for one ``DccServerBase`` instance.

    This is the *high-level* lifecycle orchestrator — distinct from
    ``ServerLifecycleController`` which only manages quit hooks.
    """

    def __init__(self, owner: Any) -> None:
        self._owner = owner
        # Ensure the low-level collaborator is wired onto the owner.
        owner_dict = owner.__dict__
        if "_lifecycle" not in owner_dict:
            owner._lifecycle = ServerLifecycleController(owner)
        if "_runtime" not in owner_dict:
            owner._runtime = ServerRuntimeController(owner)

    @staticmethod
    def _stop_from_atexit(ref: weakref.ReferenceType[Any]) -> None:
        server = ref()
        if server is not None:
            server.stop()

    def _lifecycle_ctrl(self) -> ServerLifecycleController:
        owner_dict = self._owner.__dict__
        if "_lifecycle" not in owner_dict:
            owner_dict["_lifecycle"] = ServerLifecycleController(self._owner)
        return owner_dict["_lifecycle"]

    def _runtime_ctrl(self) -> ServerRuntimeController:
        owner_dict = self._owner.__dict__
        if "_runtime" not in owner_dict:
            owner_dict["_runtime"] = ServerRuntimeController(self._owner)
        return owner_dict["_runtime"]

    # -- quit hooks (delegate to ServerLifecycleController) -------------------

    def register_quit_hook(self, callback) -> Any:
        return self._lifecycle_ctrl().register_quit_hook(callback)

    def unregister_quit_hook(self, callback) -> bool:
        return self._lifecycle_ctrl().unregister_quit_hook(callback)

    def _run_quit_hooks(self) -> None:
        self._lifecycle_ctrl().run_quit_hooks(dcc_name=self._owner._dcc_name)

    # -- start / stop ---------------------------------------------------------

    def start(self, *, install_atexit_hook: bool = True) -> Any:
        """Start the MCP HTTP server."""
        owner = self._owner
        if owner._handle is not None:
            logger.warning(
                "[%s] Server already running on port %d",
                owner._dcc_name,
                owner._handle.port,
            )
            return owner._handle

        self._lifecycle_ctrl().prepare_start(
            install_atexit_hook=install_atexit_hook,
            stop_from_atexit=LifecycleController._stop_from_atexit,
            atexit_register=atexit.register,
        )

        # Initialise in-process metrics just before start.
        owner._init_telemetry()

        self._runtime_ctrl().ensure_gateway_daemon_if_needed()
        owner._stage_gateway_runtime_metadata()
        owner._handle = owner._server.start()
        server_version = getattr(owner._config, "server_version", _PKG_VERSION)
        logger.info(
            "[%s] MCP server v%s started at %s",
            owner._dcc_name,
            server_version,
            owner._handle.mcp_url(),
        )
        self._runtime_ctrl().start_gateway_guardian_if_needed()
        self._runtime_ctrl().start_gateway_election_if_needed()

        return owner._handle

    def stop(self) -> None:
        """Gracefully stop the server and gateway election thread."""
        owner = self._owner
        self._run_quit_hooks()
        self._runtime_ctrl().stop_gateway_guardian()
        self._runtime_ctrl().stop_gateway_election()

        if owner._hot_reloader is not None:
            with contextlib.suppress(Exception):
                owner._hot_reloader.disable()
        self._runtime_ctrl().shutdown_server_handle()

    # -- gateway runtime metadata ---------------------------------------------

    def _gateway_runtime_metadata(self) -> dict[str, str]:
        owner = self._owner
        guardian_running = getattr(owner, "_gateway_guardian", None) is not None
        runtime_mode = str(getattr(owner, "_gateway_runtime_mode", "unknown") or "unknown")
        gateway_recovery_driver = "none"
        if guardian_running:
            gateway_recovery_driver = "daemon_guardian"
        elif runtime_mode == "embedded-fallback":
            gateway_recovery_driver = "embedded_election"
        meta: dict[str, str] = {
            "gateway_runtime_mode": runtime_mode,
            "gateway_guardian_enabled": str(bool(guardian_running)).lower(),
            "gateway_recovery_driver": gateway_recovery_driver,
            "registration_refresh_mode": "file_registry_heartbeat",
        }
        # Surface gateway_daemon_status reason for embedded-fallback visibility.
        if runtime_mode == "embedded-fallback":
            daemon_status = getattr(owner, "_gateway_daemon_status", None)
            if daemon_status:
                meta["gateway_daemon_status_reason"] = str(
                    daemon_status.get("reason", "unknown")
                )
                meta["gateway_daemon_status_ok"] = str(
                    bool(daemon_status.get("ok", False))
                ).lower()
                # Include full status as JSON for debugging.
                with contextlib.suppress(TypeError, ValueError):
                    meta["gateway_daemon_status"] = json.dumps(
                        daemon_status, default=str
                    )
        return meta

    def _stage_gateway_runtime_metadata(self) -> None:
        owner = self._owner
        metadata = getattr(owner._config, "instance_metadata", None)
        if isinstance(metadata, dict):
            updated = dict(metadata)
            updated.update(self._gateway_runtime_metadata())
            try:
                owner._config.instance_metadata = updated
            except Exception as exc:
                logger.debug("[%s] config.instance_metadata update failed: %s", owner._dcc_name, exc)
                metadata.update(updated)

    def _publish_gateway_runtime_metadata(self) -> None:
        owner = self._owner
        self._stage_gateway_runtime_metadata()
        handle = owner._handle
        if handle is None:
            return
        update = getattr(handle, "update_gateway_metadata", None)
        if update is None:
            return
        try:
            update(self._gateway_runtime_metadata())
        except Exception as exc:
            logger.debug("[%s] handle.update_gateway_metadata failed: %s", owner._dcc_name, exc)

    # -- gateway promotion ----------------------------------------------------

    def _upgrade_to_gateway(self) -> bool:
        """Promote this instance to the active gateway by re-running bind."""
        owner = self._owner
        if owner.is_gateway:
            return True

        gateway_port = getattr(owner._config, "gateway_port", 0)
        if not gateway_port or gateway_port <= 0:
            logger.debug(
                "[%s] Cannot promote to gateway: gateway_port is not configured",
                owner._dcc_name,
            )
            return False

        old_handle = owner._handle
        if old_handle is not None:
            with contextlib.suppress(Exception):
                old_handle.shutdown()
            owner._handle = None

        try:
            owner._handle = owner._server.start()
        except Exception as exc:
            logger.error("[%s] Gateway promotion restart failed: %s", owner._dcc_name, exc)
            owner._handle = None
            return False

        promoted = bool(getattr(owner._handle, "is_gateway", False))
        if promoted:
            logger.info("[%s] Gateway promotion succeeded (re-bound on %d)", owner._dcc_name, gateway_port)
        else:
            logger.info(
                "[%s] Gateway promotion attempted but another instance won the bind; running as plain instance",
                owner._dcc_name,
            )
        return promoted

    # -- gateway metadata update ----------------------------------------------

    def update_gateway_metadata(
        self,
        scene: str | None = None,
        version: str | None = None,
        documents: list[str] | None = None,
        display_name: str | None = None,
    ) -> bool:
        """Update instance metadata in the gateway registry."""
        owner = self._owner
        if not owner.is_running:
            logger.warning("[%s] Cannot update metadata: server is not running", owner._dcc_name)
            return False

        gateway_port = getattr(owner._config, "gateway_port", 0)
        if gateway_port <= 0:
            logger.debug("[%s] Gateway not configured; metadata update skipped", owner._dcc_name)
            return False

        try:
            if scene is not None:
                owner._config.scene = scene
            if version is not None:
                owner._config.dcc_version = version
            if owner._handle is not None:
                try:
                    owner._handle.update_scene(scene, version, documents, display_name)
                except Exception as exc_inner:
                    logger.debug("[%s] handle.update_scene failed: %s", owner._dcc_name, exc_inner)
            return True
        except Exception as exc:
            logger.error("[%s] Failed to update gateway metadata: %s", owner._dcc_name, exc)
            return False

    # -- gateway election status ----------------------------------------------

    def get_gateway_election_status(self) -> dict:
        """Return gateway election thread status."""
        owner = self._owner
        gateway_port = int(getattr(owner._config, "gateway_port", 0) or 0)
        is_gateway = bool(getattr(owner, "is_gateway", False))
        gateway_metadata = self._gateway_runtime_metadata()
        if owner._gateway_election is None:
            return {
                "enabled": bool(owner._enable_gateway_failover),
                "running": False,
                "consecutive_failures": 0,
                "gateway_host": None,
                "gateway_port": gateway_port,
                "is_gateway": is_gateway,
                "gateway_runtime_mode": getattr(owner, "_gateway_runtime_mode", "unknown"),
                "gateway_recovery_driver": gateway_metadata["gateway_recovery_driver"],
                "registration_refresh_mode": gateway_metadata["registration_refresh_mode"],
                "gateway_daemon_status": dict(getattr(owner, "_gateway_daemon_status", {}) or {}),
            }
        status = owner._gateway_election.get_status()
        status["enabled"] = bool(owner._enable_gateway_failover)
        status.setdefault("gateway_port", gateway_port)
        status["is_gateway"] = is_gateway
        status["gateway_runtime_mode"] = getattr(owner, "_gateway_runtime_mode", "unknown")
        status["gateway_recovery_driver"] = gateway_metadata["gateway_recovery_driver"]
        status["registration_refresh_mode"] = gateway_metadata["registration_refresh_mode"]
        status["gateway_daemon_status"] = dict(getattr(owner, "_gateway_daemon_status", {}) or {})
        return status

    # -- plugin manifest ------------------------------------------------------

    def plugin_manifest(
        self,
        *,
        version: str | None = None,
        extra_mcp_servers: list[dict] | None = None,
    ) -> dict:
        """Generate a Claude Code plugin manifest for this server."""
        owner = self._owner
        if not owner.is_running:
            raise RuntimeError(
                f"{owner._dcc_name}: Cannot generate plugin manifest — server is not running. "
                "Call server.start() first."
            )

        global _PKG_VERSION
        return build_plugin_manifest(
            dcc_name=owner._dcc_name,
            mcp_url=owner.mcp_url,
            skill_paths=owner.collect_skill_search_paths(),
            version=version or _PKG_VERSION,
            extra_mcp_servers=extra_mcp_servers,
        )
