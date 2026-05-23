"""app_ui__act entry point."""

from __future__ import annotations

from _entrypoint import act_tool
from _entrypoint import emit

if __name__ == "__main__":
    emit(act_tool())
