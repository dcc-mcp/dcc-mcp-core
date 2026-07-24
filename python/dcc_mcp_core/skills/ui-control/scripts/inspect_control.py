"""ui_control__inspect entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import inspect_tool
else:
    from _entrypoint import emit
    from _entrypoint import inspect_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return inspect_tool(kwargs)


if __name__ == "__main__":
    emit(inspect_tool())
