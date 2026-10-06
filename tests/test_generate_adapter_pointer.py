"""Coverage pointer generator: catalog-driven rendering, insertion and reporting."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

import pytest
from scripts import generate_adapter_pointer as generator

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "dcc-mcp-catalog.yml"


@pytest.fixture(scope="module")
def catalog():
    return generator.load_catalog(CATALOG)


@pytest.fixture(scope="module")
def adapters(catalog):
    return generator.adapter_entries(catalog)


def _entry(adapters, name):
    return next(e for e in adapters if e["name"] == name)


def _flat(text: str) -> str:
    """Collapse generated line wrapping so prose assertions stay readable."""
    return " ".join(text.split())


def _write_catalog(tmp_path: Path, entries: list[dict]) -> Path:
    path = tmp_path / "dcc-mcp-catalog.yml"
    lines = ['version: "1"', "entries:"]
    for entry in entries:
        lines.append(f'  - name: "{entry["name"]}"')
        lines.append(f'    description: "{entry.get("description", "An adapter")}"')
        tags = ", ".join(f'"{t}"' for t in entry.get("tags", ["adapter"]))
        lines.append(f"    tags: [{tags}]")
        lines.append(f'    url: "{entry.get("url", "https://github.com/dcc-mcp/" + entry["name"])}"')
        if entry.get("version"):
            lines.append(f'    version: "{entry["version"]}"')
        install = entry.get("install")
        if install:
            lines.append("    install:")
            lines.append("      type: pip")
            lines.append(f'      pip_package: "{install["pip_package"]}"')
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


# --- catalog reading -------------------------------------------------------


def test_catalog_is_the_single_source_of_host_count(catalog, adapters):
    """The host count the narrative quotes must come from the catalog, not a literal."""
    assert len(adapters) == 47


def test_host_count_matches_the_documented_reproducible_command(catalog):
    """The issue's acceptance criterion pins the count to one reproducible command."""
    import re

    pattern = re.compile(r'^    tags: \[.*"adapter".*\]$')
    count = sum(1 for line in CATALOG.read_text(encoding="utf-8").splitlines() if pattern.match(line))
    assert count == 47


def test_load_catalog_rejects_a_file_without_entries(tmp_path):
    path = tmp_path / "empty.yml"
    path.write_text("version: '1'\n", encoding="utf-8")
    with pytest.raises(generator.CoverageError, match="no `entries` list"):
        generator.load_catalog(path)


# --- rendering -------------------------------------------------------------


def test_pointer_names_the_adapter_and_carries_the_catalog_description(adapters):
    entry = _entry(adapters, "dcc-mcp-houdini")
    block = generator.render_pointer(entry, host_count=len(adapters))
    assert "**dcc-mcp-houdini**" in block
    assert "Houdini tools exposed through MCP" in _flat(block)


def test_pointer_quotes_the_host_count_derived_from_the_catalog(adapters):
    """The count is generated, so a catalog change cannot leave a stale number behind."""
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    assert f"one of **{len(adapters)} host adapters**" in block


def test_host_count_follows_its_argument_not_the_current_catalog(adapters):
    """A hard-coded count would be indistinguishable from the derived value today.

    Asserting only the current count cannot tell a derived value from a
    literal. Passing a different count separates them; adding a host is
    exactly when this matters. The probe values must stay clear of the real
    catalog count, otherwise the second assertion below cannot fire.
    """
    entry = _entry(adapters, "dcc-mcp-maya")
    probe_counts = tuple(c for c in (7, 140, 141) if c != len(adapters))
    for count in probe_counts:
        block = generator.render_pointer(entry, host_count=count)
        assert f"one of **{count} host adapters**" in _flat(block)
        assert f"one of **{len(adapters)} host adapters**" not in _flat(block)


def test_pointer_states_the_shared_contract(adapters):
    """The shared layer is the protocol and the core runtime contract.

    Deliberately not claiming a shared tool surface: tool sets are host
    specific by design, so a Kdenlive timeline tool and a Wwise SoundBank
    tool are not interchangeable.
    """
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    assert "same MCP protocol" in _flat(block)
    assert "same core runtime contract" in _flat(block)


def test_pointer_makes_no_cross_host_tool_claim(adapters):
    """An agent cannot drive another host through this one's tool calls.

    This claim would be copied into 38 public READMEs and is not supported by
    the adapter contract, which has no tool-surface rule at all.
    """
    for entry in adapters:
        block = generator.render_pointer(entry, host_count=len(adapters))
        assert "the same calls" not in _flat(block)
        assert "same tool contract" not in _flat(block)


