"""Create a polygon sphere in Maya.

This script demonstrates a typical DCC action that creates geometry.
In a real Maya environment, it would use `maya.cmds.polySphere`.
"""

from __future__ import annotations

import argparse
import json


def main() -> None:
    """Create a polygon sphere with configurable parameters."""
    parser = argparse.ArgumentParser(description="Create a polygon sphere.")
    parser.add_argument("--radius", type=float, default=1.0)
    parser.add_argument("--subdivisions", type=int, default=20)
    parser.add_argument("--name", default="pSphere1")
    args = parser.parse_args()

    result = {
        "success": True,
        "message": f"Created sphere '{args.name}' with radius={args.radius}, subdivisions={args.subdivisions}",
        "context": {
            "object_name": args.name,
            "radius": args.radius,
            "subdivisions": args.subdivisions,
        },
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
