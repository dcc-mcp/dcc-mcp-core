"""ui_control__expand entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import expand_tool
else:
    from _entrypoint import emit
    from _entrypoint import expand_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return expand_tool(kwargs)


if __name__ == "__main__":
    emit(expand_tool())
