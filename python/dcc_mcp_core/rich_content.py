"""MCP Apps rich content support for dcc-mcp-core (issue #409).

MCP Apps is the first official MCP protocol extension.  A tool can return
an interactive interface — chart, form, dashboard, image, table — rendered
inline in the chat interface.  Servers that return rich content see
meaningfully higher adoption than those returning text alone.

MCP Apps reference: https://modelcontextprotocol.io/extensions/apps/overview

This module provides:

1. :class:`RichContent` — union type for chart / form / dashboard / image /
   table payloads.
2. Skill script helpers — :func:`skill_success_with_chart`,
   :func:`skill_success_with_table`, :func:`skill_success_with_image` — for
   use inside skill scripts.
3. A :func:`attach_rich_content` helper that attaches rich content to an
   existing ``ToolResult``-like dict.

Note:
----
The Rust-level ``McpHttpServer`` support for embedding ``rich`` content in
``tools/call`` responses (MCP Apps extension format) is planned as a follow-up
Rust PR (issue #409).  Until that lands:

- Rich content is stored in ``result.context["__rich__"]`` as a JSON-
  serialisable dict.  Clients that support MCP Apps can render it; others
  ignore it gracefully.
- Backward compatibility is guaranteed: existing plain-text clients see the
  normal ``message`` and ``context`` fields and do not break.

DCC Opportunities
-----------------
| Tool | Rich Return | Value |
|------|-------------|-------|
| ``render_frames`` | Thumbnail gallery + stats table | Visual verification without leaving chat |
| ``get_scene_hierarchy`` | Interactive tree view | Browse 10,000-node scene |
| ``diagnostics__screenshot`` | Inline screenshot | More useful than a file path |
| ``analyze_keyframes`` | Animation curve chart | Visual timing debugging |
| ``get_render_stats`` | Bar chart per layer | Faster than raw JSON array |
| ``list_materials`` | Material swatch grid | Visual selection |

Usage
-----
::

    from dcc_mcp_core.rich_content import skill_success_with_chart, skill_success_with_table

    def get_render_stats(**kwargs):
        stats = [{"layer": "beauty", "time_secs": 12.3}, ...]
        return skill_success_with_chart(
            message="Render complete",
            chart_spec={
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "data": {"values": stats},
                "mark": "bar",
                "encoding": {
                    "x": {"field": "layer", "type": "nominal"},
                    "y": {"field": "time_secs", "type": "quantitative"},
                },
            },
        )

"""

from __future__ import annotations

import base64
import dataclasses
import enum
from pathlib import Path
from typing import Any

__all__ = [
    "RichContent",
    "RichContentKind",
    "attach_rich_content",
    "skill_success_with_chart",
    "skill_success_with_image",
    "skill_success_with_table",
]


class RichContentKind(str, enum.Enum):
    """Discriminator for :class:`RichContent` payloads."""

    CHART = "chart"
    FORM = "form"
    DASHBOARD = "dashboard"
    IMAGE = "image"
    TABLE = "table"


@dataclasses.dataclass
class RichContent:
    """Rich inline content attached to a tool result.

    The ``kind`` field determines how MCP Apps clients render the content.

    Args:
        kind: :class:`RichContentKind` — determines the renderer.
        payload: Kind-specific data (see constructors below).

    Constructors
    ------------
    Use the class methods instead of the raw constructor:

    - :meth:`chart` — Vega-Lite / Chart.js spec
    - :meth:`form` — JSON Schema rendered as an interactive form
    - :meth:`image` — PNG/JPEG bytes displayed inline
    - :meth:`table` — headers + rows grid

    """

    kind: RichContentKind
    payload: dict[str, Any]

    @classmethod
    def chart(cls, spec: dict[str, Any]) -> RichContent:
        """Vega-Lite or Chart.js specification.

        Args:
            spec: Vega-Lite v5 schema dict.

        Example::

            RichContent.chart({
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "data": {"values": [{"x": 1, "y": 2}]},
                "mark": "line",
                "encoding": {"x": {"field": "x"}, "y": {"field": "y"}},
            })

        """
        return cls(kind=RichContentKind.CHART, payload={"spec": spec})

    @classmethod
    def form(cls, schema: dict[str, Any], *, title: str | None = None) -> RichContent:
        """Interactive form rendered from a JSON Schema.

        Args:
            schema: JSON Schema describing the form fields.
            title: Optional form title.

        """
        payload: dict[str, Any] = {"schema": schema}
        if title:
            payload["title"] = title
        return cls(kind=RichContentKind.FORM, payload=payload)

    @classmethod
    def image(
        cls,
        data: bytes,
        mime: str = "image/png",
        *,
        alt: str | None = None,
    ) -> RichContent:
        """Inline image (PNG, JPEG, WebP…).

        Args:
            data: Raw image bytes.
            mime: MIME type.  Default ``"image/png"``.
            alt: Alt text for accessibility.

        """
        encoded = base64.b64encode(data).decode()
        payload: dict[str, Any] = {"data": encoded, "mime": mime}
        if alt:
            payload["alt"] = alt
        return cls(kind=RichContentKind.IMAGE, payload=payload)

    @classmethod
    def image_from_file(
        cls,
        path: str | Path,
        mime: str | None = None,
        *,
        alt: str | None = None,
    ) -> RichContent:
        """Inline image loaded from a file.

        Args:
            path: Path to image file.
            mime: MIME type.  Auto-detected from extension when ``None``.
            alt: Alt text.

        """
        p = Path(path)
        if mime is None:
            suffix = p.suffix.lower()
            mime = {
                ".png": "image/png",
                ".jpg": "image/jpeg",
                ".jpeg": "image/jpeg",
                ".webp": "image/webp",
                ".gif": "image/gif",
            }.get(suffix, "application/octet-stream")
        data = p.read_bytes()
        return cls.image(data, mime, alt=alt or p.name)

    @classmethod
    def table(
        cls,
        headers: list[str],
        rows: list[list[Any]],
        *,
        title: str | None = None,
    ) -> RichContent:
        """Grid table with headers and rows.

        Args:
            headers: Column header labels.
            rows: List of row lists.  Each inner list must have the same
                length as ``headers``.
            title: Optional table caption.

        """
        payload: dict[str, Any] = {"headers": headers, "rows": rows}
        if title:
            payload["title"] = title
        return cls(kind=RichContentKind.TABLE, payload=payload)

    @classmethod
    def dashboard(cls, components: list[RichContent]) -> RichContent:
        """Composite layout containing multiple rich components.

        Args:
            components: Ordered list of child :class:`RichContent` items.

        """
        return cls(
            kind=RichContentKind.DASHBOARD,
            payload={"components": [c.to_dict() for c in components]},
        )

    def to_dict(self) -> dict[str, Any]:
        """Serialise to a JSON-safe dict."""
        return {"kind": self.kind.value, **self.payload}


