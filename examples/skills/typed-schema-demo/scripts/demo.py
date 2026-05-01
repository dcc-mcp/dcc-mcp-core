"""Typed handler demo for issue #242.

The dataclass-annotated handler below is passed through
``tool_spec_from_callable`` which derives both ``inputSchema`` and
``outputSchema`` from the annotations — no hand-written JSON schema, no
``pydantic`` dependency.

Run this module standalone to print the derived schemas::

    python -m examples.skills.typed-schema-demo.scripts.demo
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import json
from typing import Literal

from dcc_mcp_core import tool_spec_from_callable
from dcc_mcp_core._tool_registration import ToolSpec


@dataclass
class ExportInput:
    """Arguments for :func:`export_scene`."""

    scene_path: str = field(
        metadata={"description": "Absolute path to the scene file to export."},
    )
    format: Literal["fbx", "abc", "usd"] = field(
        default="fbx",
        metadata={"description": "Interchange format; one of fbx / abc / usd."},
    )
    frame_range: tuple[int, int] = field(
        default=(1, 100),
        metadata={"description": "Inclusive frame range (start, end)."},
    )


@dataclass
class ExportResult:
    """Result payload for :func:`export_scene`."""

    path: str
    size_bytes: int
    took_ms: int


def export_scene(args: ExportInput) -> ExportResult:
    """Export a scene to an interchange format (demo — no real IO)."""
    # In a real adapter, this would call the DCC's export API.
    return ExportResult(path=args.scene_path, size_bytes=0, took_ms=0)


# ToolSpec ready for register_tools(server, [spec]).  Both schemas are
# derived; the adapter does nothing schema-related by hand.
spec: ToolSpec = tool_spec_from_callable(export_scene, name="typed_schema_demo__export")


if __name__ == "__main__":
    print("inputSchema:")
    print(json.dumps(spec.input_schema, indent=2))
    print()
    print("outputSchema:")
    print(json.dumps(spec.output_schema, indent=2))
