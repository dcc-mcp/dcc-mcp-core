"""Asset business-logic service.

The service layer is the place to put orchestration that is shared across
multiple tool entry points. It must:

- accept and return plain Python types (no MCP knowledge),
- raise typed exceptions on failure rather than returning envelopes,
- be unit-testable without spinning up an MCP server.
"""

from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass

from utils.path_utils import make_asset_id
from utils.path_utils import normalise_asset_name


class AssetError(Exception):
    """Base class for asset-service failures."""


class AssetNotFound(AssetError):
    """Raised when ``asset_id`` does not match a known asset."""


@dataclass
class Asset:
    id: str
    name: str
    kind: str
    state: str


class AssetService:
    """In-memory asset store used purely as a structural example.

    A real service would persist to disk, USD, a project DB, etc. The
    point here is the *shape* — tools call into one cohesive object, not
    into a tangle of free functions.
    """

    def __init__(self) -> None:
        self._assets: dict[str, Asset] = {}

    def create(self, name: str, kind: str = "model") -> Asset:
        clean_name = normalise_asset_name(name)
        if not clean_name:
            raise AssetError(f"asset name {name!r} is empty after normalisation")
        asset_id = make_asset_id(clean_name, kind)
        asset = Asset(id=asset_id, name=clean_name, kind=kind, state="draft")
        self._assets[asset_id] = asset
        return asset

    def publish(self, asset_id: str) -> Asset:
        asset = self._assets.get(asset_id)
        if asset is None:
            raise AssetNotFound(asset_id)
        asset.state = "published"
        return asset

    def validate(self, asset_id: str) -> dict:
        asset = self._assets.get(asset_id)
        if asset is None:
            raise AssetNotFound(asset_id)
        issues: list[str] = []
        if asset.name != normalise_asset_name(asset.name):
            issues.append("name not normalised")
        return {"asset": asdict(asset), "issues": issues, "ok": not issues}
