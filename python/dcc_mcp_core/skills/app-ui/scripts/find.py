"""app_ui__find entry point for the mock backend."""

from __future__ import annotations

from _backend import emit
from _backend import find_tool

if __name__ == "__main__":
    emit(find_tool())
