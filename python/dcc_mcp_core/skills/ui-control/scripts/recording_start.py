"""ui_control__recording_start entry point."""

from __future__ import annotations

try:
    from ._entrypoint import emit
    from ._entrypoint import recording_start_tool
except ImportError:
    from _entrypoint import emit
    from _entrypoint import recording_start_tool


def main(**kwargs):
    """Start CUA trajectory recording."""
    return recording_start_tool(kwargs)


if __name__ == "__main__":
    emit(recording_start_tool())
