"""Run the contracted import and behavior smoke on a real Python 3.7."""

from __future__ import annotations

import argparse
import importlib
from pathlib import Path
import sys

try:
    from .python_support_contract import load_contract
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import load_contract


def _verify_profile(profile: str) -> None:
    contract = load_contract()
    expected = tuple(int(part) for part in contract["build"][profile]["python"].split("."))
    if sys.version_info[:2] != expected:
        actual = sys.version_info[:2]
        raise RuntimeError(f"{profile} smoke requires Python {expected[0]}.{expected[1]}, got {actual[0]}.{actual[1]}")

    for module_name in contract["runtime_smoke"][profile]:
        importlib.import_module(module_name)

    if profile == "native_py37":
        from dcc_mcp_core import ToolRegistry

        registry = ToolRegistry()
        if registry.count_actions() != 0:
            raise RuntimeError("new ToolRegistry must be empty")
    else:
        try:
            importlib.import_module("dcc_mcp_core._core")
        except ImportError:
            pass
        else:
            raise RuntimeError("lite_py37 must not contain dcc_mcp_core._core")

        from dcc_mcp_core.host import BlockingDispatcher
        from dcc_mcp_core.host import StandaloneHost
        from dcc_mcp_core.skill import skill_success

        if not skill_success("py37 smoke").get("success"):
            raise RuntimeError("skill_success fallback returned an invalid result")
        dispatcher = BlockingDispatcher()
        with StandaloneHost(dispatcher):
            if dispatcher.post(lambda: 37).wait(timeout=2.0) != 37:
                raise RuntimeError("pure-Python host dispatcher smoke failed")


def main(argv: list[str] | None = None) -> int:
    """Run one Python 3.7 compatibility profile."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=("native_py37", "lite_py37"), required=True)
    args = parser.parse_args(argv)
    try:
        _verify_profile(args.profile)
    except Exception as exc:
        sys.stderr.write(f"python37-runtime-smoke: {exc}\n")
        return 1
    sys.stdout.write(f"python37-runtime-smoke: {args.profile} OK\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
