"""Hello World skill script — prints a greeting message.

Parameter resolution order:
1. stdin JSON: {"name": "..."} — used by dcc-mcp-core execute_script
2. CLI positional arg: greet.py <name> — used by direct invocation / tests
3. Default: "World"
"""

from __future__ import annotations

import json
import sys


def main() -> None:
    """Entry point for the greet action."""
    name = "World"

    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
            if raw.strip():
                params = json.loads(raw)
                name = params.get("name", name)
    except Exception:
        pass

    if name == "World" and len(sys.argv) > 1:
        name = sys.argv[1]

    result = {
        "success": True,
        "message": f"Hello, {name}! (from remote MCP server)",
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
