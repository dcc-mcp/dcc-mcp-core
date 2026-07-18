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
import sys
import traceback

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
