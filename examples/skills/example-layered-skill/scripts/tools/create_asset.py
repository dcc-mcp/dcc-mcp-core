"""Tool entry point — create_asset.

Thin adapter:
1. Read JSON params from stdin (the dcc-mcp-core convention).
2. Delegate to ``AssetService.create``.
3. Print the success / error envelope to stdout.

This file is intentionally short. All non-trivial logic lives in
``scripts/services/asset_service.py``.
"""

from __future__ import annotations

import json
from pathlib import Path
import sys
import traceback

# Make sibling `services/` and `utils/` directories importable. The script
# lives at <skill>/scripts/tools/create_asset.py; siblings are one level up.
_SCRIPTS_DIR = Path(__file__).resolve().parent.parent
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

from services.asset_service import AssetError
from services.asset_service import AssetService


def main() -> dict:
    raw = sys.stdin.read() or "{}"
    try:
        params = json.loads(raw)
    except json.JSONDecodeError as exc:
        return {"success": False, "message": f"invalid JSON params: {exc}"}

    name = params.get("name")
    if not name:
        return {"success": False, "message": "`name` is required"}

    try:
        asset = AssetService().create(name=name, kind=params.get("kind", "model"))
    except AssetError as exc:
        return {"success": False, "message": str(exc)}
    except Exception as exc:  # pragma: no cover — defensive net
        return {
            "success": False,
            "message": f"create_asset failed: {exc}",
            "traceback": traceback.format_exc(),
        }

    return {
        "success": True,
        "message": f"Created asset {asset.id}",
        "context": {"asset_id": asset.id, "kind": asset.kind, "state": asset.state},
    }


if __name__ == "__main__":
    print(json.dumps(main()))
