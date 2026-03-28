"""Generate a changelog from git log between two refs."""

import json
import subprocess
import sys


def main():
    """Generate changelog between two git refs."""
    repo = "."
    from_ref = None
    to_ref = "HEAD"

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--repo" and i + 1 < len(args):
            repo = args[i + 1]
        elif arg == "--from" and i + 1 < len(args):
            from_ref = args[i + 1]
        elif arg == "--to" and i + 1 < len(args):
            to_ref = args[i + 1]

    if not from_ref:
        # Default: from latest tag
        result = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0"],
            capture_output=True, text=True, cwd=repo,
        )
        from_ref = result.stdout.strip() if result.returncode == 0 else None

    if not from_ref:
        print(json.dumps({"success": False, "message": "No --from ref and no tags found."}))
        sys.exit(1)

    git_range = f"{from_ref}..{to_ref}"
    result = subprocess.run(
        ["git", "log", git_range, "--pretty=format:%h %s (%an)", "--no-merges"],
        capture_output=True, text=True, timeout=30, cwd=repo,
    )

    if result.returncode != 0:
        print(json.dumps({"success": False, "message": f"git log failed: {result.stderr.strip()}"}))
        sys.exit(1)

    lines = result.stdout.strip().splitlines()
    changelog = f"## Changes ({git_range})\n\n"
    for line in lines:
        changelog += f"- {line}\n"

    print(json.dumps({
        "success": True,
        "message": f"Generated changelog: {len(lines)} entries ({git_range})",
        "context": {"from": from_ref, "to": to_ref, "entries": len(lines), "changelog": changelog},
    }))


if __name__ == "__main__":
    main()
