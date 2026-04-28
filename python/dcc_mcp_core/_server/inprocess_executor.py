"""In-process Python skill execution for embedded DCC adapters (issue #521).

Lifts the `_wire_in_process_executor` / `_run_skill_script` pattern that
`dcc-mcp-maya` 0.2.19 implements (~150 LOC in `server.py`) into a
DCC-neutral helper. Every embedded DCC plugin (Maya, Houdini, Unreal,
Blender Python …) needs the exact same flow:

1. Run the skill script in the live DCC interpreter (no subprocess).
2. Route the script through a host dispatcher so it executes on the
   UI thread.
3. Honour the ``main(**params)`` calling convention with the
   ``SystemExit + __mcp_result__`` fallback used by skill authors.
4. Return a JSON-serialisable :class:`ToolResult`-shaped dict.

The actual MCP wiring stays in
:meth:`McpHttpServer.set_in_process_executor` (already shipped, see
issues #464/#465). This module supplies the *executor closure* that
satisfies that callable contract and the dispatcher protocol it routes
through.
"""

# Import built-in modules
from __future__ import annotations

import importlib.util
import logging
from pathlib import Path
import sys
from typing import TYPE_CHECKING
from typing import Any
from typing import Callable
from typing import Mapping
from typing import Protocol
from typing import runtime_checkable
import uuid

if TYPE_CHECKING:
    pass

logger = logging.getLogger(__name__)

__all__ = [
    "BaseDccCallableDispatcher",
    "build_inprocess_executor",
    "run_skill_script",
]


@runtime_checkable
class BaseDccCallableDispatcher(Protocol):
    """Protocol every DCC dispatcher must satisfy to receive in-process calls.

    The dispatcher submits ``func`` to the DCC's UI / main thread (Maya
    deferred queue, Houdini ``hou.session``, Unreal game thread …) and
    returns the script's result. Implementations are free to be
    synchronous (block on a queue) or to dispatch through a futures
    object internally; from the executor's point of view, the call is
    a plain ``func(*args, **kwargs)`` invocation that may take time.

    Concrete dispatchers do not need to inherit from this protocol —
    duck typing is enough — but tagging implementations explicitly
    enables runtime ``isinstance(dispatcher, BaseDccCallableDispatcher)``
    sanity checks.
    """

    def dispatch_callable(
        self,
        func: Callable[..., Any],
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        """Run *func* on the host's main / UI thread; return the result."""
        ...


def run_skill_script(script_path: str, params: Mapping[str, Any]) -> Any:
    """Lazy-import a skill script and call its ``main(**params)``.

    Mirrors the convention skill authors already use:

    * Module is loaded with a unique synthetic name to keep import
      caches from colliding when the same path is loaded twice.
    * ``main`` is the entry point; raise an explicit error if the
      module does not expose it.
    * ``SystemExit`` is intercepted because some DCCs raise it from
      inside ``main`` to bail out of the host's event loop; in that
      case the script is expected to publish a result via
      ``module.__mcp_result__`` before exiting.
    """
    path = Path(script_path)
    if not path.is_file():
        raise FileNotFoundError(f"Skill script not found: {script_path}")

    mod_name = f"_dcc_mcp_inproc_{uuid.uuid4().hex}"
    spec = importlib.util.spec_from_file_location(mod_name, str(path))
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot create import spec for {script_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[mod_name] = module
    try:
        try:
            spec.loader.exec_module(module)
        except SystemExit:
            return getattr(module, "__mcp_result__", None)

        if not hasattr(module, "main"):
            raise AttributeError(
                f"Skill script {script_path!r} does not expose a `main` callable",
            )
        try:
            return module.main(**dict(params))
        except SystemExit:
            return getattr(module, "__mcp_result__", None)
    finally:
        sys.modules.pop(mod_name, None)


def build_inprocess_executor(
    dispatcher: BaseDccCallableDispatcher | None,
    *,
    runner: Callable[[str, Mapping[str, Any]], Any] = run_skill_script,
) -> Callable[[str, Mapping[str, Any]], Any]:
    """Return an executor callable suitable for ``set_in_process_executor``.

    When *dispatcher* is ``None`` (e.g. ``mayapy``, Houdini batch,
    pytest), the executor calls *runner* on the current thread — the
    standalone fallback Maya already implements.

    When *dispatcher* satisfies :class:`BaseDccCallableDispatcher`,
    every script invocation is routed through
    ``dispatcher.dispatch_callable(runner, script_path, params)`` so
    the script executes on the host's UI / main thread regardless of
    which thread MCP request handling lives on.

    Args:
        dispatcher: The host dispatcher, or ``None`` for inline
            execution.
        runner: Override the inner script runner (defaults to
            :func:`run_skill_script`). Mostly useful for tests.

    Returns:
        A ``(script_path, params) -> Any`` callable that
        :meth:`McpHttpServer.set_in_process_executor` accepts.

    """
    if dispatcher is None:
        def _inline(script_path: str, params: Mapping[str, Any]) -> Any:
            return runner(script_path, params)

        return _inline

    def _routed(script_path: str, params: Mapping[str, Any]) -> Any:
        return dispatcher.dispatch_callable(runner, script_path, params)

    return _routed
