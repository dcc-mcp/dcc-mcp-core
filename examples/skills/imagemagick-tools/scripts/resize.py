"""Resize images using ImageMagick."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys


def main() -> None:
    """Resize an image with configurable dimensions and fit mode."""
    parser = argparse.ArgumentParser(description="Resize images using ImageMagick.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--output", default=None, dest="output_file")
    parser.add_argument("--width", type=int, default=1024)
    parser.add_argument("--height", type=int, default=1024)
    parser.add_argument("--fit", default="contain", choices=["contain", "cover", "exact"])
    args = parser.parse_args()

    output_file = args.output_file
    if not output_file:
        p = Path(args.input_file)
        output_file = str(p.with_name(f"{p.stem}_{args.width}x{args.height}{p.suffix}"))

    geometry_map = {
        "contain": f"{args.width}x{args.height}",
        "cover": f"{args.width}x{args.height}^",
        "exact": f"{args.width}x{args.height}!",
    }
    geometry = geometry_map[args.fit]

    cmd = ["magick", args.input_file, "-resize", geometry, output_file]

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=60, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"Resize failed: {result.stderr.strip()}",
                    }
                )
            )
            sys.exit(1)

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Resized {args.input_file} -> {output_file} ({args.width}x{args.height}, {args.fit})",
                    "context": {
                        "input": args.input_file,
                        "output": output_file,
                        "width": args.width,
                        "height": args.height,
                        "fit": args.fit,
                    },
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "ImageMagick not found. Install with: brew install imagemagick",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
