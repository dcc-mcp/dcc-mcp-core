"""app_ui__find entry point."""

from __future__ import annotations

from _entrypoint import emit
from _entrypoint import find_tool

if __name__ == "__main__":
    emit(find_tool())
