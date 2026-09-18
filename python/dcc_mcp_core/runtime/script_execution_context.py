"""Persistent script execution contexts owned by one DCC server instance.

``ScriptExecutionContext`` is the domain object behind DCC script execution. It
owns three things for a single host instance:

- the live DCC globals injected by the adapter (``cmds``, ``hou``, ``bpy``, …);
- the persistent namespace that lets a later ``execute_python`` call read
  variables bound by an earlier one (IDE-style execution);
- the optional scene-digest capability used to prove what one script actually
  changed (issue #2300).

This module is the single home for that object and for the module-level facade
adapters call. ``dcc_mcp_core.script_execution`` re-exports the public names,
so existing imports keep working unchanged.
"""

from __future__ import annotations

import threading
from typing import Any

from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestExecution
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot
from dcc_mcp_core.runtime.scene_digest import StateDigestProvider
from dcc_mcp_core.runtime.scene_digest import snapshot_from_provider
from dcc_mcp_core.runtime.scene_digest_execution import capture_scene_digest_wire as _capture_scene_digest_wire
from dcc_mcp_core.runtime.scene_digest_execution import resolve_scene_digest_transaction as _resolve_digest_transaction

try:
    from dcc_mcp_core._core import _run_with_scene_digest_transaction
except ImportError:  # pragma: no cover - py37-lite has no native transaction boundary
    _run_with_scene_digest_transaction = None


class ScriptExecutionContext:
    """Persistent script globals owned by one DCC server instance."""

    def __init__(self) -> None:
        self._lock = threading.RLock()
        self._dcc_namespace: dict[str, Any] = {}
        self._script_namespace: dict[str, Any] = {}
        self._state_digest_provider: StateDigestProvider | None = None

    def register_dcc_namespace(self, namespace: dict[str, Any]) -> None:
        """Use *namespace* as the live DCC globals for later executions."""
        with self._lock:
            self._dcc_namespace = namespace

    def register_state_digest_provider(self, provider: StateDigestProvider | None) -> None:
        """Enable or disable scene-digest capture for this server instance.

        Passing ``None`` explicitly removes a provider during adapter shutdown
        or reconfiguration.  This keeps the capability instance-owned and
        avoids retaining a stale host callback after its DCC has gone away.
        """
        if provider is not None and not callable(provider):
            raise TypeError("state digest provider must be callable or None")
        with self._lock:
            self._state_digest_provider = provider

    def capture_state_digest(self) -> SceneDigestSnapshot:
        """Read one bounded digest, failing closed when capability is absent."""
        with self._lock:
            if self._state_digest_provider is None:
                raise SceneDigestError(
                    "scene_digest_provider_missing",
                    "No scene digest provider is registered for this script context",
                )
            return snapshot_from_provider(self._state_digest_provider)

    def script_namespace(self) -> dict[str, Any]:
        """Return a shallow copy of persistent variables."""
        with self._lock:
            return dict(self._script_namespace)

    def clear(self) -> None:
        """Clear variables produced by earlier script executions."""
        with self._lock:
            self._script_namespace.clear()

    def update_script_namespace(self, values: dict[str, Any]) -> None:
        """Merge variables bound by a sandboxed execution back into the namespace.

        Used by the ``dcc_execute`` persistent-namespace path (issue #2300):
        the sandbox runs in its own dict and only the variable namespace is
        copied back, so ``clear_script_namespace`` remains the single reset
        point for both execution tools. Dunder keys are dropped — they belong
        to the execution machinery, not to the script.
        """
        with self._lock:
            for key, value in values.items():
                if str(key).startswith("__"):
                    continue
                self._script_namespace[key] = value

    def execute(self, code: str, *, filename: str = "<execute_python>") -> Any:
        """Execute code and persist variables atomically in this context."""
        with self._lock:
            namespace: dict[str, Any] = {}
            namespace.update(self._dcc_namespace)
            namespace.update(self._script_namespace)
            local_namespace: dict[str, Any] = {}
            exec(compile(code, filename, "exec"), namespace, local_namespace)
            self._script_namespace.update(local_namespace)
            return local_namespace.get("result")

    def execute_with_state_digest(
        self,
        code: str,
        *,
        filename: str = "<execute_python>",
    ) -> SceneDigestExecution:
        """Capture host state immediately before and after one script."""
        with self._lock:
            if _run_with_scene_digest_transaction is None:
                raise SceneDigestError(
                    "scene_digest_custody_unavailable",
                    "Transactional in-process scene observations require the native custody boundary",
                )
            provider = self._state_digest_provider
            if provider is None:
                raise SceneDigestError(
                    "scene_digest_provider_missing",
                    "No scene digest provider is registered for this script context",
                )
            try:
                transaction = _run_with_scene_digest_transaction(
                    provider,
                    _capture_scene_digest_wire,
                    self._execute_digest_callback,
                    code,
                    filename,
                )
            finally:
                # A script may reach this context through frame inspection, but
                # provider changes made by that script cannot escape the active
                # transaction or poison the next one.
                self._state_digest_provider = provider

            return _resolve_digest_transaction(transaction)

    def _execute_digest_callback(self, code: str, filename: str) -> Any:
        """Run one script while native code retains the before-state evidence."""
        return self.execute(code, filename=filename)

    def reset_for_tests(self) -> None:
        """Clear both DCC and persistent script namespaces."""
        with self._lock:
            self._dcc_namespace = {}
            self._script_namespace.clear()
            self._state_digest_provider = None


