"""verification__validate_scene_vs_spec entry point."""

from __future__ import annotations

from typing import Any
from typing import Dict

from _common import existing_file
from _common import load_json_object
from _common import read_params
from _common import success

from dcc_mcp_core.verification import validate_scene_vs_spec as _validate


def _resolve(key: str, inline: Any, path_value: Any) -> Any:
    if inline is not None:
        return inline
    if path_value:
        return load_json_object(existing_file(key, path_value))
    return None


def validate_scene_vs_spec(
    scene: Any = None,
    spec: Any = None,
    scene_path: Any = None,
    spec_path: Any = None,
) -> Dict[str, Any]:
    """Validate a scene export against a spec and return {passed, failures[]}."""
    resolved_scene = _resolve("scene", scene, scene_path)
    resolved_spec = _resolve("spec", spec, spec_path)
    if not isinstance(resolved_scene, dict):
        raise ValueError("scene is required and must be a JSON object")
    if not isinstance(resolved_spec, dict):
        raise ValueError("spec is required and must be a JSON object")
    result = _validate(resolved_scene, resolved_spec)
    payload = result.to_dict()
    message = "Scene matches spec." if result.passed else f"Scene failed {len(result.failures)} check(s)."
    return success(message, result=payload)


def main(**params: Any) -> Dict[str, Any]:
    """Run the validate_scene_vs_spec tool."""
    try:
        return validate_scene_vs_spec(**params)
    except (ValueError, TypeError) as exc:
        return {
            "success": False,
            "message": str(exc),
            "prompt": "Provide a valid scene and spec object.",
            "error": "invalid_input",
            "context": {},
        }


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    from _common import emit

    emit(main(**read_params()))
