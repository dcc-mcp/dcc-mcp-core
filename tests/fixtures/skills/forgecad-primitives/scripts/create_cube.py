"""ForgeCAD-style skill script — create a CAD cube.

Parameter resolution order:
1. stdin JSON: {"edge": ..., "marker": "..."} — used by dcc-mcp-core execute_script
2. CLI positional arg: create_cube.py <edge> — used by direct invocation
3. Defaults: edge=1.0, marker=""
"""

from __future__ import annotations

import contextlib
import json
import sys


def main() -> None:
    """Entry point for the create_cube action."""
    edge = 1.0
    marker = ""

    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
            if raw.strip():
                params = json.loads(raw)
                edge = float(params.get("edge", edge))
                marker = str(params.get("marker", marker))
    except Exception:
        pass

    if edge == 1.0 and len(sys.argv) > 1:
        with contextlib.suppress(ValueError):
            edge = float(sys.argv[1])

    result = {
        "success": True,
        "shape": "cube",
        "edge": edge,
        "marker": marker,
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
