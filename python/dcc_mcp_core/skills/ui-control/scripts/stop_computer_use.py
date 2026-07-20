"""ui_control__stop_computer_use entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import stop_computer_use_tool
else:
    from _entrypoint import emit
    from _entrypoint import stop_computer_use_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return stop_computer_use_tool(kwargs)


if __name__ == "__main__":
    emit(stop_computer_use_tool())
