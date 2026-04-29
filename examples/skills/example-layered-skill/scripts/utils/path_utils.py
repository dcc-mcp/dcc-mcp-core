"""Pure path helpers — no I/O, no DCC calls, no global state.

Anything in ``utils/`` must be:
- side-effect free,
- importable without bringing in heavy dependencies,
- trivially unit-testable.
"""

from __future__ import annotations

import re

_INVALID = re.compile(r"[^a-zA-Z0-9_\-]+")


def normalise_asset_name(raw: str) -> str:
    """Return a filesystem-safe asset name.

    >>> normalise_asset_name("Hero  Mesh!")
    'hero_mesh'
    """
    cleaned = _INVALID.sub("_", raw.strip().lower())
    return cleaned.strip("_")


def make_asset_id(name: str, kind: str) -> str:
    """Build a deterministic asset identifier from name + kind."""
    return f"{kind}/{normalise_asset_name(name)}"
