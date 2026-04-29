"""End-to-end skill-execution tests against the production bindings.

Complements ``crates/dcc-mcp-skills/src/catalog/tests/test_execute_script_real.rs``
(which covers the subprocess code path) by exercising the **in-process**
executor pipeline that DCC adapters (Maya, Blender, Houdini) actually use:

1. Materialise a real ``SKILL.md`` package on disk (frontmatter + script).
2. Wire ``SkillCatalog.set_in_process_executor`` with the production helper
   ``build_inprocess_executor`` plus a real ``BaseDccCallableDispatcher``.
3. Discover and load the skill via ``SkillCatalog`` so the registry-side
   wiring is exercised end-to-end.
4. Invoke the executor with the loaded script path and verify the script
   actually processed an on-disk file (not just that ``is_loaded`` flipped).

Historical context: dcc-mcp-maya issues #137/#138 surfaced because the
script-execution chain was only ever exercised via smoke tests in CI.
These tests close that gap by asserting on real file artefacts after
execution.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path
import textwrap
from typing import Any
from typing import Callable

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor
from dcc_mcp_core._server.inprocess_executor import run_skill_script

# ── helpers ────────────────────────────────────────────────────────────────


def _write_skill(
    base_dir: Path,
    skill_name: str,
    scripts: dict[str, str],
    *,
    dcc: str = "python",
) -> Path:
    """Materialise a SKILL.md package with one or more scripts.

    The frontmatter uses the modern ``metadata.dcc-mcp.dcc`` form so the
    ``DCC_NAMES_REQUIRING_HOST_PYTHON`` ambient-python guard does not fire
    even though we never reach the subprocess path.
    """
    skill_dir = base_dir / skill_name
    (skill_dir / "scripts").mkdir(parents=True, exist_ok=True)
    front = (
        "---\n"
        f"name: {skill_name}\n"
        "description: Real execution test skill for dcc-mcp-core\n"
        "version: 1.0.0\n"
        "metadata:\n"
        f"  dcc-mcp.dcc: {dcc}\n"
        "  dcc-mcp.layer: example\n"
        "---\n"
        f"\n# {skill_name}\n"
    )
    (skill_dir / "SKILL.md").write_text(front, encoding="utf-8")
    for name, body in scripts.items():
        (skill_dir / "scripts" / name).write_text(textwrap.dedent(body).lstrip(), encoding="utf-8")
    return skill_dir


class _RecordingDispatcher:
    """Minimal ``BaseDccCallableDispatcher`` that runs callables inline.

    Mirrors the contract every real DCC dispatcher (Maya UI thread,
    Houdini ``hou.session``, Unreal game thread) honours: forward to
    ``func(*args, **kwargs)`` and return its result.
    """

    def __init__(self) -> None:
        self.calls: list[tuple[Callable[..., Any], tuple[Any, ...], dict[str, Any]]] = []

    def dispatch_callable(self, func: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
        self.calls.append((func, args, kwargs))
        return func(*args, **kwargs)


# ── tests: real Python script processes a real file ───────────────────────


class TestInProcessSkillProcessesFile:
    """The in-process executor must route to the script and observe its file output."""

    def test_executor_runs_script_that_copies_file(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "file-copy-skill",
            {
                "do_copy.py": """
                from pathlib import Path
                def main(input, output, suffix=""):
                    src = Path(input).read_text(encoding="utf-8")
                    Path(output).write_text(src + suffix, encoding="utf-8")
                    return {"success": True, "wrote": output, "bytes": len(src) + len(suffix)}
                """,
            },
        )

        # Discover + load via the production catalog so registry wiring runs.
        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        dispatcher = _RecordingDispatcher()
        catalog.set_in_process_executor(build_inprocess_executor(dispatcher))
        catalog.discover(extra_paths=[str(tmp_path)])
        catalog.load_skill("file-copy-skill")
        assert catalog.is_loaded("file-copy-skill")

        # Invoke the executor exactly the way dispatch would: with the script
        # path and a real params dict. Verify the on-disk file is correct.
        script = skill_dir / "scripts" / "do_copy.py"
        input_file = tmp_path / "in.txt"
        output_file = tmp_path / "out.txt"
        input_file.write_text("python skill payload", encoding="utf-8")

        executor = build_inprocess_executor(dispatcher)
        result = executor(str(script), {"input": str(input_file), "output": str(output_file), "suffix": "!"})

        assert result == {
            "success": True,
            "wrote": str(output_file),
            "bytes": len("python skill payload") + 1,
        }
        assert output_file.read_text(encoding="utf-8") == "python skill payload!"
        # Dispatcher must have been routed through (UI-thread contract).
        assert len(dispatcher.calls) == 1
        func, args, kwargs = dispatcher.calls[0]
        assert func is run_skill_script
        assert args[0] == str(script)
        assert kwargs == {}

    def test_executor_runs_script_that_writes_json_manifest(self, tmp_path: Path) -> None:
        """A common DCC skill pattern: scan a directory, emit a JSON manifest."""
        # Materialise a fake "input asset" tree the script will scan.
        assets = tmp_path / "assets"
        (assets / "scenes").mkdir(parents=True)
        (assets / "textures").mkdir()
        (assets / "scenes" / "shot010.ma").write_text("// dummy", encoding="utf-8")
        (assets / "scenes" / "shot020.ma").write_text("// dummy", encoding="utf-8")
        (assets / "textures" / "diffuse.png").write_bytes(b"\x89PNG\r\n")

        skill_dir = _write_skill(
            tmp_path,
            "manifest-skill",
            {
                "build_manifest.py": """
                import json
                from pathlib import Path
                def main(root, output):
                    root = Path(root)
                    files = sorted(str(p.relative_to(root)).replace("\\\\", "/")
                                   for p in root.rglob("*") if p.is_file())
                    Path(output).write_text(json.dumps({"files": files}, indent=2), encoding="utf-8")
                    return {"success": True, "count": len(files)}
                """,
            },
        )

        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        dispatcher = _RecordingDispatcher()
        catalog.set_in_process_executor(build_inprocess_executor(dispatcher))
        catalog.discover(extra_paths=[str(tmp_path)])
        catalog.load_skill("manifest-skill")

        script = skill_dir / "scripts" / "build_manifest.py"
        manifest = tmp_path / "manifest.json"
        result = build_inprocess_executor(dispatcher)(
            str(script),
            {"root": str(assets), "output": str(manifest)},
        )

        assert result == {"success": True, "count": 3}
        # Import standard library JSON only here so the test file stays linear.
        import json

        loaded = json.loads(manifest.read_text(encoding="utf-8"))
        assert loaded == {
            "files": [
                "scenes/shot010.ma",
                "scenes/shot020.ma",
                "textures/diffuse.png",
            ],
        }


# ── multi-script skills ────────────────────────────────────────────────────


class TestMultiScriptSkill:
    def test_each_script_in_a_skill_is_independently_executable(self, tmp_path: Path) -> None:
        """A skill with two scripts must let each one process its own file."""
        skill_dir = _write_skill(
            tmp_path,
            "two-tools-skill",
            {
                "uppercase.py": """
                from pathlib import Path
                def main(input, output):
                    src = Path(input).read_text(encoding="utf-8")
                    Path(output).write_text(src.upper(), encoding="utf-8")
                    return {"success": True, "tool": "uppercase"}
                """,
                "wordcount.py": """
                from pathlib import Path
                def main(input):
                    txt = Path(input).read_text(encoding="utf-8")
                    return {"success": True, "tool": "wordcount", "words": len(txt.split())}
                """,
            },
        )

        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        catalog.set_in_process_executor(build_inprocess_executor(None))
        catalog.discover(extra_paths=[str(tmp_path)])
        catalog.load_skill("two-tools-skill")

        executor = build_inprocess_executor(None)
        input_file = tmp_path / "doc.txt"
        upper_out = tmp_path / "upper.txt"
        input_file.write_text("the quick brown fox", encoding="utf-8")

        upper_result = executor(
            str(skill_dir / "scripts" / "uppercase.py"),
            {"input": str(input_file), "output": str(upper_out)},
        )
        assert upper_result == {"success": True, "tool": "uppercase"}
        assert upper_out.read_text(encoding="utf-8") == "THE QUICK BROWN FOX"

        wc_result = executor(
            str(skill_dir / "scripts" / "wordcount.py"),
            {"input": str(input_file)},
        )
        assert wc_result == {"success": True, "tool": "wordcount", "words": 4}


# ── error propagation ────────────────────────────────────────────────────


class TestSkillScriptErrorPropagation:
    def test_script_raise_propagates_to_caller(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "raises-skill",
            {
                "boom.py": """
                def main(reason):
                    raise RuntimeError(f"skill failed: {reason}")
                """,
            },
        )

        executor = build_inprocess_executor(None)
        with pytest.raises(RuntimeError, match="skill failed: invalid input"):
            executor(
                str(skill_dir / "scripts" / "boom.py"),
                {"reason": "invalid input"},
            )

    def test_dispatcher_errors_are_visible_to_caller(self, tmp_path: Path) -> None:
        """If the host dispatcher's UI thread fails (e.g. Maya viewport closed),
        the error must surface so MCP can return a proper error envelope.
        """
        skill_dir = _write_skill(
            tmp_path,
            "dispatched-skill",
            {
                "noop.py": "def main(**_): return {'success': True}\n",
            },
        )

        class _BoomDispatcher:
            def dispatch_callable(
                self,
                func: Callable[..., Any],
                *args: Any,
                **kwargs: Any,
            ) -> Any:
                raise RuntimeError("UI thread shutting down")

        executor = build_inprocess_executor(_BoomDispatcher())
        with pytest.raises(RuntimeError, match="UI thread shutting down"):
            executor(str(skill_dir / "scripts" / "noop.py"), {})

    def test_missing_main_callable_raises_attribute_error(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "no-main-skill",
            {
                "module_only.py": "value = 42\n",
            },
        )
        executor = build_inprocess_executor(None)
        with pytest.raises(AttributeError, match="`main` callable"):
            executor(str(skill_dir / "scripts" / "module_only.py"), {})


# ── catalog ↔ executor wiring round-trip ───────────────────────────────────


class TestExecutorWiringSurvivesCatalogLifecycle:
    def test_clear_then_reset_executor(self, tmp_path: Path) -> None:
        """Clearing then re-installing the executor must leave the catalog in a
        consistent state — protects against the pattern where a Maya plugin
        reloads a skill bundle without restarting the MCP server.
        """
        skill_dir = _write_skill(
            tmp_path,
            "reset-skill",
            {
                "tag.py": """
                from pathlib import Path
                def main(output, tag):
                    Path(output).write_text(tag, encoding="utf-8")
                    return {"success": True}
                """,
            },
        )

        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        catalog.set_in_process_executor(build_inprocess_executor(None))
        catalog.discover(extra_paths=[str(tmp_path)])
        catalog.load_skill("reset-skill")

        # Clear and re-install — must not raise and the catalog must still
        # report the skill as loaded.
        catalog.set_in_process_executor(None)
        catalog.set_in_process_executor(build_inprocess_executor(_RecordingDispatcher()))
        assert catalog.is_loaded("reset-skill")

        # Run the script and check the file artefact.
        out = tmp_path / "out.txt"
        executor = build_inprocess_executor(None)
        result = executor(
            str(skill_dir / "scripts" / "tag.py"),
            {"output": str(out), "tag": "v2"},
        )
        assert result == {"success": True}
        assert out.read_text(encoding="utf-8") == "v2"
