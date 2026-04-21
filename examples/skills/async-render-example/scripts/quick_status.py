"""Return fake render status — sync demo tool for issue #317."""

from __future__ import annotations

import json
import sys


def main() -> None:
    _ = json.load(sys.stdin)
    json.dump({"success": True, "status": "idle", "queued": 0}, sys.stdout)


if __name__ == "__main__":
    main()
