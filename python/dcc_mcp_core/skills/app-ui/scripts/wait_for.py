"""app_ui__wait_for entry point."""

from __future__ import annotations

from _entrypoint import emit
from _entrypoint import wait_for_tool

if __name__ == "__main__":
    emit(wait_for_tool())
