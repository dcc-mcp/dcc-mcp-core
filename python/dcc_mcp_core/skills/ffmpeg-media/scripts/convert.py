"""Convert media files between formats using ffmpeg."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Convert a media file to another format."""
    parser = argparse.ArgumentParser(description="Convert media files using ffmpeg.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--output", required=True, dest="output_file")
    parser.add_argument("--codec", default=None)
    args = parser.parse_args()

    cmd = ["ffmpeg", "-y", "-i", args.input_file]
    if args.codec:
        cmd.extend(["-c:v", args.codec])
    cmd.append(args.output_file)

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=300, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"Conversion failed: {result.stderr[-200:]}",
                    }
                )
            )
            sys.exit(1)

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Converted {args.input_file} -> {args.output_file}",
                    "context": {"input": args.input_file, "output": args.output_file, "codec": args.codec},
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "ffmpeg not found. Install FFmpeg first.",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
