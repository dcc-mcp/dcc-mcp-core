"""ui_control__act entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import act_tool
    from ._entrypoint import emit
else:
    from _entrypoint import act_tool
    from _entrypoint import emit


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return act_tool(kwargs)


if __name__ == "__main__":
    emit(act_tool())
