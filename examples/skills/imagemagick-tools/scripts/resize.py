"""Resize images using ImageMagick."""

import json
import subprocess
import sys


def main():
    """Resize an image with configurable dimensions and fit mode."""
    input_file = None
    output_file = None
    width = 1024
    height = 1024
    fit = "contain"  # contain, cover, exact

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--output" and i + 1 < len(args):
            output_file = args[i + 1]
        elif arg == "--width" and i + 1 < len(args):
            width = int(args[i + 1])
        elif arg == "--height" and i + 1 < len(args):
            height = int(args[i + 1])
        elif arg == "--fit" and i + 1 < len(args):
            fit = args[i + 1]

    if not input_file:
        print(json.dumps({"success": False, "message": "Missing --input"}))
        sys.exit(1)

    if not output_file:
        output_file = input_file.rsplit(".", 1)[0] + f"_{width}x{height}." + input_file.rsplit(".", 1)[-1]

    # ImageMagick geometry flags
    geometry_map = {
        "contain": f"{width}x{height}",       # fit within, preserve aspect ratio
        "cover": f"{width}x{height}^",         # fill area, may crop
        "exact": f"{width}x{height}!",         # exact size, ignore aspect ratio
    }
    geometry = geometry_map.get(fit, f"{width}x{height}")

    cmd = ["magick", input_file, "-resize", geometry, output_file]

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"Resize failed: {result.stderr.strip()}",
            }))
            sys.exit(1)

        print(json.dumps({
            "success": True,
            "message": f"Resized {input_file} -> {output_file} ({width}x{height}, {fit})",
            "context": {
                "input": input_file, "output": output_file,
                "width": width, "height": height, "fit": fit,
            },
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "ImageMagick not found. Install with: brew install imagemagick",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
