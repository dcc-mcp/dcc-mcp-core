"""Format code using prettier — demonstrates wrapping a Node.js CLI tool."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Format a file using prettier."""
    parser = argparse.ArgumentParser(description="Format code using prettier.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()

    cmd = ["prettier"]
    if args.write:
        cmd.append("--write")
    cmd.append(args.input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=30, encoding="utf-8")
        if result.returncode != 0:
            print(
                json.dumps(
                    {
                        "success": False,
                        "message": f"prettier failed: {result.stderr.strip()}",
                    }
                )
            )
            sys.exit(1)

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Formatted {args.input_file}" + (" (written)" if args.write else " (dry-run)"),
                    "context": {"file": args.input_file, "written": args.write},
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "prettier not found. Install: npm install -g prettier",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
