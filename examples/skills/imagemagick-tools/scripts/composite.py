"""Composite/overlay images using ImageMagick."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Overlay one image on another with configurable blend."""
    parser = argparse.ArgumentParser(description="Composite images using ImageMagick.")
    parser.add_argument("--base", required=True)
    parser.add_argument("--overlay", required=True)
    parser.add_argument("--output", default="composite_output.png")
    parser.add_argument("--gravity", default="center")
    parser.add_argument("--opacity", type=int, default=100)
    args = parser.parse_args()

    cmd = [
        "magick",
        args.base,
        "(",
        args.overlay,
        "-alpha",
        "set",
        "-channel",
        "A",
        "-evaluate",
        "multiply",
        str(args.opacity / 100.0),
        "+channel",
        ")",
        "-gravity",
        args.gravity,
        "-composite",
        args.output,
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=60, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"Composite failed: {result.stderr.strip()}",
                    }
                )
            )
            sys.exit(1)

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Composited {args.overlay} onto {args.base} -> {args.output}",
                    "context": {
                        "base": args.base,
                        "overlay": args.overlay,
                        "output": args.output,
                        "gravity": args.gravity,
                        "opacity": args.opacity,
                    },
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "ImageMagick not found.",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
