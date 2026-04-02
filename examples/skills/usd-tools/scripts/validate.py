"""Validate USD files using usdchecker compliance rules."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Run usdchecker on a USD file."""
    parser = argparse.ArgumentParser(description="Validate USD files.")
    parser.add_argument("--input", required=True, dest="input_file")
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    cmd = ["usdchecker"]
    if args.strict:
        cmd.append("--strict")
    cmd.append(args.input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, timeout=120, encoding="utf-8")

        issues = []
        for line in result.stdout.splitlines() + result.stderr.splitlines():
            line = line.strip()
            if line and ("Warning" in line or "Error" in line or "failed" in line.lower()):
                issues.append(line)

        is_valid = result.returncode == 0 and len(issues) == 0

        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"{'Valid' if is_valid else 'Issues found'}: {args.input_file} ({len(issues)} issues)",
                    "context": {
                        "file": args.input_file,
                        "valid": is_valid,
                        "issue_count": len(issues),
                        "issues": issues[:20],
                        "strict_mode": args.strict,
                    },
                }
            )
        )

    except FileNotFoundError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "usdchecker not found. Install OpenUSD: pip install usd-core",
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
