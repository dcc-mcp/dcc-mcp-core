"""Create a polygon sphere in Maya.

This script demonstrates a typical DCC action that creates geometry.
In a real Maya environment, it would use `maya.cmds.polySphere`.
"""

import json
import sys


def main():
    """Create a polygon sphere with configurable parameters."""
    radius = 1.0
    subdivisions = 20
    name = "pSphere1"

    # Parse arguments (simplified for example)
    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--radius" and i + 1 < len(args):
            radius = float(args[i + 1])
        elif arg == "--subdivisions" and i + 1 < len(args):
            subdivisions = int(args[i + 1])
        elif arg == "--name" and i + 1 < len(args):
            name = args[i + 1]

    result = {
        "success": True,
        "message": f"Created sphere '{name}' with radius={radius}, subdivisions={subdivisions}",
        "context": {
            "object_name": name,
            "radius": radius,
            "subdivisions": subdivisions,
        },
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
