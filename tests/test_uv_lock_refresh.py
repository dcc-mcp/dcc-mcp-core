"""Scheduled uv.lock refresh guard contract tests."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "uv_lock_refresh.py"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "uv-lock-refresh.yml"
# Security boundary owned by release-please-lock-sync.yml; the refresh workflow
# must never execute or redefine it.
PINNED_VALIDATOR_REF = "b8e294e8a64abba426871b1e86eb106e75aab075"


def _load_module():
    spec = importlib.util.spec_from_file_location("uv_lock_refresh", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _wheel(package: str, version: str, tag: str) -> str:
    return f"https://files.pythonhosted.org/packages/ab/cd/{package}-{version}-{tag}.whl"


def _semantic_wheels(version: str, *, cp37: bool = True) -> list:
    wheels = []
    if cp37:
        wheels.append(_wheel("dcc_mcp_core_semantic", version, "cp37-cp37m-manylinux_2_28_x86_64"))
    wheels.append(_wheel("dcc_mcp_core_semantic", version, "cp38-abi3-manylinux_2_28_x86_64"))
    return wheels


def _server_wheels(version: str) -> list:
    return [_wheel("dcc_mcp_server", version, "py3-none-manylinux2014_x86_64")]


def _parse(text: str) -> dict:
    try:
        import tomllib
    except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
        import tomli as tomllib

    return tomllib.loads(text)


def _lock_text(*, requires_python: str = ">=3.7", resolution_markers: list | None = None, packages: list) -> str:
    lines = ["version = 1", "revision = 3", f'requires-python = "{requires_python}"']
    if resolution_markers is not None:
        lines.append("resolution-markers = [")
        for marker in resolution_markers:
            lines.append(f'    "{marker}",')
        lines.append("]")
    lines.append("")
    for entry in packages:
        # A package entry is `(name, version, wheels)` or
        # `(name, version, wheels, markers)`; `markers` are the marker strings
        # carried by that block's `dependencies` entries.
        name, version, wheels = entry[0], entry[1], entry[2]
        markers = entry[3] if len(entry) > 3 else ()
        lines.append("[[package]]")
        lines.append(f'name = "{name}"')
        lines.append(f'version = "{version}"')
        if markers:
            lines.append("dependencies = [")
            for marker in markers:
                lines.append(f'    {{ name = "dep", marker = "{marker}" }},')
            lines.append("]")
        if wheels:
            lines.append("wheels = [")
            for url in wheels:
                lines.append(f'    {{ url = "{url}", hash = "sha256:deadbeef" }},')
            lines.append("]")
        lines.append("")
    return "\n".join(lines)


def _record(spec, default_hash: str) -> tuple:
    """Split an ``sdist`` / ``wheels`` spec into the ``(url, hash)`` it renders.

    A bare string is a URL; a ``(url, hash)`` pair pins the hash as well, which
    is how a re-uploaded artifact is modelled.
    """
    if isinstance(spec, tuple):
        return spec[0], spec[1]
    return spec, default_hash


def _block(
    name: str,
    version: str,
    *,
    dependencies=(),
    optional=None,
    requires_dist=None,
    source=None,
    sdist=None,
    wheels=(),
    block_resolution_markers=None,
) -> str:
    """Render one ``[[package]]`` block exercising every fingerprinted field.

    Keys that belong to ``[[package]]`` itself are emitted before the
    ``[package.*]`` sub-tables, which is what puts them inside the block rather
    than inside ``optional-dependencies`` or ``metadata``.
    """
    lines = ["[[package]]", f'name = "{name}"', f'version = "{version}"']
    if dependencies:
        lines.append("dependencies = [")
        for marker in dependencies:
            lines.append(f'    {{ name = "dep", marker = "{marker}" }},')
        lines.append("]")
    if source is not None:
        lines.append(f'source = {{ registry = "{source}" }}')
    if sdist is not None:
        url, digest = _record(sdist, "sha256:sdist")
        lines.append(f'sdist = {{ url = "{url}", hash = "{digest}", size = 1024 }}')
    if wheels:
        lines.append("wheels = [")
        for spec in wheels:
            url, digest = _record(spec, "sha256:wheel")
            lines.append(f'    {{ url = "{url}", hash = "{digest}", size = 2048 }},')
        lines.append("]")
    if block_resolution_markers:
        lines.append("resolution-markers = [")
        for marker in block_resolution_markers:
            lines.append(f'    "{marker}",')
        lines.append("]")
    for group in sorted(optional or {}):
        lines.append("[package.optional-dependencies]")
        lines.append(f"{group} = [")
        for marker in optional[group]:
            lines.append(f'    {{ name = "extra-dep", marker = "{marker}" }},')
        lines.append("]")
    if requires_dist:
        lines.append("[package.metadata]")
        lines.append("requires-dist = [")
        for marker in requires_dist:
            lines.append(f'    {{ name = "dist", marker = "{marker}" }},')
        lines.append("]")
    lines.append("")
    return "\n".join(lines)


def _lock_from_blocks(*blocks: str, requires_python: str = ">=3.7", resolution_markers: list | None = None) -> str:
    lines = ["version = 1", "revision = 3", f'requires-python = "{requires_python}"']
    if resolution_markers is not None:
        lines.append("resolution-markers = [")
        for marker in resolution_markers:
            lines.append(f'    "{marker}",')
        lines.append("]")
    lines.append("")
    lines.extend(blocks)
    return "\n".join(lines)


# The layering uv emits for a project that still supports Python 3.7: newest
# first, with the py37 marker last.
_PY37_RESOLUTION_MARKERS = [
    "python_full_version >= '3.14'",
    "python_full_version == '3.13.*'",
    "python_full_version == '3.9.*'",
    "python_full_version == '3.8.*'",
    "python_full_version < '3.8'",
]


def _baseline():
    return [
        ("dcc-mcp-core-semantic", "0.20.33", _semantic_wheels("0.20.33")),
        ("dcc-mcp-server", "0.20.33", _server_wheels("0.20.33")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")]),
    ]


def _refreshed():
    return [
        ("dcc-mcp-core-semantic", "0.20.34", _semantic_wheels("0.20.34")),
        ("dcc-mcp-server", "0.20.34", _server_wheels("0.20.34")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")]),
    ]


def test_unchanged_lock_passes() -> None:
    module = _load_module()
    lock = _parse(_lock_text(packages=_baseline()))

    assert module.verify_refresh(lock, lock) == []


def test_allowed_package_bump_passes() -> None:
    module = _load_module()

    assert (
        module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=_refreshed()))) == []
    )


def test_unmanaged_package_bump_fails() -> None:
    module = _load_module()
    drifted = _refreshed()
    drifted[2] = ("zipp", "3.21.0", [_wheel("zipp", "3.21.0", "py3-none-any")])

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=drifted)))

    assert len(errors) == 1
    assert "zipp" in errors[0]
    assert "outside the allowed refresh set" in errors[0]


def test_dropped_cp37_wheel_fails() -> None:
    module = _load_module()
    without_cp37 = _refreshed()
    without_cp37[0] = ("dcc-mcp-core-semantic", "0.20.34", _semantic_wheels("0.20.34", cp37=False))

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=without_cp37)))

    assert any("cp37" in error and "dcc-mcp-core-semantic" in error for error in errors)


def test_narrowed_requires_python_fails() -> None:
    module = _load_module()
    before = _parse(_lock_text(packages=_baseline()))
    after = _parse(_lock_text(requires_python=">=3.9", packages=_baseline()))

    errors = module.verify_refresh(before, after)

    assert any("requires-python" in error for error in errors)


def test_removed_allowed_pin_fails() -> None:
    module = _load_module()
    trimmed = [entry for entry in _refreshed() if entry[0] != "dcc-mcp-server"]

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=trimmed)))

    assert any("dcc-mcp-server" in error and "removed" in error for error in errors)


def test_downgraded_allowed_pin_fails() -> None:
    module = _load_module()
    # `0.20.34` got yanked, so the resolver falls back to an older release.
    rolled_back = _refreshed()
    rolled_back[0] = ("dcc-mcp-core-semantic", "0.20.32", _semantic_wheels("0.20.32"))

    errors = module.verify_refresh(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=rolled_back)))

    assert any("backwards" in error and "dcc-mcp-core-semantic" in error for error in errors)


def test_marker_only_relock_is_classified_as_marker_only() -> None:
    # The case PR #2643 actually was: `uv lock` re-emitted dependency markers
    # on package blocks whose versions never moved, so the pull request must
    # not describe itself as a version refresh.
    module = _load_module()
    before = [
        ("dcc-mcp-core-semantic", "0.20.38", _semantic_wheels("0.20.38")),
        ("dcc-mcp-server", "0.20.38", _server_wheels("0.20.38")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")]),
    ]
    after = [
        ("dcc-mcp-core-semantic", "0.20.38", _semantic_wheels("0.20.38"), ["python_full_version >= '3.8'"]),
        ("dcc-mcp-server", "0.20.38", _server_wheels("0.20.38")),
        ("zipp", "3.19.1", [_wheel("zipp", "3.19.1", "py3-none-any")], ["python_full_version < '3.8'"]),
    ]

    report = module.classify_change(_parse(_lock_text(packages=before)), _parse(_lock_text(packages=after)))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["version_changes"] == []
    assert report["marker_changes"] == ["dcc-mcp-core-semantic==0.20.38", "zipp==3.19.1"]
    assert report["resolution_markers_changed"] is False
    assert "marker-only" in module.describe_change(report)
    assert "no package version changed" in module.describe_change(report)


def test_marker_only_relock_still_passes_the_guard() -> None:
    # Widening the classifier must not turn a marker-only relock into a
    # failure: nothing left the allowed blast radius.
    module = _load_module()
    before = [("dcc-mcp-core-semantic", "0.20.38", _semantic_wheels("0.20.38"))]
    after = [("dcc-mcp-core-semantic", "0.20.38", _semantic_wheels("0.20.38"), ["python_full_version >= '3.8'"])]

    errors = module.verify_refresh(_parse(_lock_text(packages=before)), _parse(_lock_text(packages=after)))

    assert errors == []


def test_identical_locks_classify_as_unchanged() -> None:
    module = _load_module()
    lock = _parse(_lock_text(packages=_baseline()))

    report = module.classify_change(lock, lock)

    assert report["kind"] == module.CHANGE_UNCHANGED
    assert "unchanged" in module.describe_change(report)


def test_version_bump_classifies_as_versions_not_marker_only() -> None:
    module = _load_module()

    report = module.classify_change(_parse(_lock_text(packages=_baseline())), _parse(_lock_text(packages=_refreshed())))

    assert report["kind"] == module.CHANGE_VERSIONS
    assert report["version_changes"] == ["dcc-mcp-core-semantic", "dcc-mcp-server"]
    assert "version refresh" in module.describe_change(report)


def test_reordered_resolution_markers_are_detected_without_a_version_change() -> None:
    # The resolution-marker list encodes the resolver's Python-version layering,
    # so swapping two entries is a real change that a version-only comparison
    # would miss entirely.
    module = _load_module()
    reordered = list(_PY37_RESOLUTION_MARKERS)
    reordered[0], reordered[-1] = reordered[-1], reordered[0]

    report = module.classify_change(
        _parse(_lock_text(packages=_baseline(), resolution_markers=_PY37_RESOLUTION_MARKERS)),
        _parse(_lock_text(packages=_baseline(), resolution_markers=reordered)),
    )

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["version_changes"] == []
    assert report["marker_changes"] == []
    assert report["resolution_markers_changed"] is True
    assert "resolution-marker list changed" in module.describe_change(report)


def test_optional_dependency_markers_are_fingerprinted() -> None:
    # One surface per test on purpose: a case that moves both
    # `optional-dependencies` and `requires-dist` stays green when either read
    # alone is dropped, which is exactly the silent regression this pins.
    module = _load_module()
    before = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", optional={"test": ["extra == 'test'"]}),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block(
            "dcc-mcp-core",
            "0.20.38",
            optional={"test": ["python_full_version >= '3.8' and extra == 'test'"]},
        ),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_requires_dist_markers_are_fingerprinted() -> None:
    # The `[package.metadata] requires-dist` surface on its own, so dropping
    # that read turns the suite red.
    module = _load_module()
    before = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", requires_dist=["extra == 'dev'"]),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", requires_dist=["python_full_version >= '3.8' and extra == 'dev'"]),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_block_level_resolution_markers_are_fingerprinted() -> None:
    # `resolution-markers` is not only a top-level list: uv writes one per
    # package block as well, and re-layering a block moves bytes the global
    # comparison never sees.
    module = _load_module()
    before = _lock_from_blocks(
        _block(
            "dcc-mcp-core",
            "0.20.38",
            block_resolution_markers=["python_full_version >= '3.8'", "python_full_version < '3.8'"],
        ),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block(
            "dcc-mcp-core",
            "0.20.38",
            block_resolution_markers=["python_full_version < '3.8'", "python_full_version >= '3.8'"],
        ),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["resolution_markers_changed"] is False
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_added_wheel_is_fingerprinted() -> None:
    # A wheel added for a release that did not move is real byte drift the
    # marker tables never mention.
    module = _load_module()
    before = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", wheels=[_wheel("dcc-mcp-core", "0.20.38", "py3-none-any")]),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block(
            "dcc-mcp-core",
            "0.20.38",
            wheels=[
                _wheel("dcc-mcp-core", "0.20.38", "py3-none-any"),
                _wheel("dcc-mcp-core", "0.20.38", "cp39-cp39-manylinux_2_28_x86_64"),
            ],
        ),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["version_changes"] == []
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_reuploaded_sdist_is_fingerprinted() -> None:
    # Same URL, new hash: the artifact bytes changed while no version and no
    # marker did.
    module = _load_module()
    url = "https://files.pythonhosted.org/packages/ab/cd/dcc-mcp-core-0.20.38.tar.gz"
    before = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", sdist=(url, "sha256:before")),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", sdist=(url, "sha256:after")),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["version_changes"] == []
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_changed_source_is_fingerprinted() -> None:
    # A package re-resolved from a different index carries no marker change at
    # all, but it is not the same resolution.
    module = _load_module()
    before = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", source="https://pypi.org/simple"),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block("dcc-mcp-core", "0.20.38", source="https://internal.example.com/simple"),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    report = module.classify_change(_parse(before), _parse(after))

    assert report["kind"] == module.CHANGE_MARKER_ONLY
    assert report["version_changes"] == []
    assert report["marker_changes"] == ["dcc-mcp-core==0.20.38"]


def test_artifact_drift_does_not_change_the_guard_verdict() -> None:
    # Widening the fingerprint must only make the classifier more honest: an
    # artifact-only relock still stays inside the allowed blast radius.
    module = _load_module()
    baseline_wheels = _semantic_wheels("0.20.38")
    extra_wheel = _wheel("dcc_mcp_core_semantic", "0.20.38", "cp313-cp313-manylinux_2_28_x86_64")
    before = _lock_from_blocks(
        _block("dcc-mcp-core-semantic", "0.20.38", wheels=baseline_wheels),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block("dcc-mcp-core-semantic", "0.20.38", wheels=[*baseline_wheels, extra_wheel]),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    assert module.verify_refresh(_parse(before), _parse(after)) == []


def test_reordering_package_blocks_does_not_look_like_a_change() -> None:
    module = _load_module()
    before = _lock_from_blocks(
        _block("attrs", "24.2.0", dependencies=["python_full_version < '3.8'"]),
        _block("zipp", "3.19.1"),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )
    after = _lock_from_blocks(
        _block("zipp", "3.19.1"),
        _block("attrs", "24.2.0", dependencies=["python_full_version < '3.8'"]),
        resolution_markers=_PY37_RESOLUTION_MARKERS,
    )

    assert module.classify_change(_parse(before), _parse(after))["kind"] == module.CHANGE_UNCHANGED


def test_classify_cli_prints_json_and_writes_github_outputs(tmp_path: Path, monkeypatch) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(packages=_refreshed()), encoding="utf-8")
    outputs = tmp_path / "github-output"
    monkeypatch.setenv("GITHUB_OUTPUT", str(outputs))

    assert module.main(["classify", str(before), str(after), "--json"]) == 0

    written = outputs.read_text(encoding="utf-8")
    assert "change-kind=versions" in written
    assert "marker-only=false" in written
    assert "version-changes=dcc-mcp-core-semantic,dcc-mcp-server" in written
    assert "summary=version refresh:" in written


def test_classify_cli_reports_marker_only_for_a_marker_relock(tmp_path: Path, monkeypatch) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=[("zipp", "3.19.1", [])]), encoding="utf-8")
    after.write_text(_lock_text(packages=[("zipp", "3.19.1", [], ["python_full_version < '3.8'"])]), encoding="utf-8")
    outputs = tmp_path / "github-output"
    monkeypatch.setenv("GITHUB_OUTPUT", str(outputs))

    assert module.main(["classify", str(before), str(after)]) == 0

    written = outputs.read_text(encoding="utf-8")
    assert "change-kind=marker-only" in written
    assert "marker-only=true" in written
    assert "version-changes=\n" in written


def test_cli_verify_accepts_a_scoped_refresh(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(packages=_refreshed()), encoding="utf-8")

    assert module.main(["verify", str(before), str(after)]) == 0


def test_cli_verify_rejects_an_unscoped_refresh(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(requires_python=">=3.10", packages=_baseline()), encoding="utf-8")

    assert module.main(["verify", str(before), str(after)]) == 1


def test_cli_verify_honours_a_package_override(tmp_path: Path) -> None:
    module = _load_module()
    before = tmp_path / "before.lock"
    after = tmp_path / "after.lock"
    moved = _baseline()
    moved[2] = ("zipp", "3.21.0", [_wheel("zipp", "3.21.0", "py3-none-any")])
    before.write_text(_lock_text(packages=_baseline()), encoding="utf-8")
    after.write_text(_lock_text(packages=moved), encoding="utf-8")

    assert module.main(["verify", str(before), str(after), "--package", "zipp"]) == 0
    assert module.main(["verify", str(before), str(after)]) == 1


@pytest.mark.parametrize(
    "expected",
    [
        "uv lock --upgrade-package dcc-mcp-core-semantic --upgrade-package dcc-mcp-server",
        "scripts/ci/uv_lock_refresh.py verify",
        "scripts/ci/check_lock_versions.py",
        "scripts/ci/check_uv_lock.py",
        "uv lock --check",
        "chore(lock):",
        "bot/uv-lock-refresh",
        "git diff --quiet -- uv.lock",
        "python scripts/ci/uv_lock_refresh.py classify",
    ],
)
def test_workflow_carries_the_refresh_contract(expected: str) -> None:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    assert expected in workflow


def _workflow_body() -> str:
    """Return the workflow with its explanatory comments stripped."""
    lines = [
        line for line in WORKFLOW_PATH.read_text(encoding="utf-8").splitlines() if not line.lstrip().startswith("#")
    ]
    return "\n".join(lines)


def test_workflow_never_touches_the_pinned_validator_boundary() -> None:
    workflow = _workflow_body()

    assert PINNED_VALIDATOR_REF not in workflow
    assert "pull_request_target" not in workflow
    assert "generated_lock_sync.py" not in workflow


def test_workflow_does_not_auto_merge() -> None:
    # `main` has no required status checks and no required review, so
    # auto-merge would land on the first status event. The refresh must stop
    # at "open" and let a human merge it.
    workflow = _workflow_body()

    assert "gh pr merge" not in workflow
    assert "--auto" not in workflow


def test_workflow_token_stays_read_only() -> None:
    # Every write goes through secrets.PERSONAL_ACCESS_TOKEN; the workflow
    # token must not carry write scopes it never exercises.
    workflow = _workflow_body()

    assert "contents: read" in workflow
    assert "contents: write" not in workflow
    assert "pull-requests: write" not in workflow


def test_workflow_pins_the_resolver() -> None:
    workflow = _workflow_body()

    assert "UV_VERSION" in workflow
    assert "uv==" in workflow
    assert "timeout-minutes" in workflow


def test_workflow_derives_the_pr_title_from_the_classifier() -> None:
    # A relock that moves no version must not be titled as a version refresh,
    # so the title is picked from `uv_lock_refresh.py classify` rather than
    # being a single hardcoded string.
    workflow = _workflow_body()

    assert "steps.kind.outputs.change-kind" in workflow
    assert 'case "$CHANGE_KIND" in' in workflow
    assert "TITLE_MARKER_ONLY" in workflow
    assert "TITLE_VERSIONS" in workflow
    assert "TITLE_UNCHANGED" in workflow


def test_workflow_has_no_single_hardcoded_title() -> None:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    assert "$REFRESH_TITLE" not in workflow
    assert '--title "$refresh_title"' in workflow


def test_workflow_marker_only_title_does_not_claim_a_version_refresh() -> None:
    workflow = _workflow_body()
    titles = [line for line in workflow.splitlines() if line.strip().startswith("TITLE_MARKER_ONLY:")]

    assert len(titles) == 1
    assert "without changing package versions" in titles[0]


def test_workflow_states_the_change_kind_in_the_pr_body() -> None:
    workflow = _workflow_body()

    assert "**Classification:**" in workflow
    assert "BODY_MARKER_ONLY" in workflow
    assert "BODY_VERSIONS" in workflow
    assert "BODY_UNCHANGED" in workflow
    assert "gh pr edit" in workflow


def test_workflow_gives_unchanged_its_own_title_and_body() -> None:
    # The diff gate only proves `uv.lock` bytes moved, not that the classifier
    # can see the change, so `unchanged` is reachable and must not inherit the
    # version-refresh wording through the `*` fallback.
    workflow = _workflow_body()
    titles = [line for line in workflow.splitlines() if line.strip().startswith("TITLE_UNCHANGED:")]

    assert len(titles) == 1
    assert "classifiable" in titles[0]
    assert "version refresh" not in titles[0]
    assert 'unchanged) refresh_title="$TITLE_UNCHANGED"' in workflow
    assert 'elif [ "$CHANGE_KIND" = "unchanged" ]; then' in workflow


def test_workflow_does_not_claim_unchanged_is_unreachable() -> None:
    # The old comment asserted the diff gate made `unchanged` impossible; the
    # gate proves only that the bytes moved.
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    assert "cannot reach this step" not in workflow


def test_workflow_schedule_avoids_the_hour_and_the_release_window() -> None:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    cron_lines = [line.strip() for line in workflow.splitlines() if line.strip().startswith("- cron:")]
    assert cron_lines, "the refresh workflow must declare a schedule"

    for line in cron_lines:
        expression = line.split("cron:", 1)[1].strip().strip('"')
        minute, hour = expression.split()[0], expression.split()[1]
        assert minute != "0"
        assert minute != "00"
        assert not (hour == "1" and minute == "13")
        assert hour == "*" or int(hour) != 1
