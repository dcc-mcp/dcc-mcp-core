"""Inspect USD stage structure using usdcat.

Displays prim hierarchy, layer composition, and metadata for USD files.
Works with .usd, .usda, .usdc, .usdz formats.
"""

import json
import subprocess
import sys


def main():
    """Inspect a USD file and report its structure."""
    input_file = None
    flatten = False

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--flatten":
            flatten = True

    if not input_file:
        print(json.dumps({"success": False, "message": "Missing --input <usd_file>"}))
        sys.exit(1)

    cmd = ["usdcat"]
    if flatten:
        cmd.append("--flatten")
    cmd.append(input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"usdcat failed: {result.stderr.strip()}",
            }))
            sys.exit(1)

        content = result.stdout
        prim_count = content.count("def ")
        layer_count = content.count("subLayers") + content.count("references") + content.count("payload")

        print(json.dumps({
            "success": True,
            "message": f"Inspected {input_file}: ~{prim_count} prims",
            "context": {
                "file": input_file,
                "prim_count": prim_count,
                "composition_arcs": layer_count,
                "flattened": flatten,
                "preview": content[:2000],
            },
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "usdcat not found. Install OpenUSD: pip install usd-core",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
