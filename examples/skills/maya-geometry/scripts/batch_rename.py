"""Batch rename objects with a prefix/suffix pattern.

Demonstrates a multi-parameter action script.
"""

from __future__ import annotations

import argparse
import json


def main() -> None:
    """Batch rename objects."""
    parser = argparse.ArgumentParser(description="Batch rename objects.")
    parser.add_argument("--prefix", default="")
    parser.add_argument("--suffix", default="")
    parser.add_argument("--objects", default="", help="Comma-separated list of objects")
    args = parser.parse_args()

    objects = args.objects.split(",") if args.objects else []
    renamed = [f"{args.prefix}{obj}{args.suffix}" for obj in objects]

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
