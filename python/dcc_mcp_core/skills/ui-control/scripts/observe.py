"""ui_control__observe entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import observe_tool
else:
    from _entrypoint import emit
    from _entrypoint import observe_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return observe_tool(kwargs)


if __name__ == "__main__":
    emit(observe_tool())
