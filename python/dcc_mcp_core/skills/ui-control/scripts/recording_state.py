"""ui_control__recording_state entry point."""

from __future__ import annotations

try:
    from ._entrypoint import emit
    from ._entrypoint import recording_state_tool
except ImportError:
    from _entrypoint import emit
    from _entrypoint import recording_state_tool


def main(**kwargs):
    """Read CUA trajectory recording state."""
    return recording_state_tool(kwargs)


if __name__ == "__main__":
    emit(recording_state_tool())
