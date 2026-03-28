"""Composite/overlay images using ImageMagick."""

import json
import subprocess
import sys


def main():
    """Overlay one image on another with configurable blend."""
    base = None
    overlay = None
    output = None
    gravity = "center"
    opacity = 100

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--base" and i + 1 < len(args):
            base = args[i + 1]
        elif arg == "--overlay" and i + 1 < len(args):
            overlay = args[i + 1]
        elif arg == "--output" and i + 1 < len(args):
            output = args[i + 1]
        elif arg == "--gravity" and i + 1 < len(args):
            gravity = args[i + 1]
        elif arg == "--opacity" and i + 1 < len(args):
            opacity = int(args[i + 1])

    if not base or not overlay:
        print(json.dumps({"success": False, "message": "Missing --base or --overlay"}))
        sys.exit(1)

    if not output:
        output = "composite_output.png"

    cmd = [
        "magick", base,
        "(", overlay, "-alpha", "set", "-channel", "A",
        "-evaluate", "multiply", str(opacity / 100.0), "+channel", ")",
        "-gravity", gravity,
        "-composite", output,
    ]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"Composite failed: {result.stderr.strip()}",
            }))
            sys.exit(1)

        print(json.dumps({
            "success": True,
            "message": f"Composited {overlay} onto {base} -> {output}",
            "context": {
                "base": base, "overlay": overlay, "output": output,
                "gravity": gravity, "opacity": opacity,
            },
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "ImageMagick not found.",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
