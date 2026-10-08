"""Resolve the active ``McpHttpConfig`` implementation for the current wheel."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core._runtime.core_availability import is_core_extension_available


def resolve_mcp_http_config_class() -> type[Any]:
    """Return PyO3 ``McpHttpConfig`` when available, else the pure-Python dataclass.

    The result is never ``None``: when the compiled extension exists but does
    not export ``McpHttpConfig`` (a partially built or ABI-mismatched wheel),
    the call falls back to the pure-Python dataclass instead of raising
    ``ImportError``.
    """
    if is_core_extension_available():
        try:
            from dcc_mcp_core._core import McpHttpConfig as CoreMcpHttpConfig
        except (ImportError, AttributeError):
            pass
        else:
            if CoreMcpHttpConfig is not None:
                return CoreMcpHttpConfig
    from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig

    return PureMcpHttpConfig


def _pure_config_class() -> type[Any]:
    from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig

    return PureMcpHttpConfig


def _looks_like_mcp_http_config(value: Any) -> bool:
    """Return ``True`` when ``value`` quacks like an ``McpHttpConfig``.

    Identity checks are deliberately avoided. Two distinct classes named
    ``McpHttpConfig`` legitimately coexist: the PyO3 type exported by the
    compiled ``_core`` extension and the pure-Python dataclass used by the
    py37-lite profile. They are never the same object, so ``isinstance``
    accepts one and rejects the other depending on which wheel is installed,
    which produced the contradictory failure ``'McpHttpConfig' object is not an
    instance of 'McpHttpConfig'``. Both spellings expose the same core
    attributes, so the attributes are the contract.
    """
    if isinstance(value, _pure_config_class()):
        return True
    return all(hasattr(value, name) for name in ("port", "server_name", "endpoint_path"))


def accepted_mcp_http_config_types() -> tuple[type[Any], ...]:
    """Return the ``McpHttpConfig`` classes a caller may pass.

    Only one implementation is resolved per wheel, but an adapter can hand over
    either spelling: ``dcc_mcp_core.McpHttpConfig`` is whatever this wheel
    resolved, while the pure-Python dataclass is what py37-lite helpers and
    tests build directly. Both are listed for introspection, though validation
    itself is duck-typed so a config class from either side is accepted.
    """
    pure = _pure_config_class()
    try:
        resolved = resolve_mcp_http_config_class()
    except (AttributeError, ImportError):
        # A partially faked ``dcc_mcp_core._core`` (tests, half-installed
        # wheels) must not make config validation itself unimportable.
        return (pure,)
    if resolved is None or resolved is pure:
        return (pure,)
    return (resolved, pure)


def validate_mcp_http_config(value: Any, *, owner: str, arg: str = "config") -> None:
    """Reject a ``config`` value the server backends cannot consume.

    Both backends read typed attributes off ``McpHttpConfig`` — the embedded
    one through PyO3, the py37-lite sidecar through ``getattr`` defaults — so a
    value of any other type used to travel all the way into PyO3 and surface as
    ``argument 'config': 'dict' object is not an instance of 'McpHttpConfig'``.
    That names the expected type but never says how to build one, so callers
    could only discover the contract by trial and error.

    Args:
        value: The candidate config. ``None`` is valid and means "backend
            defaults", not "skip validation".
        owner: Human-readable call site used as the message prefix, e.g.
            ``create_adapter_server(dcc_name='blender')``.
        arg: Parameter name to quote in the message.

    Raises:
        TypeError: ``value`` is neither ``None`` nor an ``McpHttpConfig``
            instance. The message names the received type and shows the
            supported construction paths.

    """
    if value is None or _looks_like_mcp_http_config(value):
        return
    received = f"{type(value).__module__}.{type(value).__name__}"
    raise TypeError(
        f"{owner}: '{arg}' must be an McpHttpConfig instance or None, got {received}.\n"
        "Build one with the config class the installed wheel resolves:\n"
        "    from dcc_mcp_core import McpHttpConfig\n"
        "    cfg = McpHttpConfig(port=8765)\n"
        "Pass None instead to accept the backend defaults (OS-assigned port, shared gateway port,\n"
        "no job persistence) -- None is a supported value, not a skipped validation.\n"
        "Values that are not an McpHttpConfig (dict, namespace, str, int) are never coerced."
    )


McpHttpConfig = resolve_mcp_http_config_class()
