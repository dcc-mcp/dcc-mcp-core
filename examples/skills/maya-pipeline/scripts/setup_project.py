"""Set up a standardized Maya project directory structure."""

import json
import os
import sys


def main():
    """Create Maya project directory structure."""
    name = "UntitledProject"
    root = "."

    args = sys.argv[1:]
    for i, arg in enumerate(args):
        if arg == "--name" and i + 1 < len(args):
            name = args[i + 1]
        elif arg == "--root" and i + 1 < len(args):
            root = args[i + 1]

    project_dir = os.path.join(root, name)
    subdirs = ["scenes", "textures", "cache", "renders", "exports"]
    created = []

    for sub in subdirs:
        d = os.path.join(project_dir, sub)
        os.makedirs(d, exist_ok=True)
        created.append(d)

    print(json.dumps({
        "success": True,
        "message": f"Created project '{name}' with {len(subdirs)} directories",
        "context": {
            "project_dir": project_dir,
            "directories": created,
        },
    }))


if __name__ == "__main__":
    main()
