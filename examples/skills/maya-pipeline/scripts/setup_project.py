"""Set up a standardized Maya project directory structure."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> None:
    """Create Maya project directory structure."""
    parser = argparse.ArgumentParser(description="Set up a Maya project structure.")
    parser.add_argument("--name", default="UntitledProject")
    parser.add_argument("--root", default=".")
    args = parser.parse_args()

    project_dir = Path(args.root) / args.name
    subdirs = ["scenes", "textures", "cache", "renders", "exports"]
    created = []

    for sub in subdirs:
        d = project_dir / sub
        d.mkdir(parents=True, exist_ok=True)
        created.append(str(d))

    print(
        json.dumps(
            {
                "success": True,
                "message": f"Created project '{args.name}' with {len(subdirs)} directories",
                "context": {
                    "project_dir": str(project_dir),
                    "directories": created,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
