"""Python action script example."""

from __future__ import annotations

import json


def main() -> None:
    """Execute the Python action."""
    print(json.dumps({"success": True, "message": "Executed Python action"}))


if __name__ == "__main__":
    main()
