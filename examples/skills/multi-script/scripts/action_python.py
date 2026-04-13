"""Python action script example.

Reads JSON parameters from stdin (dcc-mcp-core execute_script protocol).
"""

from __future__ import annotations

import json
import sys


def main() -> None:
    """Execute the Python action."""
    try:
        raw = sys.stdin.read()
        params = json.loads(raw) if raw.strip() else {}
    except Exception:
        params = {}

    message = params.get("message", "hello")
    print(json.dumps({"success": True, "message": f"Python says: {message}"}))


if __name__ == "__main__":
    main()
