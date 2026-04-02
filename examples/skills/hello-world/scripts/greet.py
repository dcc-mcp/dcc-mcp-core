"""Hello World skill script — prints a greeting message."""

from __future__ import annotations

import json
import sys


def main() -> None:
    """Entry point for the greet action."""
    name = "World"
    if len(sys.argv) > 1:
        name = sys.argv[1]

    result = {
        "success": True,
        "message": f"Hello, {name}!",
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
