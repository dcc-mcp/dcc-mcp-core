"""Shared agent-first adapter Install SOP contracts."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

INSTALL_SOP_SCHEMA_VERSION = 1

INSTALL_EXIT_OK = 0
INSTALL_EXIT_PREFLIGHT = 10
INSTALL_EXIT_ACQUIRE = 20
INSTALL_EXIT_INSTALL = 30
INSTALL_EXIT_VERIFY = 40
INSTALL_EXIT_REQUIRES_RESTART = 50

INSTALL_EXIT_CODES = {
    "ok": INSTALL_EXIT_OK,
    "preflight": INSTALL_EXIT_PREFLIGHT,
    "acquire": INSTALL_EXIT_ACQUIRE,
    "install": INSTALL_EXIT_INSTALL,
    "verify": INSTALL_EXIT_VERIFY,
    "requires_restart": INSTALL_EXIT_REQUIRES_RESTART,
}

_SCHEMA_PATH = Path(__file__).resolve().parent.parent / "schemas" / "adapter-install-sop-v1.schema.json"


def load_install_sop_schema() -> dict[str, Any]:
    """Return a fresh copy of the packaged Install SOP JSON Schema."""
    return json.loads(_SCHEMA_PATH.read_text(encoding="utf-8"))


__all__ = [
    "INSTALL_EXIT_ACQUIRE",
    "INSTALL_EXIT_CODES",
    "INSTALL_EXIT_INSTALL",
    "INSTALL_EXIT_OK",
    "INSTALL_EXIT_PREFLIGHT",
    "INSTALL_EXIT_REQUIRES_RESTART",
    "INSTALL_EXIT_VERIFY",
    "INSTALL_SOP_SCHEMA_VERSION",
    "load_install_sop_schema",
]
