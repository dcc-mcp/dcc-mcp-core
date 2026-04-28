"""Typed result envelope for Python MCP tool handlers (#487).

Replaces the ad-hoc ``{"success": ..., "message": ..., "context": ...}``
dicts that previously appeared inline in every handler in ``recipes.py``,
``feedback.py``, ``introspect.py``, ``docs_resources.py``,
``workflow_yaml.py``, and ``dcc_server.py``.

The wire format is preserved: :meth:`ToolResult.to_dict` produces the
same JSON shape that existing clients receive today, including the
""empty fields are pruned"" convention that makes the envelope stable
across feature flags.

Typical usage::

    from dcc_mcp_core.result_envelope import ToolResult

    return ToolResult.success("Loaded skill", name="recipe.x").to_dict()

    return ToolResult.error(
        "Failed to load skill",
        error="not_found",
        prompt="Try `recipes__list` to see available skills.",
        skill_name=name,
    ).to_dict()
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from typing import Any

from dcc_mcp_core import json_dumps


@dataclass
class ToolResult:
    """Typed envelope for MCP tool return values.

    Attributes
    ----------
    success:
        ``True`` if the tool succeeded, ``False`` otherwise.
    message:
        Short human-readable summary suitable for surfacing to an AI agent.
    error:
        Stable, machine-readable error code (e.g. ``"not_found"``,
        ``"invalid_input"``). ``None`` on success.
    prompt:
        Optional next-step suggestion shown to the agent (e.g. ``"Try
        `recipes__list` to see available skills."``).
    context:
        Free-form structured data carried alongside the message. Empty
        contexts are pruned by :meth:`to_dict` to keep the wire format
        stable with the historical hand-rolled dicts.

    """

    success: bool
    message: str = ""
    error: str | None = None
    prompt: str = ""
    context: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        """Render to the JSON-compatible dict shape clients expect.

        Empty optional fields (``error=None``, ``prompt=""``,
        ``context={}``) are pruned so the wire format matches what the
        previous hand-rolled dicts produced.
        """
        out: dict[str, Any] = {"success": self.success}
        if self.message:
            out["message"] = self.message
        if self.error is not None:
            out["error"] = self.error
        if self.prompt:
            out["prompt"] = self.prompt
        if self.context:
            out["context"] = self.context
        return out

    def to_json(self) -> str:
        """Render to the JSON string form used by JSON-RPC handlers."""
        return json_dumps(self.to_dict())

    # ── Factory helpers ────────────────────────────────────────────────────

    @classmethod
    def success_(cls, message: str = "", **context: Any) -> ToolResult:
        """Build a success envelope. Keyword arguments become ``context``.

        Named with a trailing underscore to avoid shadowing
        ``unittest.TestCase.success`` and similar builtins on intermixed
        usage. Use :meth:`ok` as a shorter alias.
        """
        return cls(success=True, message=message, context=dict(context))

    ok = success_

    @classmethod
    def error_(
        cls,
        message: str,
        error: str = "error",
        prompt: str = "",
        **context: Any,
    ) -> ToolResult:
        """Build an error envelope with a stable error code and optional prompt.

        Named with a trailing underscore to avoid shadowing the dataclass
        ``error`` field.
        """
        return cls(
            success=False,
            message=message,
            error=error,
            prompt=prompt,
            context=dict(context),
        )

    fail = error_

    @classmethod
    def not_found(
        cls,
        entity_type: str,
        entity_name: str,
        **context: Any,
    ) -> ToolResult:
        """Build a ``not_found`` envelope for missing entities."""
        return cls.error_(
            message=f"{entity_type} not found: {entity_name}",
            error="not_found",
            **context,
        )

    @classmethod
    def invalid_input(cls, message: str, **context: Any) -> ToolResult:
        """Build an ``invalid_input`` envelope for caller-side validation errors."""
        return cls.error_(message=message, error="invalid_input", **context)


__all__ = ["ToolResult"]
