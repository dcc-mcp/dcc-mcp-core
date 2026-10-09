"""ui_control__prepare_foreground entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import prepare_foreground_tool
else:
    from _entrypoint import emit
    from _entrypoint import prepare_foreground_tool


def main(**kwargs):
    """Run explicit foreground preparation on the retained project route."""
    return prepare_foreground_tool(kwargs)


if __name__ == "__main__":
    emit(prepare_foreground_tool())
