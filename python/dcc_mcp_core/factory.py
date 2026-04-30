"""Singleton factory helpers for DCC MCP server instances.

Each DCC adapter maintains a module-level singleton server. This module
provides :func:`create_dcc_server` to eliminate the boilerplate threading
lock + ``None`` check that every adapter would otherwise duplicate.

Usage::

    # blender_adapter/server.py
    import threading
    from pathlib import Path
    from dcc_mcp_core.server_base import DccServerBase
    from dcc_mcp_core.factory import create_dcc_server, make_start_stop

    class BlenderMcpServer(DccServerBase):
        def __init__(self, port=8765, **kwargs):
            super().__init__(
                dcc_name="blender",
                builtin_skills_dir=Path(__file__).parent / "skills",
                port=port,
                **kwargs,
            )

    # Recommended: use make_start_stop for zero-boilerplate adapters
    start_server, stop_server = make_start_stop(
        BlenderMcpServer,
        hot_reload_env_var="DCC_MCP_BLENDER_HOT_RELOAD",
    )

    # Or manually with a list-based holder:
    _holder = [None]
    _lock = threading.Lock()

    def start_server(port=8765, **kwargs):
        return create_dcc_server(
            instance_holder=_holder,
            lock=_lock,
            server_class=BlenderMcpServer,
            port=port,
            **kwargs,
        )

    def stop_server():
        with _lock:
            if _holder[0] is not None:
                _holder[0].stop()
                _holder[0] = None
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import logging
import os
import threading
from typing import Any
from typing import Callable

logger = logging.getLogger(__name__)


def create_dcc_server(
    *,
    instance_holder: list[Any | None],
    lock: threading.Lock,
    server_class: type[Any],
    port: int = 8765,
    dispatcher: Any | None = None,
    dispatcher_factory: Callable[[], Any | None] | None = None,
    register_builtins: bool = True,
    extra_skill_paths: list[str] | None = None,
    include_bundled: bool = True,
    enable_hot_reload: bool = False,
    hot_reload_env_var: str | None = None,
    **server_kwargs: Any,
) -> Any:
    """Create-or-return a singleton DCC MCP server and start it.

    Thread-safe singleton creation pattern extracted from every DCC adapter's
    ``start_server()`` function.

    Args:
        instance_holder: A single-element list used as a mutable reference,
            e.g. ``_instance_holder = [None]``.  The function reads and writes
            ``instance_holder[0]``.
        lock: Module-level ``threading.Lock`` for thread safety.
        server_class: The :class:`~dcc_mcp_core.server_base.DccServerBase`
            subclass to instantiate.
        port: TCP port for the MCP HTTP server.
        dispatcher: Optional pre-created host dispatcher. When supplied and
            ``server_kwargs`` does not already contain ``dispatcher``, it is
            forwarded to ``server_class`` before skill discovery.
        dispatcher_factory: Optional zero-argument factory called while creating
            a new server instance. Use this when the dispatcher should be
            constructed immediately before the server and not on repeated
            ``start_server()`` calls that return an existing singleton.
        register_builtins: If ``True``, call ``register_builtin_actions()``
            after creating the server.
        extra_skill_paths: Additional skill directories.
        include_bundled: Include dcc-mcp-core bundled skills.
        enable_hot_reload: Enable skill hot-reload.  Also respects
            ``hot_reload_env_var`` if set.
        hot_reload_env_var: Environment variable name to check for hot-reload
            override, e.g. ``"DCC_MCP_MAYA_HOT_RELOAD"``.
        **server_kwargs: Keyword arguments forwarded to ``server_class.__init__``.

    Returns:
        ``McpServerHandle`` from the running server's ``.start()`` call.

    Example::

        _holder = [None]
        _lock = threading.Lock()

        def start_server(port=8765, **kwargs):
            return create_dcc_server(
                instance_holder=_holder,
                lock=_lock,
                server_class=MyDccServer,
                port=port,
                **kwargs,
            )

    """
    with lock:
        instance: Any | None = instance_holder[0]
        if instance is None or not instance.is_running:
            effective_dispatcher = dispatcher
            if effective_dispatcher is None and dispatcher_factory is not None:
                effective_dispatcher = dispatcher_factory()
            if effective_dispatcher is not None and "dispatcher" not in server_kwargs:
                server_kwargs["dispatcher"] = effective_dispatcher

            instance = server_class(port=port, **server_kwargs)

            if register_builtins:
                instance.register_builtin_actions(
                    extra_skill_paths=extra_skill_paths,
                    include_bundled=include_bundled,
                )

            # Hot-reload: explicit arg OR environment variable
            hot_reload_active = enable_hot_reload
            if not hot_reload_active and hot_reload_env_var:
                hot_reload_active = os.environ.get(hot_reload_env_var, "0") == "1"

            if hot_reload_active:
                try:
                    if instance.enable_hot_reload():
                        logger.info("[%s] Skill hot-reload enabled", instance._dcc_name)
                    else:
                        logger.warning("[%s] Failed to enable skill hot-reload", instance._dcc_name)
                except Exception as exc:
                    logger.warning("Error enabling hot-reload: %s", exc)

            instance_holder[0] = instance

        return instance_holder[0].start()


def start_embedded_dcc_server(
    *,
    dcc_name: str,
    instance_holder: list[Any | None],
    lock: threading.Lock,
    server_class: type[Any],
    port: int = 8765,
    dispatcher_factory: Callable[[], Any | None] | None = None,
    dispatcher: Any | None = None,
    env_prefix: str | None = None,
    register_builtins: bool = True,
    extra_skill_paths: list[str] | None = None,
    include_bundled: bool = True,
    enable_hot_reload: bool = False,
    hot_reload_env_var: str | None = None,
    **server_kwargs: Any,
) -> Any:
    """Start an embedded DCC server with the safe dispatcher-first ordering.

    This is a small adapter bootstrap for hosts such as Blender, Photoshop,
    Houdini, Unreal, and Maya. It standardizes the lifecycle that adapters used
    to open-code:

    1. Create or receive the host dispatcher.
    2. Construct the ``DccServerBase`` subclass with that dispatcher.
    3. Discover/load skills.
    4. Start the HTTP server and optional gateway registration.

    Args mirror :func:`create_dcc_server`. ``env_prefix`` can be used to infer
    a hot-reload variable such as ``DCC_MCP_BLENDER_HOT_RELOAD``.
    """
    inferred_hot_reload = hot_reload_env_var
    if inferred_hot_reload is None and env_prefix:
        inferred_hot_reload = f"{env_prefix.rstrip('_')}_HOT_RELOAD"

    return create_dcc_server(
        instance_holder=instance_holder,
        lock=lock,
        server_class=server_class,
        port=port,
        dispatcher=dispatcher,
        dispatcher_factory=dispatcher_factory,
        register_builtins=register_builtins,
        extra_skill_paths=extra_skill_paths,
        include_bundled=include_bundled,
        enable_hot_reload=enable_hot_reload,
        hot_reload_env_var=inferred_hot_reload,
        **server_kwargs,
    )


def make_start_stop(
    server_class: type[Any],
    hot_reload_env_var: str | None = None,
    dispatcher_factory: Callable[[], Any | None] | None = None,
) -> tuple[Callable[..., Any], Callable[[], None]]:
    """Generate a ``(start_server, stop_server)`` function pair for a DCC adapter.

    Convenience factory that creates the singleton holder + lock and returns
    ready-to-use ``start_server`` / ``stop_server`` callables.

    Args:
        server_class: The :class:`~dcc_mcp_core.server_base.DccServerBase` subclass.
        hot_reload_env_var: Env var to check for hot-reload (e.g.
            ``"DCC_MCP_BLENDER_HOT_RELOAD"``).
        dispatcher_factory: Optional factory used to create the host dispatcher
            before constructing the server singleton.

    Returns:
        Tuple of ``(start_server_fn, stop_server_fn)``.

    Example::

        start_server, stop_server = make_start_stop(
            BlenderMcpServer,
            hot_reload_env_var="DCC_MCP_BLENDER_HOT_RELOAD",
        )

    """
    _holder: list[Any | None] = [None]
    _lock = threading.Lock()

    def start_server(
        port: int = 8765,
        register_builtins: bool = True,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
        enable_hot_reload: bool = False,
        **kwargs: Any,
    ) -> Any:
        return create_dcc_server(
            instance_holder=_holder,
            lock=_lock,
            server_class=server_class,
            port=port,
            dispatcher_factory=dispatcher_factory,
            register_builtins=register_builtins,
            extra_skill_paths=extra_skill_paths,
            include_bundled=include_bundled,
            enable_hot_reload=enable_hot_reload,
            hot_reload_env_var=hot_reload_env_var,
            **kwargs,
        )

    def stop_server() -> None:
        with _lock:
            if _holder[0] is not None:
                _holder[0].stop()
                _holder[0] = None

    return start_server, stop_server


# ── convenience getter ────────────────────────────────────────────────────────


def get_server_instance(instance_holder: list[Any | None]) -> Any | None:
    """Return the current singleton instance (or ``None`` if not started).

    Args:
        instance_holder: The same list passed to :func:`create_dcc_server`.

    Returns:
        The server instance, or ``None``.

    """
    return instance_holder[0] if instance_holder else None
