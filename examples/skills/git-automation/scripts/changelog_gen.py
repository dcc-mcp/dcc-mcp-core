"""Generate a changelog from git log between two refs."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys


def main() -> None:
    """Generate changelog between two git refs."""
    parser = argparse.ArgumentParser(description="Generate a changelog from git log.")
    parser.add_argument("--repo", default=".")
    parser.add_argument("--from", default=None, dest="from_ref")
    parser.add_argument("--to", default="HEAD", dest="to_ref")
    args = parser.parse_args()

    from_ref = args.from_ref
    if not from_ref:
        result = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"],
            capture_output=True,
            timeout=30,
            cwd=args.repo,
            encoding="utf-8",
        )
        from_ref = result.stdout.strip() if result.returncode == 0 else None

    if not from_ref:
        print(json.dumps({"success": False, "message": "No --from ref and no tags found."}))
        sys.exit(1)

    git_range = f"{from_ref}..{args.to_ref}"
    result = subprocess.run(
        ["git", "log", git_range, "--pretty=format:%h %s (%an)", "--no-merges"],
        capture_output=True,
        timeout=30,
        cwd=args.repo,
        encoding="utf-8",
    )

    if result.returncode != 0:
        print(json.dumps({"success": False, "message": f"git log failed: {result.stderr.strip()}"}))
        sys.exit(1)

    lines = result.stdout.strip().splitlines()
    entries = [f"- {line}" for line in lines]
    changelog = f"## Changes ({git_range})\n\n" + "\n".join(entries) + "\n"

    print(
        json.dumps(
            {
                "success": True,
                "message": f"Generated changelog: {len(lines)} entries ({git_range})",
                "context": {"from": from_ref, "to": args.to_ref, "entries": len(lines), "changelog": changelog},
            }
        )
    )


if __name__ == "__main__":
    main()
