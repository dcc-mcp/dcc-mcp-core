"""Reopen a document using native software geometry queries."""

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success
from runtime import configured_runtime


@skill_entry
def main(source_file):
    try:
        return skill_success(
            "Native document reopened and queried", **configured_runtime().document_inspect(source_file)
        )
    except (KeyError, ValueError, RuntimeError, OSError) as exc:
        return skill_error(str(exc), "inkscape_inspect_failed")


if __name__ == "__main__":
    run_main(main)