# ---------------------------------------------------------------------------
# Skill script helpers
# ---------------------------------------------------------------------------


def attach_rich_content(result: dict[str, Any], content: RichContent) -> dict[str, Any]:
    """Attach :class:`RichContent` to an existing skill result dict.

    Stores the rich payload under ``result["context"]["__rich__"]``.
    MCP Apps clients will render it; plain clients ignore it.

    Args:
        result: Existing ``skill_success()`` / ``skill_error()`` return dict.
        content: :class:`RichContent` to attach.

    Returns:
        The same dict with ``context.__rich__`` populated.

    Example::

        result = skill_success("Render complete", total_frames=250)
        chart = RichContent.chart({...})
        return attach_rich_content(result, chart)

    """
    ctx = result.setdefault("context", {})
    ctx["__rich__"] = content.to_dict()
    return result


def skill_success_with_chart(
    message: str,
    chart_spec: dict[str, Any],
    **kwargs: Any,
) -> dict[str, Any]:
    """Return a skill success dict with an inline Vega-Lite chart.

    Args:
        message: Human-readable success message.
        chart_spec: Vega-Lite v5 specification dict.
        **kwargs: Additional context key-value pairs (forwarded to
            ``skill_success``).

    Returns:
        Dict with ``success=True``, ``message``, and rich chart attached.

    Example::

        return skill_success_with_chart(
            "Render complete",
            chart_spec={
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "data": {"values": render_stats},
                "mark": "bar",
                "encoding": {"x": {"field": "layer"}, "y": {"field": "time_secs"}},
            },
            total_frames=250,
        )

    """
    result: dict[str, Any] = {"success": True, "message": message, "context": dict(kwargs)}
    return attach_rich_content(result, RichContent.chart(chart_spec))


def skill_success_with_table(
    message: str,
    headers: list[str],
    rows: list[list[Any]],
    *,
    title: str | None = None,
    **kwargs: Any,
) -> dict[str, Any]:
    """Return a skill success dict with an inline table.

    Args:
        message: Human-readable success message.
        headers: Column header labels.
        rows: Row data.
        title: Optional table caption.
        **kwargs: Additional context key-value pairs.

    Returns:
        Dict with ``success=True``, ``message``, and rich table attached.

    Example::

        return skill_success_with_table(
            "Scene objects",
            headers=["Name", "Type", "Vertices"],
            rows=[["pCube1", "mesh", 8], ["nurbsSphere1", "nurbs", 0]],
        )

    """
    result: dict[str, Any] = {"success": True, "message": message, "context": dict(kwargs)}
    return attach_rich_content(result, RichContent.table(headers, rows, title=title))


def skill_success_with_image(
    message: str,
    image_data: bytes | None = None,
    image_path: str | Path | None = None,
    mime: str = "image/png",
    *,
    alt: str | None = None,
    **kwargs: Any,
) -> dict[str, Any]:
    """Return a skill success dict with an inline image.

    Provide either ``image_data`` (raw bytes) or ``image_path`` (file path).

    Args:
        message: Human-readable success message.
        image_data: Raw image bytes.
        image_path: Path to an image file (used when ``image_data`` is ``None``).
        mime: MIME type for ``image_data``.  Default ``"image/png"``.
        alt: Alt text.
        **kwargs: Additional context key-value pairs.

    Returns:
        Dict with ``success=True``, ``message``, and rich image attached.

    Example::

        screenshot_bytes = capture_viewport()
        return skill_success_with_image(
            "Viewport captured",
            image_data=screenshot_bytes,
            alt="Maya viewport",
        )

    """
    if image_data is not None:
        content = RichContent.image(image_data, mime, alt=alt)
    elif image_path is not None:
        content = RichContent.image_from_file(image_path, alt=alt)
    else:
        raise ValueError("Either image_data or image_path must be provided")

    result: dict[str, Any] = {"success": True, "message": message, "context": dict(kwargs)}
    return attach_rich_content(result, content)
