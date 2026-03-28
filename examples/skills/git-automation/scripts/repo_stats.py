"""Analyze repository statistics using git."""

import json
import subprocess
import sys


def run_git(args, cwd=None):
    """Run a git command and return stdout."""
    result = subprocess.run(
        ["git", *args],
        capture_output=True, text=True, timeout=30, cwd=cwd,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def main():
    """Gather repository statistics."""
    repo = "."
    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--repo" and i + 1 < len(args):
            repo = args[i + 1]

    try:
        # Total commits
        commit_count = run_git(["rev-list", "--count", "HEAD"], cwd=repo)

        # Contributors
        contributors_raw = run_git(["shortlog", "-sn", "--no-merges", "HEAD"], cwd=repo)
        contributors = []
        for line in contributors_raw.splitlines():
            parts = line.strip().split("\t", 1)
            if len(parts) == 2:
                contributors.append({"commits": int(parts[0].strip()), "name": parts[1].strip()})

        # Tracked files
        files_raw = run_git(["ls-files"], cwd=repo)
        file_count = len(files_raw.splitlines()) if files_raw else 0

        # Current branch
        branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=repo)

        # Latest tag
        latest_tag = run_git(["describe", "--tags", "--abbrev=0"], cwd=repo) or None

        stats = {
            "branch": branch,
            "total_commits": int(commit_count) if commit_count else 0,
            "tracked_files": file_count,
            "contributors": len(contributors),
            "top_contributors": contributors[:5],
            "latest_tag": latest_tag,
        }

        print(json.dumps({
            "success": True,
            "message": f"Repository: {stats['total_commits']} commits, "
                       f"{stats['tracked_files']} files, "
                       f"{stats['contributors']} contributors",
            "context": stats,
        }))

    except FileNotFoundError:
        print(json.dumps({"success": False, "message": "git not found."}))
        sys.exit(1)


if __name__ == "__main__":
    main()
