"""app_ui__wait_for entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import wait_for_tool
else:
    from _entrypoint import emit
    from _entrypoint import wait_for_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return wait_for_tool(kwargs)


if __name__ == "__main__":
    emit(wait_for_tool())
