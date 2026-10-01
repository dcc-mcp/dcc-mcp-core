"""Inspect the configured native software."""

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success
from runtime import configured_runtime


@skill_entry
def main():
    try:
        return skill_success("Native Inkscape capabilities queried", **configured_runtime().capabilities())
    except (KeyError, ValueError, RuntimeError, OSError) as exc:
        return skill_error(str(exc), "inkscape_unavailable")


if __name__ == "__main__":
    run_main(main)
