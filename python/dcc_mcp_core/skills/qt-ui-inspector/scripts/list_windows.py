"""CLI wrapper that invokes ``qt_list_windows`` with stdin JSON parameters."""

import json
import sys

from dcc_mcp_core.skills.qt_ui_inspector import qt_list_windows


def main() -> None:
    """Read JSON params from stdin, call the tool, and write the JSON result to stdout."""
    try:
        params = json.loads(sys.stdin.read()) if not sys.stdin.isatty() else {}
    except ValueError:
        params = {}
    result = qt_list_windows(**params)
    sys.stdout.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    main()
