"""Run the contracted import and behavior smoke on a real Python 3.7."""

from __future__ import annotations

import argparse
import importlib
import os
from pathlib import Path
import sys
import tempfile

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

        from dcc_mcp_core import ENV_DISABLE_ACCUMULATED_SKILLS
        from dcc_mcp_core import ENV_DISABLE_DEFAULT_SKILL_PATHS
        from dcc_mcp_core import ENV_SKILL_PATHS
        from dcc_mcp_core import ENV_TEAM_SKILL_PATHS
        from dcc_mcp_core import ENV_USER_SKILL_PATHS
        from dcc_mcp_core import create_skill_server
        from dcc_mcp_core import parse_skill_md
        from dcc_mcp_core.host import BlockingDispatcher
        from dcc_mcp_core.host import StandaloneHost
        from dcc_mcp_core.skill import skill_success

        if ENV_DISABLE_DEFAULT_SKILL_PATHS != "DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS":
            raise RuntimeError("lite_py37 exposed an invalid default-skill-path flag")
        if ENV_DISABLE_ACCUMULATED_SKILLS != "DCC_MCP_DISABLE_ACCUMULATED_SKILLS":
            raise RuntimeError("lite_py37 exposed an invalid accumulated-skill flag")
        if ENV_SKILL_PATHS != "DCC_MCP_SKILL_PATHS":
            raise RuntimeError("lite_py37 exposed an invalid skill-path variable")
        if ENV_USER_SKILL_PATHS != "DCC_MCP_USER_SKILL_PATHS":
            raise RuntimeError("lite_py37 exposed an invalid user skill-path variable")
        if ENV_TEAM_SKILL_PATHS != "DCC_MCP_TEAM_SKILL_PATHS":
            raise RuntimeError("lite_py37 exposed an invalid team skill-path variable")

        original_skill_paths = os.environ.get("DCC_MCP_SKILL_PATHS")
        with tempfile.TemporaryDirectory() as skill_dir:
            skill_path = Path(skill_dir) / "public-smoke"
            skill_path.mkdir()
            (skill_path / "SKILL.md").write_text(
                "---\n"
                "name: py37-public-smoke\n"
                "description: Public factory discovery smoke\n"
                "metadata:\n"
                "  dcc-mcp:\n"
                "    dcc: maya\n"
                '    version: "1.2.3"  # smoke version\n'
                "    tags: [py37, smoke]\n"
                "---\n",
                encoding="utf-8",
            )
            server = create_skill_server("maya", extra_paths=[skill_dir], accumulated=False)
            if getattr(server, "backend", None) != "sidecar":
                raise RuntimeError("lite_py37 public factory did not select the sidecar backend")
            names = [skill.name for skill in server.list_skills()]
            if "py37-public-smoke" not in names:
                raise RuntimeError("lite_py37 public factory did not discover the explicit skill")
            matches = [skill.name for skill in server.search_skills(query="discovery smoke")]
            if "py37-public-smoke" not in matches:
                raise RuntimeError("lite_py37 public skill search did not return the explicit skill")
            parsed = parse_skill_md(str(skill_path))
            discovered = server.get_skill("py37-public-smoke")
            if parsed is None or discovered is None:
                raise RuntimeError("lite_py37 shared skill parser returned no metadata")
            for field in ("name", "description", "dcc", "version", "tags"):
                if getattr(parsed, field) != getattr(discovered, field):
                    raise RuntimeError(f"lite_py37 skill metadata diverged for {field}")
            launch_env = server._launch_environment()
            if skill_dir not in launch_env.get("DCC_MCP_SKILL_PATHS", "").split(os.pathsep):
                raise RuntimeError("lite_py37 extra_paths did not reach the sidecar launch environment")
            if launch_env.get("DCC_MCP_DISABLE_ACCUMULATED_SKILLS") != "1":
                raise RuntimeError("lite_py37 accumulated=False did not reach the sidecar launch environment")
            try:
                server.load_skill("py37-public-smoke")
            except RuntimeError as exc:
                if "dispatch-only" not in str(exc):
                    raise
            else:
                raise RuntimeError("lite_py37 metadata-only server silently claimed skill activation")
        if os.environ.get("DCC_MCP_SKILL_PATHS") != original_skill_paths:
            raise RuntimeError("lite_py37 public factory mutated the host skill-path environment")

        try:
            create_skill_server("maya", unsupported=True)
        except TypeError:
            pass
        else:
            raise RuntimeError("lite_py37 public factory silently accepted an unknown keyword")

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