_DEFAULT_SCRIPT_EXECUTION_CONTEXT = ScriptExecutionContext()


def get_default_script_execution_context() -> ScriptExecutionContext:
    """Return the compatibility context used when none is injected."""
    return _DEFAULT_SCRIPT_EXECUTION_CONTEXT


def reset_default_script_execution_context_for_tests() -> None:
    """Reset the compatibility script context between tests."""
    _DEFAULT_SCRIPT_EXECUTION_CONTEXT.reset_for_tests()


def _script_context(context: ScriptExecutionContext | None) -> ScriptExecutionContext:
    return context if context is not None else _DEFAULT_SCRIPT_EXECUTION_CONTEXT


def register_dcc_namespace(
    ns: dict[str, Any],
    *,
    context: ScriptExecutionContext | None = None,
) -> None:
    """Make *ns* available as the DCC globals during script execution.

    Call once at adapter startup so that scripts can call DCC commands
    without having to ``import pymel as pm`` or
    ``import maya.cmds as cmds`` in every snippet.

    Example (Maya adapter)::

        import __main__
        from dcc_mcp_core.script_execution import register_dcc_namespace
        register_dcc_namespace(
            vars(__main__), context=server.script_execution_context
        )

    """
    _script_context(context).register_dcc_namespace(ns)


def register_state_digest_provider(
    provider: StateDigestProvider | None,
    *,
    context: ScriptExecutionContext | None = None,
) -> None:
    """Register one host-owned scene digest provider for an adapter instance."""
    _script_context(context).register_state_digest_provider(provider)


def capture_state_digest(
    *,
    context: ScriptExecutionContext | None = None,
) -> SceneDigestSnapshot:
    """Capture one validated digest through the registered capability."""
    return _script_context(context).capture_state_digest()


def get_script_namespace(*, context: ScriptExecutionContext | None = None) -> dict[str, Any]:
    """Return a copy of the persistent script namespace."""
    return _script_context(context).script_namespace()


def clear_script_namespace(*, context: ScriptExecutionContext | None = None) -> None:
    """Reset the persistent script namespace (useful before a fresh workflow)."""
    _script_context(context).clear()


def update_script_namespace(
    values: dict[str, Any],
    *,
    context: ScriptExecutionContext | None = None,
) -> None:
    """Merge sandboxed variables into the persistent script namespace."""
    _script_context(context).update_script_namespace(values)


def execute_with_context(
    code: str,
    *,
    filename: str = "<execute_python>",
    context: ScriptExecutionContext | None = None,
) -> Any:
    """Execute *code* with DCC globals + persistent namespace.

    Returns the value of the ``result`` variable if the script assigns one,
    otherwise ``None``.

    The persistent namespace is **updated in-place** after execution so
    that newly-assigned variables are visible to the next call.
    """
    return _script_context(context).execute(code, filename=filename)


def execute_with_state_digest(
    code: str,
    *,
    filename: str = "<execute_python>",
    context: ScriptExecutionContext | None = None,
) -> SceneDigestExecution:
    """Execute code with fail-closed before/after scene digest evidence."""
    return _script_context(context).execute_with_state_digest(code, filename=filename)
