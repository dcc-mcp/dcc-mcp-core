"""ui_control__record_clip entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import record_clip_tool
else:
    from _entrypoint import emit
    from _entrypoint import record_clip_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return record_clip_tool(kwargs)


if __name__ == "__main__":
    emit(record_clip_tool())
