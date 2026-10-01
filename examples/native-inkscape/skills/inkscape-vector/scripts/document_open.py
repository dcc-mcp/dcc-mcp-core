"""Open an isolated native GUI process for visual acceptance."""

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success
from runtime import configured_runtime


@skill_entry
def main(source_file):
    try:
        return skill_success(
            "Native GUI opened for visual acceptance", **configured_runtime().document_open(source_file)
        )
    except (KeyError, ValueError, RuntimeError, OSError) as exc:
        return skill_error(str(exc), "inkscape_open_failed")


if __name__ == "__main__":
    run_main(main)
