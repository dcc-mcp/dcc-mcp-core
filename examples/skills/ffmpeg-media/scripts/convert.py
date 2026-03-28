"""Convert media files between formats using ffmpeg."""

import json
import subprocess
import sys


def main():
    """Convert a media file to another format."""
    input_file = None
    output_file = None
    codec = None

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--output" and i + 1 < len(args):
            output_file = args[i + 1]
        elif arg == "--codec" and i + 1 < len(args):
            codec = args[i + 1]

    if not input_file or not output_file:
        print(json.dumps({"success": False, "message": "Missing --input or --output"}))
        sys.exit(1)

    cmd = ["ffmpeg", "-y", "-i", input_file]
    if codec:
        cmd.extend(["-c:v", codec])
    cmd.append(output_file)

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"Conversion failed: {result.stderr[-200:]}",
            }))
            sys.exit(1)

        print(json.dumps({
            "success": True,
            "message": f"Converted {input_file} -> {output_file}",
            "context": {"input": input_file, "output": output_file, "codec": codec},
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "ffmpeg not found. Install FFmpeg first.",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
