"""Create native editable vectors through the Inkscape extension."""

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success
from runtime import configured_runtime


@skill_entry
def main(output_file, plan):
    try:
        return skill_success("Native vector document created", **configured_runtime().document_build(output_file, plan))
    except (KeyError, TypeError, ValueError, RuntimeError, OSError) as exc:
        return skill_error(str(exc), "inkscape_build_failed")


if __name__ == "__main__":
    run_main(main)
