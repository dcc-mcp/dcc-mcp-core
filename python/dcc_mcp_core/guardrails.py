"""Adapter-extensible weak guardrails for ad-hoc DCC code execution.

This module is intentionally small and opt-in. It is **not** a security
sandbox; it only helps adapters block known-dangerous host operations while
running a single generated script.
"""

from __future__ import annotations

from contextlib import suppress
from dataclasses import dataclass
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Mapping

__all__ = [
    "DccBlockedCall",
    "DccGuardrailError",
    "DccWeakSandbox",
]


class DccGuardrailError(RuntimeError):
    """Raised when a weak guardrail blocks a known-dangerous call."""


@dataclass(frozen=True)
class DccBlockedCall:
    """A blocked host operation that an adapter may install during execution.

    ``target`` and ``attribute`` are optional so adapters can keep a declarative
    table of known-dangerous calls even when some host modules are unavailable.
    When both are provided, :class:`DccWeakSandbox` temporarily replaces the
    attribute with a callable that raises :class:`DccGuardrailError`.
    """

    name: str
    reason: str
    target: Any | None = None
    attribute: str | None = None


@dataclass(frozen=True)
class _Patch:
    target: Any
    attribute: str
    replacement: Any
    original: Any


_MISSING = object()


class DccWeakSandbox:
    """Scoped, adapter-extensible guardrails for generated DCC scripts.

    Example:

    .. code-block:: python

        with DccWeakSandbox(
            blocked_calls=[
                DccBlockedCall("sys.exit", "terminates the host process", sys, "exit"),
            ],
        ):
            exec(code, namespace)

    """

    def __init__(
        self,
        *,
        blocked_calls: Iterable[DccBlockedCall] | None = None,
        attr_overrides: Mapping[Any, Mapping[str, Any]] | None = None,
    ) -> None:
        self.blocked_calls = tuple(blocked_calls or ())
        self.attr_overrides = attr_overrides or {}
        self._patches: list[_Patch] = []
        self._active = False

    @staticmethod
    def blocked_callable(name: str, reason: str) -> Callable[..., Any]:
        """Return a callable that raises a clear guardrail error."""

        def _blocked(*_args: Any, **_kwargs: Any) -> Any:
            raise DccGuardrailError(f"Blocked DCC operation '{name}': {reason}")

        _blocked.__name__ = f"blocked_{name.replace('.', '_')}"
        return _blocked

    def __enter__(self) -> DccWeakSandbox:
        if self._active:
            raise RuntimeError("DccWeakSandbox is already active")
        self._active = True
        try:
            for blocked in self.blocked_calls:
                if blocked.target is None or blocked.attribute is None:
                    continue
                self._install(
                    blocked.target,
                    blocked.attribute,
                    self.blocked_callable(blocked.name, blocked.reason),
                )

            for target, attrs in self.attr_overrides.items():
                for attribute, replacement in attrs.items():
                    self._install(target, attribute, replacement)
        except Exception:
            self._restore()
            self._active = False
            raise
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> bool:
        self._restore()
        self._active = False
        return False

    def _install(self, target: Any, attribute: str, replacement: Any) -> None:
        original = getattr(target, attribute, _MISSING)
        setattr(target, attribute, replacement)
        self._patches.append(_Patch(target, attribute, replacement, original))

    def _restore(self) -> None:
        while self._patches:
            patch = self._patches.pop()
            current = getattr(patch.target, patch.attribute, _MISSING)
            if current is not patch.replacement:
                continue
            if patch.original is _MISSING:
                with suppress(AttributeError):
                    delattr(patch.target, patch.attribute)
            else:
                setattr(patch.target, patch.attribute, patch.original)
