"""Dependency-light wire envelope for Python MCP tool handlers.

``dcc_mcp_core.ToolResult`` is the Rust-backed runtime model. This module owns
the distinct JSON-compatible wire builder, :class:`ToolResultEnvelope`, used by
pure-Python skill scripts and handler code. Keeping the names distinct avoids
the historical collision where two unrelated classes were both called
``ToolResult`` (issue #2183).

Typical usage::

    from dcc_mcp_core.result_envelope import ToolResultEnvelope

    return ToolResultEnvelope.ok("Loaded skill", name="recipe.x").to_dict()

    return ToolResultEnvelope.fail(
        "Failed to load skill",
        error="not_found",
        prompt="Try `recipes__list` to see available skills.",
        skill_name=name,
    ).to_dict()

The deprecated ``result_envelope.ToolResult`` name remains available through a
module-level compatibility shim for one migration window. New code must use
``ToolResultEnvelope`` or the top-level Rust-backed ``ToolResult`` explicitly.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import json
from typing import TYPE_CHECKING
from typing import Any
from typing import Mapping
import warnings

_KNOWN_KEYS = frozenset({"success", "message", "error", "prompt", "context", "postcondition", "_meta"})


@dataclass
class ToolResultEnvelope:
    """JSON-compatible envelope for MCP tool return values.

    ``error`` is always a string when present. Structured diagnostics belong
    under namespaced ``_meta`` entries such as ``dcc.error`` or
    ``dcc.raw_trace``; they must never replace the string error field.

    ``postcondition`` carries structured readback evidence for a successful
    mutation. Its optional ``verified`` member is always a boolean when
    present; absence means that the producer did not report verification.

    Empty optional fields are pruned by default. Callers that expose a legacy
    fixed-key mapping may pass ``prune_empty=False`` to :meth:`to_dict` while
    retaining the same schema and field types.
    """

    success: bool
    message: str = ""
    error: str | None = None
    prompt: str | None = None
    context: dict[str, Any] = field(default_factory=dict)
    _meta: dict[str, Any] = field(default_factory=dict)
    postcondition: dict[str, Any] | None = None

    def __post_init__(self) -> None:
        """Enforce the wire schema on every construction path."""
        if not isinstance(self.success, bool):
            raise TypeError("'success' field must be a bool")
        if not isinstance(self.message, str):
            raise TypeError("'message' field must be a string")
        if self.error is not None and not isinstance(self.error, str):
            raise TypeError("'error' field must be a string or None")
        if self.prompt is not None and not isinstance(self.prompt, str):
            raise TypeError("'prompt' field must be a string or None")
        if not isinstance(self.context, Mapping):
            raise TypeError("'context' field must be a mapping")
        if self.postcondition is not None and not isinstance(self.postcondition, Mapping):
            raise TypeError("'postcondition' field must be a mapping or None")
        if self.postcondition is not None and "verified" in self.postcondition:
            verified = self.postcondition["verified"]
            if not isinstance(verified, bool):
                raise TypeError("'postcondition.verified' field must be a bool")
        if not isinstance(self._meta, Mapping):
            raise TypeError("'_meta' field must be a mapping")
        self.context = dict(self.context)
        if self.postcondition is not None:
            self.postcondition = dict(self.postcondition)
        self._meta = dict(self._meta)

    def to_dict(self, *, prune_empty: bool = True) -> dict[str, Any]:
        """Render the canonical JSON-compatible mapping.

        Args:
            prune_empty: Omit empty optional fields when ``True``. Skill helper
                compatibility paths use ``False`` so released scripts can keep
                indexing ``prompt``, ``error``, and ``context`` directly.

        """
        out: dict[str, Any] = {"success": self.success}
        if self.message or not prune_empty:
            out["message"] = self.message
        if self.error is not None or not prune_empty:
            out["error"] = self.error
        if self.prompt not in (None, "") or not prune_empty:
            out["prompt"] = self.prompt
        if self.context or not prune_empty:
            out["context"] = self.context
        if self.postcondition is not None:
            out["postcondition"] = self.postcondition
        if self._meta:
            out["_meta"] = self._meta
        return out

    def to_json(self, *, prune_empty: bool = True) -> str:
        """Render the envelope as dependency-free UTF-8 JSON."""
        return json.dumps(
            self.to_dict(prune_empty=prune_empty),
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )

    @classmethod
    def from_dict(
        cls,
        payload: Mapping[str, Any],
        *,
        strict: bool = True,
    ) -> ToolResultEnvelope:
        """Validate and normalize a result mapping.

        Unknown top-level keys are rejected in strict mode. ``strict=False``
        preserves the historical runtime behavior by folding them into
        ``context``; :func:`dcc_mcp_core.skill._serialize_result` uses that
        compatibility mode for released skill scripts.
        """
        if not isinstance(payload, Mapping):
            raise TypeError("tool result envelope must be a mapping")

        success = payload.get("success")
        if not isinstance(success, bool):
            raise TypeError("'success' field must be a bool")

        message = payload.get("message", "")
        if not isinstance(message, str):
            raise TypeError("'message' field must be a string")

        error = payload.get("error")
        if error is not None and not isinstance(error, str):
            raise TypeError("'error' field must be a string or None")

        prompt = payload.get("prompt")
        if prompt is not None and not isinstance(prompt, str):
            raise TypeError("'prompt' field must be a string or None")

        raw_context = payload.get("context")
        if raw_context is None:
            context: dict[str, Any] = {}
        elif isinstance(raw_context, Mapping):
            context = dict(raw_context)
        else:
            raise TypeError("'context' field must be a mapping or None")

        raw_meta = payload.get("_meta")
        if raw_meta is None:
            meta: dict[str, Any] = {}
        elif isinstance(raw_meta, Mapping):
            meta = dict(raw_meta)
        else:
            raise TypeError("'_meta' field must be a mapping or None")

        raw_postcondition = payload.get("postcondition")
        if raw_postcondition is None:
            postcondition = None
        elif isinstance(raw_postcondition, Mapping):
            postcondition = dict(raw_postcondition)
        else:
            raise TypeError("'postcondition' field must be a mapping or None")

        extras = {key: value for key, value in payload.items() if key not in _KNOWN_KEYS}
        if extras and strict:
            names = ", ".join(sorted(str(key) for key in extras))
            raise ValueError(f"unknown tool result envelope fields: {names}")
        context.update(extras)

        return cls(
            success=success,
            message=message,
            error=error,
            prompt=prompt,
            context=context,
            postcondition=postcondition,
            _meta=meta,
        )

    from_mapping = from_dict

    @classmethod
    def success_(
        cls,
        message: str = "",
        *,
        prompt: str | None = None,
        postcondition: Mapping[str, Any] | None = None,
        verified: bool | None = None,
        _meta: Mapping[str, Any] | None = None,
        **context: Any,
    ) -> ToolResultEnvelope:
        """Build a success envelope with optional post-condition evidence.

        ``verified`` is merged into ``postcondition["verified"]``. Conflicting
        evidence is rejected instead of silently rewriting the readback claim.
        Keyword arguments not reserved by this signature become ``context``.
        """
        normalized_postcondition = dict(postcondition) if postcondition is not None else None
        if verified is not None:
            if normalized_postcondition is None:
                normalized_postcondition = {}
            elif "verified" in normalized_postcondition:
                existing_verified = normalized_postcondition["verified"]
                if not isinstance(existing_verified, bool):
                    raise TypeError("'postcondition.verified' field must be a bool")
                if existing_verified != verified:
                    raise ValueError("'verified' conflicts with postcondition verification evidence")
            normalized_postcondition["verified"] = verified
        return cls(
            success=True,
            message=message,
            prompt=prompt,
            context=dict(context),
            postcondition=normalized_postcondition,
            _meta=dict(_meta or {}),
        )

    ok = success_

    @classmethod
    def error_(
        cls,
        message: str,
        error: str = "error",
        prompt: str | None = None,
        *,
        _meta: Mapping[str, Any] | None = None,
        **context: Any,
    ) -> ToolResultEnvelope:
        """Build an error envelope with a string code and optional metadata."""
        return cls(
            success=False,
            message=message,
            error=error,
            prompt=prompt,
            context=dict(context),
            _meta=dict(_meta or {}),
        )

    fail = error_

    @classmethod
    def not_found(
        cls,
        entity_type: str,
        entity_name: str,
        **context: Any,
    ) -> ToolResultEnvelope:
        """Build a ``not_found`` envelope for a missing entity."""
        return cls.error_(
            message=f"{entity_type} not found: {entity_name}",
            error="not_found",
            **context,
        )

    @classmethod
    def invalid_input(cls, message: str, **context: Any) -> ToolResultEnvelope:
        """Build an ``invalid_input`` envelope for caller-side validation."""
        return cls.error_(message=message, error="invalid_input", **context)


@dataclass
class _LegacyToolResultEnvelope(ToolResultEnvelope):
    """Behavior-compatible implementation of the deprecated class name."""

    prompt: str | None = ""

    @classmethod
    def success_(cls, message: str = "", **context: Any) -> _LegacyToolResultEnvelope:
        return cls(success=True, message=message, context=dict(context))

    ok = success_

    @classmethod
    def error_(
        cls,
        message: str,
        error: str = "error",
        prompt: str = "",
        **context: Any,
    ) -> _LegacyToolResultEnvelope:
        return cls(
            success=False,
            message=message,
            error=error,
            prompt=prompt,
            context=dict(context),
        )

    fail = error_


if TYPE_CHECKING:
    ToolResult = _LegacyToolResultEnvelope


def __getattr__(name: str) -> Any:
    """Resolve the legacy pure-Python class name with a deprecation warning."""
    if name == "ToolResult":
        warnings.warn(
            "dcc_mcp_core.result_envelope.ToolResult is deprecated; use "
            "ToolResultEnvelope for wire dictionaries or dcc_mcp_core.ToolResult "
            "for the Rust-backed runtime model.",
            DeprecationWarning,
            stacklevel=2,
        )
        return _LegacyToolResultEnvelope
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def __dir__() -> list[str]:
    return sorted([*globals(), "ToolResult"])


__all__ = ["ToolResult", "ToolResultEnvelope"]
