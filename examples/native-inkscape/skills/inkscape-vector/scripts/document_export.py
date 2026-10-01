"""Export with Inkscape's native software actions."""

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success
from runtime import configured_runtime


@skill_entry
def main(
    source_file,
    output_file,
    format="png",
    width=None,
    height=None,
    background="#ffffff",
    background_opacity=0,
    plain_svg=False,
    text_to_path=False,
):
    try:
        result = configured_runtime().document_export(
            source_file, output_file, format, width, height, background, background_opacity, plain_svg, text_to_path
        )
        return skill_success("Native document exported", **result)
    except (KeyError, TypeError, ValueError, RuntimeError, OSError) as exc:
        return skill_error(str(exc), "inkscape_export_failed")


if __name__ == "__main__":
    run_main(main)
