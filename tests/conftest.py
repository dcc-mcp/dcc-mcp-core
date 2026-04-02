"""Shared fixtures for dcc-mcp-core tests."""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# Resolve examples/skills relative to repo root
REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")


def create_skill_dir(
    base_dir: str,
    name: str,
    frontmatter: str = "",
    *,
    dcc: str = "",
    body: str = "",
) -> str:
    """Create a temporary skill directory with a SKILL.md file.

    Args:
        base_dir: Parent directory to create the skill under.
        name: Skill directory name.
        frontmatter: Raw YAML frontmatter (excluding delimiters). If empty,
            a minimal ``name: <name>`` block is generated.
        dcc: Optional DCC field to add to auto-generated frontmatter.
        body: Optional body text after the frontmatter.

    Returns:
        Path to the created skill directory.

    """
    skill_path = Path(base_dir) / name
    skill_path.mkdir(parents=True, exist_ok=True)
    if not frontmatter:
        lines = [f"name: {name}"]
        if dcc:
            lines.append(f"dcc: {dcc}")
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
