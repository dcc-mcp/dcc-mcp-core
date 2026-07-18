"""Tool entry point — publish_asset.

See ``create_asset.py`` for the layering rationale.
"""

from __future__ import annotations

import json
import sys

from services.asset_service import AssetError
from services.asset_service import AssetNotFound
from services.asset_service import AssetService


def main() -> dict:
    raw = sys.stdin.read() or "{}"
    try:
        params = json.loads(raw)
    except json.JSONDecodeError as exc:
        return {"success": False, "message": f"invalid JSON params: {exc}"}

    asset_id = params.get("asset_id")
    if not asset_id:
        return {"success": False, "message": "`asset_id` is required"}

    try:
        asset = AssetService().publish(asset_id)
    except AssetNotFound:
        return {"success": False, "message": f"asset_id {asset_id!r} not found"}
    except AssetError as exc:
        return {"success": False, "message": str(exc)}

    return {
        "success": True,
        "message": f"Published {asset.id}",
        "context": {"asset_id": asset.id, "state": asset.state},
    }


if __name__ == "__main__":
    print(json.dumps(main()))
