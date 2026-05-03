"""ForgeCAD-style skill script — create a CAD cylinder.

Parameter resolution order:
1. stdin JSON: {"radius": ..., "height": ..., "marker": "..."} — dcc-mcp-core execute_script
2. Defaults: radius=1.0, height=2.0, marker=""
"""

from __future__ import annotations

import json
import sys


def main() -> None:
    """Entry point for the create_cylinder action."""
    radius = 1.0
    height = 2.0
    marker = ""

    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
            if raw.strip():
                params = json.loads(raw)
                radius = float(params.get("radius", radius))
                height = float(params.get("height", height))
                marker = str(params.get("marker", marker))
    except Exception:
        pass

    result = {
        "success": True,
        "shape": "cylinder",
        "radius": radius,
        "height": height,
        "marker": marker,
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
