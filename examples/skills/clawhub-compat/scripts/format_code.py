"""Format code using prettier — demonstrates wrapping a Node.js CLI tool."""

import json
import subprocess
import sys


def main():
    """Format a file using prettier."""
    input_file = None
    write = False

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--write":
            write = True

    if not input_file:
        print(json.dumps({"success": False, "message": "Missing --input <file>"}))
        sys.exit(1)

    cmd = ["prettier"]
    if write:
        cmd.append("--write")
    cmd.append(input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if result.returncode != 0:
            print(json.dumps({
                "success": False,
                "message": f"prettier failed: {result.stderr.strip()}",
            }))
            sys.exit(1)

        print(json.dumps({
            "success": True,
            "message": f"Formatted {input_file}" + (" (written)" if write else " (dry-run)"),
            "context": {"file": input_file, "written": write},
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "prettier not found. Install: npm install -g prettier",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