def test_pointer_links_to_the_shared_front_door(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    assert generator.ECOSYSTEM_URL in block
    assert generator.CORE_README_URL in block
    assert generator.SHOWCASE_URL in block


def test_generator_path_in_the_block_is_repo_qualified(adapters):
    """The block is emitted into adapter READMEs, where `scripts/` is absent.

    A bare `scripts/generate_adapter_pointer.py` is the only clue a maintainer
    gets about how to refresh the block, so it has to name the repository that
    owns the script. Unqualified, it points at a path none of the 38
    repositories has.
    """
    for entry in adapters:
        block = generator.render_pointer(entry, host_count=len(adapters))
        assert f"scripts/generate_adapter_pointer.py in {generator.SOURCE_REPO}" in block
        # The unqualified form ends the sentence right after the filename; the
        # qualified one is followed by " in <repo>".
        assert "by scripts/generate_adapter_pointer.py." not in block


def test_pointer_carries_no_ranking_or_vacancy_claim(adapters):
    """Every adapter's generated text must clear the PIP-3711 banned copy list."""
    for entry in adapters:
        block = generator.render_pointer(entry, host_count=len(adapters))
        for term in generator.BANNED_COPY_TERMS:
            assert term not in block, f"{entry['name']} generated a banned term: {term}"


def test_pointer_never_breaks_an_inline_code_span(adapters):
    """A line break inside `dcc-mcp-catalog.yml` would corrupt the rendered link."""
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    assert "[`dcc-mcp-catalog.yml`]" in block


def test_prose_lines_stay_within_the_readme_wrap_width(adapters):
    """Generated prose is wrapped, so the block matches surrounding README style."""
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    # URLs cannot be wrapped, so only the surrounding prose is width-bounded.
    prose = [
        line for line in block.splitlines() if line and not line.startswith(("<", "-", "#")) and "](http" not in line
    ]
    assert prose
    assert max(len(line) for line in prose) <= generator._PROSE_WIDTH


def test_render_refuses_english_banned_copy(adapters):
    """The generated prose is English, so the English forms are checked too.

    Only the Chinese terms were listed at first, which left a catalog
    description reading "the leading DCC bridge" free to reach 38 READMEs.
    """
    entry = dict(_entry(adapters, "dcc-mcp-maya"))
    for phrase in (
        "the leading DCC bridge",
        "Industry-First Maya support",
        "with no competitor in sight",
        "best-in-class tooling",
    ):
        entry["description"] = f"Maya adapter, {phrase}"
        with pytest.raises(generator.CoverageError, match="banned copy term"):
            generator.render_pointer(entry, host_count=len(adapters))


def test_catalog_read_does_not_need_the_compiled_extension():
    """The generator runs on lanes that never build the wheel.

    Reading the catalog through ``dcc_mcp_core.yaml_loads`` works in a dev
    checkout and in the wheel-installing test lanes, but raises
    ModuleNotFoundError on a bare checkout, where ``dcc_mcp_core._core`` does
    not exist. That is why PyYAML is used here. Asserted at the source level:
    on a machine with the extension built, both routes pass, so a behavioural
    test could not tell them apart.
    """
    source = Path(generator.__file__).read_text(encoding="utf-8")
    assert "from dcc_mcp_core import yaml_loads" not in source
    assert "yaml.safe_load" in source


def test_pyyaml_is_declared_in_the_test_toolchain_contract():
    """The catalog read needs PyYAML, so every test lane must install it.

    It is read through PyYAML rather than the Rust-backed
    ``dcc_mcp_core.yaml_loads`` because this generator also runs on lanes that
    do a bare checkout with no wheel build, where ``_core`` is absent. That
    makes the declaration load-bearing: drop it and all eight ``Test (os, py)``
    lanes go red again.
    """
    contract = json.loads((REPO_ROOT / "compatibility" / "python.json").read_text(encoding="utf-8"))
    toolchain = contract["test_toolchain"]

    # The contract entry is what makes the pinned Python 3.7 toolchain resolve
    # a PyYAML that still ships a cp37 wheel (6.0.2 raised its floor to 3.8).
    assert toolchain["pyyaml_py37"], "test_toolchain.pyyaml_py37 is required"

    # Declared for local `pip install -e .[test]` too, not just for CI lanes.
    pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8").lower()
    assert "pyyaml" in pyproject, "PyYAML must be declared in pyproject.toml"

    # And the wheel-installing test lanes must actually install it. Asserted
    # against the contract-validated requirement list, so this fails if the
    # contract entry is dropped rather than merely if a literal moves.
    sys.path.insert(0, str(REPO_ROOT / "scripts" / "ci"))
    try:
        from python_support_contract import python37_test_requirements

        requirements = python37_test_requirements(contract)
    finally:
        sys.path.remove(str(REPO_ROOT / "scripts" / "ci"))
    assert any("pyyaml" in r.lower() for r in requirements), (
        f"python37_test_requirements() must include PyYAML; got {requirements}"
    )

    ci = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    assert "pip install pytest pytest-xdist jsonschema pyyaml" in ci, (
        "the Test lanes must install PyYAML; a lane that skips it goes red on every catalog-reading test"
    )


def test_render_refuses_a_description_with_banned_copy(adapters):
    entry = dict(_entry(adapters, "dcc-mcp-maya"))
    entry["description"] = "Maya adapter, 领先 的 DCC bridge"
    with pytest.raises(generator.CoverageError, match="banned copy term"):
        generator.render_pointer(entry, host_count=len(adapters))


def test_render_requires_a_description(adapters):
    entry = dict(_entry(adapters, "dcc-mcp-maya"))
    entry["description"] = "   "
    with pytest.raises(generator.CoverageError, match="no `description`"):
        generator.render_pointer(entry, host_count=len(adapters))


# --- insertion -------------------------------------------------------------


QUICKSTART_README = """# dcc-mcp-houdini

<p align="center">banner</p>

<!-- dcc-mcp-agent-quickstart:start -->
## Use Houdini with AI agents

Body.
<!-- dcc-mcp-agent-quickstart:end -->

## Agent workflow

Body.
"""

HEADING_README = """# dcc-mcp-maya

![badge](x.svg)

## Features

Body.
"""

FLAT_README = """# dcc-mcp-flat

Just prose, no headings.
"""

# A blank run well away from where the block lands: the seam is after
# QUICKSTART_END, this one sits in the closing section.
SEAM_FIXTURE = QUICKSTART_README.replace(
    "## Agent workflow\n\nBody.\n",
    "## Agent workflow\n\nBody.\n\n\nBody two.\n",
)


def _split_at(text: str, marker: str) -> tuple[str, str]:
    """Split text at the first occurrence of marker, keeping it on the left."""
    index = text.index(marker) + len(marker)
    return text[:index], text[index:]


def test_insert_after_the_quickstart_block_when_one_is_present(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    result = generator.upsert_pointer(QUICKSTART_README, block)
    lines = result.splitlines()
    quickstart = lines.index(generator.QUICKSTART_END)
    start = next(i for i, line in enumerate(lines) if generator.POINTER_START in line)
    agent_workflow = lines.index("## Agent workflow")
    assert start == quickstart + 2
    assert quickstart < start < agent_workflow


def test_insertion_never_splits_the_quickstart_block(adapters):
    """Landing inside an existing generated block would corrupt both blocks."""
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    result = generator.upsert_pointer(QUICKSTART_README, block)
    start_marker = result.count(generator.QUICKSTART_END)
    assert start_marker == 1
    assert result.index(generator.POINTER_START) > result.index(generator.QUICKSTART_END)


def test_insert_before_the_first_heading_when_there_is_no_quickstart(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    result = generator.upsert_pointer(HEADING_README, block)
    lines = result.splitlines()
    assert lines.index("## Features") == next(i for i, line in enumerate(lines) if generator.POINTER_END in line) + 2


def test_insert_at_the_end_when_the_readme_has_no_headings(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    result = generator.upsert_pointer(FLAT_README, block)
    assert result.rstrip().endswith(generator.POINTER_END)
    assert "Just prose, no headings." in result


def test_insertion_leaves_no_double_blank_line(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    for readme in (QUICKSTART_README, HEADING_README, FLAT_README):
        assert "\n\n\n" not in generator.upsert_pointer(readme, block)


def test_insertion_preserves_a_trailing_blank_line(adapters):
    """A README ending in a blank line keeps it.

    splitlines() drops trailing blank lines, so a naive round trip deletes
    them and the diff picks up an unrelated deletion at end of file.
    """
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    for suffix in ("\n", "\n\n", "\n\n\n"):
        readme = HEADING_README.rstrip("\n") + suffix
        result = generator.upsert_pointer(readme, block)
        assert result.endswith(suffix), f"trailing {suffix!r} was not preserved"


def test_insertion_leaves_every_byte_outside_the_seam_alone(adapters):
    """A blank run away from the seam is the README's own formatting.

    This is the regression the counting version of this test missed: a global
    blank-run collapse deletes a run far from the insertion point, and the
    seam logic then adds one back, so the count is unchanged while the file
    is corrupted. Comparing the text outside the seam byte for byte catches it.
    """
    readme = SEAM_FIXTURE
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    result = generator.upsert_pointer(readme, block)

    marker = generator.QUICKSTART_END
    assert "\n\n\n" in readme, "fixture must hold a blank run away from the seam"
    head, tail = _split_at(readme, marker)
    result_head, result_tail = _split_at(result, marker)
    assert result_head == head, "text before the seam was modified"
    # The block is inserted at the seam, so the untouched tail must survive
    # verbatim as the suffix -- every byte of it, blank runs included.
    assert result_tail == "\n\n" + generator.POINTER_START + result_tail.split(generator.POINTER_START, 1)[1]
    assert result.endswith(tail), "text after the seam was modified"
    # The far-away blank run survives.
    assert result.count("\n\n\n") == readme.count("\n\n\n")


def test_insertion_keeps_blank_lines_that_are_not_at_the_seam(adapters):
    """Only the blank lines touching the insertion point may be adjusted.

    A filter over the whole prefix silently deletes every paragraph break in
    the README, which buries the real edit in noise.
    """
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    result = generator.upsert_pointer(QUICKSTART_README, block)
    assert result.count("") >= QUICKSTART_README.count("")
    for line in QUICKSTART_README.splitlines():
        if line.strip():
            assert line in result
    # The blank-line separators inside the untouched quickstart block survive.
    assert QUICKSTART_README.count("\n\n") <= result.count("\n\n")


def test_insertion_preserves_crlf_line_endings(adapters):
    """CRLF READMEs are the norm on Windows checkouts.

    Rewriting one as LF would show every line as changed and hide the actual
    edit in the diff.
    """
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    crlf_readme = QUICKSTART_README.replace("\n", "\r\n")
    result = generator.upsert_pointer(crlf_readme, block)
    assert "\r\n" in result
    assert "\n" not in result.replace("\r\n", "")
    assert result.count(generator.POINTER_START) == 1


def test_apply_is_idempotent(adapters):
    """Re-running the generator on an up-to-date README must be a no-op."""
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    once = generator.upsert_pointer(QUICKSTART_README, block)
    twice = generator.upsert_pointer(once, block)
    assert once == twice


def test_refresh_replaces_a_stale_block(adapters):
    """Changing the catalog must be able to overwrite an older generated block."""
    old = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=10)
    new = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=38)
    with_old = generator.upsert_pointer(QUICKSTART_README, old)
    refreshed = generator.upsert_pointer(with_old, new)
    assert "10 host adapters" not in refreshed
    assert "38 host adapters" in _flat(refreshed)
    assert refreshed.count(generator.POINTER_START) == 1


def test_orphaned_marker_is_an_error_not_a_silent_second_block(adapters):
    block = generator.render_pointer(_entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))
    broken = f"{generator.POINTER_START}\nloose text\n"
    with pytest.raises(generator.CoverageError, match="orphaned marker"):
        generator.upsert_pointer(broken, block)


# --- adapter resolution ----------------------------------------------------


def test_resolve_matches_git_origin_case_insensitively(adapters, tmp_path):
    """The catalog says dcc-mcp-PowerPoint; GitHub reports dcc-mcp-powerpoint."""
    repo = tmp_path / "repo"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-powerpoint.git"],
        check=True,
    )
    entry = generator.resolve_adapter(adapters, repo)
    assert entry["name"] == "dcc-mcp-PowerPoint"


def test_resolve_rejects_a_repository_the_catalog_does_not_know(adapters, tmp_path):
    repo = tmp_path / "other"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-unknown.git"],
        check=True,
    )
    with pytest.raises(generator.CoverageError, match="no catalog entry matches"):
        generator.resolve_adapter(adapters, repo)


