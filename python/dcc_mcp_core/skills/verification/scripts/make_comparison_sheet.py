"""verification__make_comparison_sheet entry point."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Dict
from typing import List

from _common import VerificationToolError
from _common import existing_file
from _common import read_params
from _common import run_ffmpeg
from _common import run_tool
from _common import success


def _build_command(inputs: List[Path], output: Path, layout: str, overwrite: bool) -> List[str]:
    argv = ["ffmpeg"]
    for path in inputs:
        argv += ["-i", str(path)]
    direction = "h" if layout == "horizontal" else "v"
    argv += ["-filter_complex", f"{direction}stack=inputs={len(inputs)}"]
    argv += ["-y" if overwrite else "-n", str(output)]
    return argv


def make_comparison_sheet(
    reference_path: Any,
    render_paths: Any,
    output_path: Any,
    layout: Any = "horizontal",
    overwrite: Any = False,
    timeout_secs: Any = 30,
) -> Dict[str, Any]:
    """Composite reference + renders side-by-side into one sheet (no scores)."""
    reference = existing_file("reference_path", reference_path)
    if not render_paths:
        raise VerificationToolError(
            "render_paths must be a non-empty list.",
            "missing_input",
            context={"field": "render_paths"},
        )
    renders = [existing_file("render_paths", value) for value in render_paths]
    if not output_path:
        raise VerificationToolError("output_path is required.", "missing_input", context={"field": "output_path"})
    output = Path(str(output_path))
    if output.exists() and not overwrite:
        raise VerificationToolError(
            "output_path already exists; set overwrite to replace it.",
            "output_exists",
            context={"path": str(output)},
        )
    if not output.parent.is_dir():
        raise VerificationToolError(
            "output_path parent directory does not exist.",
            "output_parent_missing",
            context={"path": str(output.parent)},
        )

    inputs = [reference, *renders]
    command = _build_command(inputs, output, str(layout), bool(overwrite))
    run_ffmpeg(command, int(timeout_secs))
    if not output.is_file() or output.stat().st_size == 0:
        raise VerificationToolError(
            "comparison sheet was not produced.",
            "output_missing",
            context={"path": str(output)},
        )
    return success(
        "Comparison sheet composited.",
        reference_path=str(reference),
        render_paths=[str(path) for path in renders],
        output_path=str(output),
        layout=str(layout),
        command=command,
    )


def main(**params: Any) -> Dict[str, Any]:
    """Run the make_comparison_sheet tool."""
    return run_tool(make_comparison_sheet, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    from _common import emit

    emit(main(**read_params()))
