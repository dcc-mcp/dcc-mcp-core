"""Pretend to render a frame range (issue #317 async-execution demo)."""

from __future__ import annotations

import json
import sys
import time


def main() -> None:
    params = json.load(sys.stdin)
    start = int(params.get("start", 1))
    end = int(params.get("end", 1))
    # In a real skill this would dispatch to the DCC renderer. We sleep a
    # tiny amount so the demo feels plausible without blocking test runs.
    time.sleep(0.05)
    result = {
        "success": True,
        "message": f"rendered frames {start}-{end}",
        "frames": end - start + 1,
    }
    json.dump(result, sys.stdout)


if __name__ == "__main__":
    main()
