"""Generate a thumbnail image from a video file."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Extract a single frame as a thumbnail."""
    parser = argparse.ArgumentParser(description="Generate a video thumbnail.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--output", default="thumbnail.jpg", dest="output_file")
    parser.add_argument("--time", default="00:00:05", dest="timestamp")
    args = parser.parse_args()

    timestamp = args.timestamp
    if ":" not in timestamp:
        timestamp = f"00:00:{int(timestamp):02d}"

    cmd = [
        "ffmpeg",
        "-y",
        "-ss",
        timestamp,
        "-i",
        args.input_file,
        "-frames:v",
        "1",
        "-q:v",
        "2",
        args.output_file,
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=30, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"Thumbnail failed: {result.stderr[-200:]}",
                    }
                )
            )
            sys.exit(1)

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Thumbnail saved to {args.output_file} (at {timestamp})",
                    "context": {"input": args.input_file, "output": args.output_file, "timestamp": timestamp},
                }
            )
        )

    except FileNotFoundError:
        print(json.dumps({"success": False, "message": "ffmpeg not found."}))
        sys.exit(1)


if __name__ == "__main__":
    main()
