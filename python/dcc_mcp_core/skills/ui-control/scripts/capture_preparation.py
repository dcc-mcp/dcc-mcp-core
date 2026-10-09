"""ui_control__capture_preparation entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import capture_preparation_tool
    from ._entrypoint import emit
else:
    from _entrypoint import capture_preparation_tool
    from _entrypoint import emit


def main(**kwargs):
    """Read or change only the explicitly granted passive preparation lifecycle."""
    return capture_preparation_tool(kwargs)


if __name__ == "__main__":
    emit(capture_preparation_tool())
