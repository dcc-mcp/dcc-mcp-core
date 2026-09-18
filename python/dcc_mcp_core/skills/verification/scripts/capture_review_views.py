"""verification__capture_review_views entry point."""

from __future__ import annotations

from typing import Any
from typing import Dict
from typing import List

from _common import read_params
from _common import success

_DEFAULT_VIEWS = ["front", "side", "top", "three_quarter"]
_DEFAULT_RESOLUTION = [1920, 1080]
_VIEWPORT_TARGETS = ("active_viewport", "viewport", "scene_viewer")


def _plan(views: List[str], resolution: List[int]) -> List[Dict[str, Any]]:
    return [{"view": view, "target": "active_viewport", "resolution": resolution} for view in views]


def _validate_captures(views: List[str], resolution: List[int], captures: Any) -> List[Dict[str, Any]]:
    failures: List[Dict[str, Any]] = []
    by_view: Dict[str, Any] = {}
    for capture in captures or []:
        if not isinstance(capture, dict):
            continue
        by_view.setdefault(str(capture.get("view")), capture)

    for view in views:
        capture = by_view.get(view)
        if capture is None:
            failures.append({"view": view, "check": "missing", "message": "no capture for required view"})
            continue
        width = capture.get("width")
        height = capture.get("height")
        if width != resolution[0] or height != resolution[1]:
            failures.append(
                {
                    "view": view,
                    "check": "resolution",
                    "message": f"capture resolution {width}x{height} != {resolution[0]}x{resolution[1]}",
                }
            )
        target = capture.get("target")
        if target is not None and str(target) not in _VIEWPORT_TARGETS:
            failures.append(
                {
                    "view": view,
                    "check": "target",
                    "message": f"capture target must be a viewport, got {target!r}",
                }
            )
        if capture.get("hidden") or capture.get("blocked"):
            failures.append(
                {"view": view, "check": "hidden", "message": "window is hidden or blocked; capture is invalid"}
            )
        if capture.get("blank") or capture.get("is_blank"):
            failures.append({"view": view, "check": "blank", "message": "capture is blank/white/near-black"})
    return failures


def capture_review_views(
    resolution: Any = None,
    views: Any = None,
    captures: Any = None,
) -> Dict[str, Any]:
    """Emit the deterministic review-view plan and validate captures against it."""
    resolved_resolution = list(resolution) if resolution else list(_DEFAULT_RESOLUTION)
    resolved_views = list(views) if views else list(_DEFAULT_VIEWS)
    plan = _plan(resolved_views, resolved_resolution)

    if captures is None:
        return success("Capture review view plan emitted.", plan=plan)

    failures = _validate_captures(resolved_views, resolved_resolution, captures)
    passed = not failures
    message = "All review views captured." if passed else f"{len(failures)} review view(s) failed."
    return success(message, plan=plan, passed=passed, failures=failures)


def main(**params: Any) -> Dict[str, Any]:
    """Run the capture_review_views tool."""
    return capture_review_views(**params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    from _common import emit

    emit(main(**read_params()))
