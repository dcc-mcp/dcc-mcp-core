"""Create a skinning joint at the current Maya selection (rigging group demo).

This script belongs to the inactive ``rigging`` tool group and only becomes
callable after the agent issues ``activate_tool_group(group="rigging")``.
"""

from __future__ import annotations

import argparse
import json
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description="Create a Maya skinning joint.")
    parser.add_argument("--name", default="joint1", help="Node name for the new joint")
    args = parser.parse_args()

    try:
        import maya.cmds as mc  # type: ignore[import-not-found]
    except ImportError:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": "Maya is not available. Run from a mayapy / Maya session.",
                }
            )
        )
        sys.exit(1)

    try:
        joint = mc.joint(name=args.name)
    except Exception as exc:
        print(json.dumps({"success": False, "message": f"mc.joint failed: {exc}"}))
        sys.exit(1)

    print(
        json.dumps(
            {
                "success": True,
                "message": f"Created joint '{joint}'",
                "context": {"joint": joint},
            }
        )
    )


if __name__ == "__main__":
    main()
