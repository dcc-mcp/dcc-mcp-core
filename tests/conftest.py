"""Shared fixtures for dcc-mcp-core tests."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import os
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# Resolve examples/skills relative to repo root
REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")

#: Environment variable read by the Rust GatewayRunner / McpHttpConfig to
#: override the default shared registry directory (issue #793).
_DCC_MCP_REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"


@pytest.fixture(scope="session", autouse=True)
def _isolated_registry_dir(tmp_path_factory: pytest.TempPathFactory):
    """Redirect the default registry directory to a session-scoped temp dir.

    Many test modules start ``McpHttpServer`` / ``create_skill_server``
    instances without an explicit ``registry_dir``.  When they do, the Rust
    runtime falls back to ``<os-tmp>/dcc-mcp-registry`` — a **shared**
    directory that persists between test runs and accumulates stale
    ``services.json`` entries (issue #793).

    Setting ``DCC_MCP_REGISTRY_DIR`` to a fresh temporary directory ensures
    every server created during the session writes its registry entries in an
    isolated location that pytest cleans up automatically at session teardown.

    Tests that need a *distinct* registry (e.g. multi-service gateway tests)
    should create their own sub-directory via ``tmp_path_factory.mktemp(...)``
    and assign it to ``cfg.registry_dir`` explicitly — that explicit
    assignment takes precedence and is unaffected by this fixture.
    """
    registry_dir = tmp_path_factory.mktemp("session-registry", numbered=True)
    previous = os.environ.get(_DCC_MCP_REGISTRY_ENV)
    os.environ[_DCC_MCP_REGISTRY_ENV] = str(registry_dir)
    yield
    # Restore (or remove) the env-var so we don't leak state into other
    # processes that might be forked after the session ends.
    if previous is None:
        os.environ.pop(_DCC_MCP_REGISTRY_ENV, None)
    else:
        os.environ[_DCC_MCP_REGISTRY_ENV] = previous


def create_skill_dir(
    base_dir: str,
    name: str,
    frontmatter: str = "",
    *,
    dcc: str = "",
    body: str = "",
) -> str:
    """Create a temporary skill directory with a SKILL.md file.

    Auto-generated frontmatter emits the agentskills.io 1.0-compliant
    nested ``metadata.dcc-mcp.*`` form (issue #356). Top-level keys
    outside the spec allowlist are rejected by ``parse_skill_md``.

    Args:
        base_dir: Parent directory to create the skill under.
        name: Skill directory name.
        frontmatter: Raw YAML frontmatter (excluding delimiters). If empty,
            a minimal ``name: <name>`` block is generated.
        dcc: Optional DCC field placed under ``metadata.dcc-mcp.dcc``.
        body: Optional body text after the frontmatter.

    Returns:
        Path to the created skill directory.

    """
    skill_path = Path(base_dir) / name
    skill_path.mkdir(parents=True, exist_ok=True)
    if not frontmatter:
        lines = [f"name: {name}"]
        if dcc:
            lines.extend(["metadata:", "  dcc-mcp:", f"    dcc: {dcc}"])
        frontmatter = "\n".join(lines)
    content = f"---\n{frontmatter}\n---\n{body}"
    (skill_path / "SKILL.md").write_text(content, encoding="utf-8")
    return str(skill_path)


def scan_and_find(
    examples_dir: str,
    skill_name: str,
) -> dcc_mcp_core.SkillMetadata:
    """Scan examples_dir and return parsed SkillMetadata for *skill_name*.

    Raises:
        StopIteration: If the skill is not found.
        AssertionError: If parsing returns None.

    """
    scanner = dcc_mcp_core.SkillScanner()
    dirs = scanner.scan(extra_paths=[examples_dir])
    skill_dir = next(d for d in dirs if Path(d).name == skill_name)
    meta = dcc_mcp_core.parse_skill_md(skill_dir)
    assert meta is not None, f"parse_skill_md returned None for {skill_name}"
    return meta


@pytest.fixture()
def examples_dir() -> str:
    """Return the path to the examples/skills directory, skipping if absent."""
    if not Path(EXAMPLES_SKILLS_DIR).is_dir():
        pytest.skip("examples/skills directory not found")
    return EXAMPLES_SKILLS_DIR


@pytest.fixture()
def scanned_metas(examples_dir: str) -> list[dcc_mcp_core.SkillMetadata]:
    """Scan all example skills and return a list of parsed SkillMetadata objects.

    Useful for tests in TestScanAndParseRoundTrip that iterate over all skills.
    """
    scanner = dcc_mcp_core.SkillScanner()
    dirs = scanner.scan(extra_paths=[examples_dir])
    metas = []
    for d in dirs:
        meta = dcc_mcp_core.parse_skill_md(d)
        assert meta is not None, f"Failed to parse {d}"
        metas.append(meta)
    return metas
