"""app_ui__find entry point."""

from __future__ import annotations

from pathlib import Path
import sys

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import find_tool
else:
    _SCRIPTS_DIR = Path(__file__).resolve().parent
    if str(_SCRIPTS_DIR) not in sys.path:
        sys.path.insert(0, str(_SCRIPTS_DIR))
    from _entrypoint import emit
    from _entrypoint import find_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return find_tool(kwargs)


if __name__ == "__main__":
    emit(find_tool())
