"""app_ui__act entry point."""

from __future__ import annotations

from pathlib import Path
import sys

if __package__:
    from ._entrypoint import act_tool
    from ._entrypoint import emit
else:
    _SCRIPTS_DIR = Path(__file__).resolve().parent
    if str(_SCRIPTS_DIR) not in sys.path:
        sys.path.insert(0, str(_SCRIPTS_DIR))
    from _entrypoint import act_tool
    from _entrypoint import emit


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return act_tool(kwargs)


if __name__ == "__main__":
    emit(act_tool())
