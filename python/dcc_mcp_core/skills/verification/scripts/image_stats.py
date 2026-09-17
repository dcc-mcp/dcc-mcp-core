"""verification__image_stats entry point."""

from __future__ import annotations

from typing import Any
from typing import Dict

from _common import existing_file
from _common import read_image_rgb
from _common import read_params
from _common import run_tool
from _common import success

from dcc_mcp_core.verification import classify_image_stats
from dcc_mcp_core.verification import compute_image_stats


def image_stats(
    input_path: Any,
    channels: Any = 3,
    bins: Any = 16,
    timeout_secs: Any = 30,
) -> Dict[str, Any]:
    """Compute in-band image statistics and bad-frame flags for a capture."""
    path = existing_file("input_path", input_path)
    width, height, rgb = read_image_rgb(path, int(timeout_secs))
    stats = compute_image_stats(width, height, rgb, channels=3, bins=int(bins))
    flags = classify_image_stats(stats)
    return success(
        "Image statistics computed.",
        input_path=str(path),
        stats=stats.to_dict(),
        flags=flags.to_dict(),
    )


def main(**params: Any) -> Dict[str, Any]:
    """Run the image_stats tool."""
    return run_tool(image_stats, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    from _common import emit

    emit(main(**read_params()))
