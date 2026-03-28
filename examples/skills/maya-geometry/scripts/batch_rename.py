"""Batch rename objects with a prefix/suffix pattern.

Demonstrates a multi-parameter action script.
"""

import json
import sys


def main():
    """Batch rename objects."""
    prefix = ""
    suffix = ""
    objects = []

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--prefix" and i + 1 < len(args):
            prefix = args[i + 1]
        elif arg == "--suffix" and i + 1 < len(args):
            suffix = args[i + 1]
        elif arg == "--objects" and i + 1 < len(args):
            objects = args[i + 1].split(",")

    renamed = [f"{prefix}{obj}{suffix}" for obj in objects]

    result = {
        "success": True,
        "message": f"Renamed {len(renamed)} objects",
        "context": {
            "renamed": renamed,
            "count": len(renamed),
        },
    }
    print(json.dumps(result))


if __name__ == "__main__":
    main()
