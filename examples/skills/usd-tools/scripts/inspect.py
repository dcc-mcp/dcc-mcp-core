"""Inspect USD stage structure using usdcat.

Displays prim hierarchy, layer composition, and metadata for USD files.
Works with .usd, .usda, .usdc, .usdz formats.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Inspect a USD file and report its structure."""
    parser = argparse.ArgumentParser(description="Inspect USD stage structure.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--flatten", action="store_true")
    args = parser.parse_args()

    cmd = ["usdcat"]
    if args.flatten:
        cmd.append("--flatten")
    cmd.append(args.input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=60, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"usdcat failed: {result.stderr.strip()}",
                    }
                )
            )
            sys.exit(1)

        content = result.stdout
        prim_count = content.count("def ")
        layer_count = content.count("subLayers") + content.count("references") + content.count("payload")

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Inspected {args.input_file}: ~{prim_count} prims",
                    "context": {
                        "file": args.input_file,
                        "prim_count": prim_count,
                        "composition_arcs": layer_count,
                        "flattened": args.flatten,
                        "preview": content[:2000],
                    },
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "usdcat not found. Install OpenUSD: pip install usd-core",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