# --- apply -----------------------------------------------------------------


def test_apply_writes_the_block(adapters, tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "README.md").write_text(QUICKSTART_README, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-houdini.git"],
        check=True,
    )
    path, changed = generator.apply_pointer(repo, _entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters))
    assert changed is True
    assert generator.POINTER_START in path.read_text(encoding="utf-8")


def test_apply_check_reports_without_writing(adapters, tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    original = QUICKSTART_README
    (repo / "README.md").write_text(original, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-houdini.git"],
        check=True,
    )
    _, changed = generator.apply_pointer(
        repo, _entry(adapters, "dcc-mcp-houdini"), host_count=len(adapters), write=False
    )
    assert changed is True
    assert (repo / "README.md").read_text(encoding="utf-8") == original


def test_apply_is_a_no_op_on_an_up_to_date_readme(adapters, tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "README.md").write_text(QUICKSTART_README, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-houdini.git"],
        check=True,
    )
    entry = _entry(adapters, "dcc-mcp-houdini")
    generator.apply_pointer(repo, entry, host_count=len(adapters))
    _, changed = generator.apply_pointer(repo, entry, host_count=len(adapters))
    assert changed is False


def test_apply_fails_loudly_when_the_readme_is_missing(adapters, tmp_path):
    with pytest.raises(generator.CoverageError, match="does not exist"):
        generator.apply_pointer(tmp_path, _entry(adapters, "dcc-mcp-maya"), host_count=len(adapters))


# --- coverage report -------------------------------------------------------


def test_every_adapter_gets_a_coverage_row(adapters):
    """Acceptance: no adapter may be silently skipped."""
    rows = generator.coverage_rows(adapters)
    assert len(rows) == len(adapters) == 47
    assert {r["name"] for r in rows} == {e["name"] for e in adapters}


def test_rows_split_on_whether_the_catalog_attests_a_pip_package(adapters):
    rows = generator.coverage_rows(adapters)
    pending = {r["name"] for r in rows if r["registry"] == "pending"}
    deferred = {r["name"] for r in rows if r["registry"] == "deferred"}
    assert len(pending) == 44
    assert deferred == {
        "dcc-mcp-excel",
        "dcc-mcp-word",
        "dcc-mcp-material-maker",
    }


def test_deferred_rows_carry_a_reason_so_the_gap_stays_enumerable(adapters):
    rows = generator.coverage_rows(adapters)
    for row in rows:
        assert row["blocker"]
        if row["registry"] == "deferred":
            assert "install" in row["blocker"]


def test_report_lists_every_adapter(adapters):
    markdown = generator.render_report(adapters)
    for entry in adapters:
        assert f"`{entry['name']}`" in markdown


def test_report_carries_no_banned_copy(adapters):
    markdown = generator.render_report(adapters)
    for term in generator.BANNED_COPY_TERMS:
        assert term not in markdown


def test_report_is_deterministic(adapters):
    assert generator.render_report(adapters) == generator.render_report(adapters)


def test_report_check_detects_a_stale_map(adapters, tmp_path, capsys):
    out = tmp_path / "adapter-coverage.md"
    out.write_text("stale\n", encoding="utf-8")
    json_out = tmp_path / "adapter-coverage.json"
    json_out.write_text("{}\n", encoding="utf-8")
    rc = generator.main(
        [
            "--catalog",
            str(CATALOG),
            "report",
            "--out",
            str(out),
            "--json-out",
            str(json_out),
            "--check",
        ]
    )
    captured = capsys.readouterr()
    assert rc == 1
    assert "stale" in captured.out


def test_report_check_accepts_a_current_map(adapters, tmp_path):
    out = tmp_path / "adapter-coverage.md"
    json_out = tmp_path / "adapter-coverage.json"
    assert (
        generator.main(
            [
                "--catalog",
                str(CATALOG),
                "report",
                "--out",
                str(out),
                "--json-out",
                str(json_out),
            ]
        )
        == 0
    )
    assert (
        generator.main(
            [
                "--catalog",
                str(CATALOG),
                "report",
                "--out",
                str(out),
                "--json-out",
                str(json_out),
                "--check",
            ]
        )
        == 0
    )
    payload = json.loads(json_out.read_text(encoding="utf-8"))
    assert payload["adapter_count"] == 47
    assert len(payload["entries"]) == 47


# --- cli -------------------------------------------------------------------


def test_cli_apply_exits_non_zero_when_the_readme_is_stale(adapters, tmp_path):
    """`--check` is the CI gate: a stale or missing block must fail the run."""
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "README.md").write_text(QUICKSTART_README, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "remote", "add", "origin", "https://github.com/dcc-mcp/dcc-mcp-houdini.git"],
        check=True,
    )
    stale_rc = generator.main(
        [
            "--catalog",
            str(CATALOG),
            "apply",
            "--repo-root",
            str(repo),
            "--check",
        ]
    )
    assert stale_rc == 1

    generator.main(["--catalog", str(CATALOG), "apply", "--repo-root", str(repo)])
    fresh_rc = generator.main(
        [
            "--catalog",
            str(CATALOG),
            "apply",
            "--repo-root",
            str(repo),
            "--check",
        ]
    )
    assert fresh_rc == 0


def test_cli_render_rejects_an_unknown_adapter():
    with pytest.raises(generator.CoverageError, match="no adapter named"):
        generator.main(["--catalog", str(CATALOG), "render", "--adapter", "dcc-mcp-nope"])
