"""Generate a thumbnail image from a video file."""

import json
import subprocess
import sys


def main():
    """Extract a single frame as a thumbnail."""
    input_file = None
    output_file = "thumbnail.jpg"
    time = "00:00:05"

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--output" and i + 1 < len(args):
            output_file = args[i + 1]
        elif arg == "--time" and i + 1 < len(args):
            t = args[i + 1]
            time = t if ":" in t else f"00:00:{int(t):02d}"

    if not input_file:
        print(json.dumps({"success": False, "message": "Missing --input"}))
        sys.exit(1)

    cmd = [
        "ffmpeg", "-y",
        "-ss", time,
        "-i", input_file,
        "-frames:v", "1",
        "-q:v", "2",
        output_file,
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"Thumbnail failed: {result.stderr[-200:]}",
            }))
            sys.exit(1)

        print(json.dumps({
            "success": True,
            "message": f"Thumbnail saved to {output_file} (at {time})",
            "context": {"input": input_file, "output": output_file, "timestamp": time},
        }))

    except FileNotFoundError:
        print(json.dumps({"success": False, "message": "ffmpeg not found."}))
        sys.exit(1)


if __name__ == "__main__":
    main()
