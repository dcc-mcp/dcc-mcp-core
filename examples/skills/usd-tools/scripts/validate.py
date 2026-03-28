"""Validate USD files using usdchecker compliance rules."""

import json
import subprocess
import sys


def main():
    """Run usdchecker on a USD file."""
    input_file = None
    strict = False

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--input" and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == "--strict":
            strict = True

    if not input_file:
        print(json.dumps({"success": False, "message": "Missing --input <usd_file>"}))
        sys.exit(1)

    cmd = ["usdchecker"]
    if strict:
        cmd.append("--strict")
    cmd.append(input_file)

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)

        # usdchecker returns 0 for valid, non-zero for issues
        issues = []
        for line in result.stdout.splitlines() + result.stderr.splitlines():
            line = line.strip()
            if line and ("Warning" in line or "Error" in line or "failed" in line.lower()):
                issues.append(line)

        is_valid = result.returncode == 0 and len(issues) == 0

        print(json.dumps({
            "success": True,
            "message": f"{'Valid' if is_valid else 'Issues found'}: {input_file} ({len(issues)} issues)",
            "context": {
                "file": input_file,
                "valid": is_valid,
                "issue_count": len(issues),
                "issues": issues[:20],
                "strict_mode": strict,
            },
        }))

    except FileNotFoundError:
        print(json.dumps({
            "success": False,
            "message": "usdchecker not found. Install OpenUSD: pip install usd-core",
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
