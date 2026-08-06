"""ui_control__recording_stop entry point."""

from __future__ import annotations

try:
    from ._entrypoint import emit
    from ._entrypoint import recording_stop_tool
except ImportError:
    from _entrypoint import emit
    from _entrypoint import recording_stop_tool


def main(**kwargs):
    """Stop CUA trajectory recording."""
    return recording_stop_tool(kwargs)


if __name__ == "__main__":
    emit(recording_stop_tool())
