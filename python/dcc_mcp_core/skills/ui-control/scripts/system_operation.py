"""ui_control__system_operation entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import system_operation_tool
else:
    from _entrypoint import emit
    from _entrypoint import system_operation_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return system_operation_tool(kwargs)


if __name__ == "__main__":
    emit(system_operation_tool())
