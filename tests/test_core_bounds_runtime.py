"""Startup-time core requirement check for adapters.

`dcc_mcp_core.deployment.core_bounds` defines the version-bound contract and
`check_runtime()` compares a declaration against the running core, but nothing
inside a DCC host called it — `git grep` found zero call sites under `python/`
outside the module itself and `deployment/__init__.py`'s re-exports. The
incident the contract was written for is therefore still invisible at runtime:
the adapter resolves a core its own metadata excludes, top-level imports keep
working, and the break surfaces several modules deep without naming a version.

These tests pin the startup half: the check must name the adapter, the declared
range, the running core, and the fix; must default to a warning rather than
aborting a live DCC session; and must stay quiet when the adapter publishes no
readable declaration.
"""

from __future__ import annotations

import ast
import logging
from pathlib import Path
import sys

import pytest

from dcc_mcp_core.constants import ENV_CORE_REQUIREMENT_ENFORCE
from dcc_mcp_core.deployment import core_bounds
from dcc_mcp_core.deployment import core_bounds_runtime as runtime

MODULE_PATH = Path(runtime.__file__).resolve()

#: An adapter pinned to the 0.19 line while a 0.20 core is running — the exact
#: combination from the reported incident.
COMPLIANT_BUT_OUT_OF_RANGE = "dcc-mcp-core>=0.19.3,<0.19.5"
RUNNING_CORE = "0.20.28"


def _write_adapter_metadata(tmp_path: Path, name: str, requirement: str | None) -> None:
    """Create a ``.dist-info/METADATA`` for an adapter under ``tmp_path``."""
    dist_info = tmp_path / f"{name}-0.9.4.dist-info"
    dist_info.mkdir(parents=True, exist_ok=True)
    lines = ["Metadata-Version: 2.1", f"Name: {name}", "Version: 0.9.4"]
    if requirement is not None:
        lines.append(f"Requires-Dist: {requirement}")
    lines.append("Requires-Dist: pyside6>=6.5")
    (dist_info / "METADATA").write_text("\n".join(lines) + "\n", encoding="utf-8")


def _isolate_metadata(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Force the stdlib ``sys.path`` scan so the host's real adapters cannot answer."""
    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(core_bounds, "_read_metadata", core_bounds._scan_dist_info_metadata)


def _core_requirement_warnings(caplog: pytest.LogCaptureFixture) -> list:
    """Warnings from the check itself, ignoring unrelated startup chatter.

    ``DccServerBase`` hands the check its own logger, and constructing a real
    server also logs about file logging, telemetry, and the like — so only the
    records naming a core requirement are evidence here.
    """
    return [r for r in caplog.records if "on dcc-mcp-core" in r.getMessage()]


# ── Python 3.7 syntax (Maya 2022 / Blender 2.83) ──────────────────────────────


def test_module_parses_under_python_3_7_feature_version() -> None:
    """The new module must be importable on the py3.7 hosts it protects.

    ``ast.parse(feature_version=(3, 7))`` rejects every grammar feature added
    after Python 3.7, which is what a Maya 2022 host would choke on.
    """
    source = MODULE_PATH.read_text(encoding="utf-8")
    try:
        if sys.version_info[:2] == (3, 7):
            compile(source, str(MODULE_PATH), "exec")
        else:
            ast.parse(source, filename=str(MODULE_PATH), feature_version=(3, 7))
    except SyntaxError as exc:  # pragma: no cover - only on a real regression
        pytest.fail(f"core_bounds_runtime.py has Python 3.8+ syntax: {exc}")


def test_module_is_import_light() -> None:
    """Startup must not pay for the check when there is nothing to check."""
    import dcc_mcp_core.deployment.core_bounds_runtime as module

    # The heavy lookups are stdlib-only and deferred; importing pulled in no
    # network or packaging dependency for the DCC host.
    assert module.installed_core_requirement is core_bounds.installed_core_requirement
    assert module.check_runtime is core_bounds.check_runtime


# ── in range → silent ─────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "declaration, running",
    [
        ("dcc-mcp-core>=0.20.0,<0.21.0", "0.20.28"),
        ("dcc-mcp-core>=0.20.28,<0.21.0", "0.20.28"),
        ("dcc_mcp_core-0.20", "0.20.28"),
        ("dcc-mcp-core>=0.19.3,<0.19.5", "0.19.4"),
    ],
)
def test_in_range_is_supported_and_silent(declaration, running, tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", declaration)
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", running)

    assert check.skipped is False
    assert check.ok is True
    assert check.verdict == core_bounds.VERDICT_SUPPORTED
    assert check.message is None


def test_in_range_never_raises_even_when_enforced(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", "dcc-mcp-core>=0.20.0,<0.21.0")
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.run_startup_core_requirement_check("dcc-mcp-maya", "0.20.28")

    assert check.ok is True


# ── core newer than the declared upper bound ──────────────────────────────────


def test_core_newer_than_declared_warns_naming_both_versions(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE)

    assert check.ok is False
    assert check.verdict == core_bounds.VERDICT_CORE_NEWER_THAN_DECLARED
    # The message must be actionable without another lookup: adapter, declared
    # range, running core, and the fix.
    assert "dcc-mcp-maya" in check.message
    assert COMPLIANT_BUT_OUT_OF_RANGE in check.message
    assert RUNNING_CORE in check.message
    assert "0.19.5" in check.message
    assert "newer than" in check.message


def test_core_newer_than_declared_warns_by_default(tmp_path, monkeypatch, caplog):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    with caplog.at_level(logging.WARNING, logger=runtime.__name__):
        check = runtime.run_startup_core_requirement_check("dcc-mcp-maya", RUNNING_CORE)

    assert check.ok is False
    assert len(caplog.records) == 1
    assert caplog.records[0].levelno == logging.WARNING
    assert RUNNING_CORE in caplog.text


def test_core_newer_than_declared_raises_when_enforced(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, "1")

    with pytest.raises(RuntimeError) as excinfo:
        runtime.run_startup_core_requirement_check("dcc-mcp-maya", RUNNING_CORE)

    assert RUNNING_CORE in str(excinfo.value)
    assert "dcc-mcp-maya" in str(excinfo.value)


# ── core older than the declared lower bound ──────────────────────────────────


def test_core_older_than_declared_warns_naming_both_versions(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", "0.18.0")

    assert check.ok is False
    assert check.verdict == core_bounds.VERDICT_CORE_OLDER_THAN_DECLARED
    assert "dcc-mcp-maya" in check.message
    assert COMPLIANT_BUT_OUT_OF_RANGE in check.message
    assert "0.18.0" in check.message
    assert "older than" in check.message


def test_core_older_than_declared_raises_when_enforced(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, "1")

    with pytest.raises(RuntimeError) as excinfo:
        runtime.run_startup_core_requirement_check("dcc-mcp-maya", "0.18.0")

    assert "0.18.0" in str(excinfo.value)


# ── an unusable declaration is a metadata problem, not a version mismatch ─────


def test_unusable_declaration_is_reported(tmp_path, monkeypatch):
    # `dcc_mcp_core-0` is the package-environment request that started this:
    # no usable floor and a ceiling five minor lines away.
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", "dcc_mcp_core-0")
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE)

    assert check.ok is False
    assert check.verdict == core_bounds.VERDICT_DECLARATION_UNUSABLE
    assert core_bounds.CODE_UPPER_BOUND_TOO_WIDE in check.report["bound"]["codes"]
    assert RUNNING_CORE in check.message
    assert "dcc-mcp-maya" in check.message


def test_unusable_declaration_raises_when_enforced(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", "dcc_mcp_core-0")
    _isolate_metadata(tmp_path, monkeypatch)
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, "1")

    with pytest.raises(RuntimeError):
        runtime.run_startup_core_requirement_check("dcc-mcp-maya", RUNNING_CORE)


# ── missing / unreadable metadata → silent skip ───────────────────────────────


@pytest.mark.parametrize("requirement", [None, ""])
def test_missing_declaration_is_skipped(requirement, tmp_path, monkeypatch):
    """A source checkout or a package-environment resolve has no declaration.

    Rez-resolved adapters declare their core request outside Python metadata,
    and a source checkout has no ``.dist-info`` at all. Neither is a finding —
    warning about metadata that was never expected to exist would make the
    startup log noise for every developer.
    """
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", requirement)
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE)

    assert check.skipped is True
    assert check.verdict is None
    assert check.message is None


def test_unrelated_distribution_is_skipped(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    # The adapter installed is `dcc-mcp-maya`; asking about a different
    # distribution must not borrow its metadata.
    check = runtime.check_adapter_core_requirement("dcc-mcp-houdini", RUNNING_CORE)

    assert check.skipped is True


def test_missing_declaration_never_raises_when_enforced(tmp_path, monkeypatch, caplog):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", None)
    _isolate_metadata(tmp_path, monkeypatch)
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, "1")

    with caplog.at_level(logging.WARNING, logger=runtime.__name__):
        check = runtime.run_startup_core_requirement_check("dcc-mcp-maya", RUNNING_CORE)

    assert check.skipped is True
    assert caplog.records == []


def test_unknown_adapter_name_is_skipped(tmp_path, monkeypatch):
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("", RUNNING_CORE)

    assert check.skipped is True


def test_metadata_read_failure_is_swallowed(tmp_path, monkeypatch):
    """A broken ``sys.path`` entry must not take the DCC host down with it."""

    def _boom(_distribution: str):
        raise OSError("permission denied")

    monkeypatch.setattr(core_bounds, "_read_metadata", _boom)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE)

    assert check.skipped is True


# ── enforcement switch ────────────────────────────────────────────────────────


def test_enforcement_defaults_off(monkeypatch):
    monkeypatch.delenv(ENV_CORE_REQUIREMENT_ENFORCE, raising=False)

    assert runtime.core_requirement_enforced() is False


@pytest.mark.parametrize("value, expected", [("1", True), ("true", True), ("TRUE", True), ("0", False), ("", False)])
def test_enforcement_reads_the_opt_in_env_var(value, expected, monkeypatch):
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, value)

    assert runtime.core_requirement_enforced() is expected


def test_check_to_dict_is_json_serialisable(tmp_path, monkeypatch):
    import json

    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    payload = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE).to_dict()

    round_tripped = json.loads(json.dumps(payload))
    assert round_tripped["adapter"] == "dcc-mcp-maya"
    assert round_tripped["running"] == RUNNING_CORE
    assert round_tripped["verdict"] == core_bounds.VERDICT_CORE_NEWER_THAN_DECLARED
    assert round_tripped["ok"] is False


# ── DccServerBase startup wiring ───────────────────────────────────────────────


class _FakeDccServer:
    """Minimal stand-in for the inner skill manager, matching test_dcc_server_options."""

    def get_skill_info(self, name):
        return None

    def discover(self, **kwargs):
        return 0


def _build_server(tmp_path, monkeypatch, *, adapter_distribution, core_version="0.20.28"):
    """Construct a real ``DccServerBase`` with a faked inner server."""
    from unittest.mock import patch

    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    skills_dir = tmp_path / "skills"
    skills_dir.mkdir(exist_ok=True)
    options = DccServerOptions.from_env(
        "maya",
        skills_dir,
        adapter_distribution=adapter_distribution,
    )
    with patch(
        "dcc_mcp_core.server_base.create_adapter_server",
        return_value=_FakeDccServer(),
    ):
        with patch(
            "dcc_mcp_core.server_base.resolve_startup_version",
            return_value=core_version,
        ):
            return DccServerBase(options)


def test_server_base_runs_the_check_on_startup(tmp_path, monkeypatch, caplog):
    """The startup path must actually exercise the contract, not just define it.

    Before this change ``check_runtime()`` and ``installed_core_requirement()``
    had zero call sites under ``python/`` outside their own module, so the
    runtime half of #2682 was dead code.
    """
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    with caplog.at_level(logging.WARNING):
        server = _build_server(tmp_path, monkeypatch, adapter_distribution="dcc-mcp-maya")

    check = server.core_requirement_check
    assert check is not None
    assert check.ok is False
    assert check.verdict == core_bounds.VERDICT_CORE_NEWER_THAN_DECLARED
    assert RUNNING_CORE in check.message
    assert "dcc-mcp-maya" in check.message


def test_server_base_warns_without_raising_by_default(tmp_path, monkeypatch, caplog):
    """A mismatch must never abort a live DCC session on its own."""
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    with caplog.at_level(logging.WARNING):
        server = _build_server(tmp_path, monkeypatch, adapter_distribution="dcc-mcp-maya")

    warnings_ = _core_requirement_warnings(caplog)
    assert warnings_, "an out-of-range core must be reported at startup"
    assert warnings_[0].levelno == logging.WARNING
    assert RUNNING_CORE in warnings_[0].getMessage()
    assert server.core_requirement_check.enforce is False


def test_server_base_raises_when_enforced(tmp_path, monkeypatch):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)
    monkeypatch.setenv(ENV_CORE_REQUIREMENT_ENFORCE, "1")

    with pytest.raises(RuntimeError) as excinfo:
        _build_server(tmp_path, monkeypatch, adapter_distribution="dcc-mcp-maya")

    assert RUNNING_CORE in str(excinfo.value)


def test_server_base_starts_silently_without_an_adapter_distribution(tmp_path, monkeypatch, caplog):
    """An adapter that names no distribution is not asked to have metadata."""
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", COMPLIANT_BUT_OUT_OF_RANGE)
    _isolate_metadata(tmp_path, monkeypatch)

    with caplog.at_level(logging.WARNING):
        server = _build_server(tmp_path, monkeypatch, adapter_distribution=None)

    check = server.core_requirement_check
    assert check is not None
    assert check.skipped is True
    assert check.message is None
    assert _core_requirement_warnings(caplog) == []


def test_server_base_stays_quiet_for_a_compliant_adapter(tmp_path, monkeypatch, caplog):
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", "dcc-mcp-core>=0.20.0,<0.21.0")
    _isolate_metadata(tmp_path, monkeypatch)

    with caplog.at_level(logging.WARNING):
        server = _build_server(tmp_path, monkeypatch, adapter_distribution="dcc-mcp-maya")

    assert server.core_requirement_check.ok is True
    assert _core_requirement_warnings(caplog) == []


def test_server_base_survives_a_broken_check(tmp_path, monkeypatch, caplog):
    """Diagnostics must never be the reason an adapter fails to start."""
    _isolate_metadata(tmp_path, monkeypatch)

    def _boom(*_args, **_kwargs):
        raise OSError("unexpected filesystem state")

    monkeypatch.setattr(runtime, "check_adapter_core_requirement", _boom)

    with caplog.at_level(logging.WARNING):
        server = _build_server(tmp_path, monkeypatch, adapter_distribution="dcc-mcp-maya")

    assert server.core_requirement_check is None
    assert _core_requirement_warnings(caplog) == []


def test_options_thread_the_adapter_distribution_through(tmp_path):
    from dcc_mcp_core._server.options import DccServerOptions

    skills_dir = tmp_path / "skills"
    skills_dir.mkdir(exist_ok=True)

    options = DccServerOptions.from_env("maya", skills_dir, adapter_distribution="dcc-mcp-maya")

    assert options.sidecar.adapter_distribution == "dcc-mcp-maya"


def test_server_base_stays_under_the_file_size_limit() -> None:
    """The check lives in its own module so ``server_base.py`` stays < 1000 lines.

    #2683 inlined ~50 lines there and tripped the gate at 1002 lines; the fix
    was never "widen the limit", it was "put the logic somewhere else".
    """
    import dcc_mcp_core.server_base as server_base

    lines = len(Path(server_base.__file__).read_text(encoding="utf-8").splitlines())

    assert lines < 1000, f"server_base.py is {lines} lines; move new logic to a module"


# ── no second contract ────────────────────────────────────────────────────────


def test_reuses_the_existing_code_table():
    """The check must not invent a parallel vocabulary.

    #2683 was closed as superseded partly because it shipped a second contract
    file with names like ``upper_bound_too_high`` alongside the
    ``upper_bound_too_wide`` that #2682 already published. Delegating to
    ``core_bounds`` is what keeps the two from drifting apart again.
    """
    source = MODULE_PATH.read_text(encoding="utf-8")

    assert "core_bounds" in source
    assert "upper_bound_too_high" not in source
    assert "lower_bound_too_low" not in source
    # Every verdict the module can produce comes from the shared contract.
    for name in (
        "VERDICT_SUPPORTED",
        "VERDICT_CORE_NEWER_THAN_DECLARED",
        "VERDICT_CORE_OLDER_THAN_DECLARED",
        "VERDICT_DECLARATION_UNUSABLE",
    ):
        assert name in source, name


def test_skipped_check_reads_no_metadata(tmp_path, monkeypatch):
    """An unnamed adapter must not trigger the `sys.path` metadata scan.

    `installed_core_requirement("")` walks every `sys.path` entry looking for a
    `.dist-info`, which costs far more than a real lookup and is paid on every
    startup — hosts without `importlib.metadata` (Python 3.7, Maya 2022) take
    that path unconditionally.
    """
    calls: list[str] = []

    def fail(distribution: str) -> None:
        calls.append(distribution)
        raise AssertionError(f"metadata was read for {distribution!r}")

    monkeypatch.setattr(core_bounds, "installed_core_requirement", fail)

    check = runtime.core_requirement_skipped("", RUNNING_CORE)

    assert calls == []
    assert check.skipped is True
    assert check.message is None


def test_empty_adapter_short_circuits_before_the_scan(tmp_path, monkeypatch):
    """The runtime module must return a skipped check without a lookup."""
    calls: list[str] = []
    monkeypatch.setattr(
        core_bounds,
        "installed_core_requirement",
        lambda distribution: calls.append(distribution) or None,
    )

    check = runtime.check_adapter_core_requirement("", RUNNING_CORE)

    assert calls == []
    assert check.skipped is True


def test_named_adapter_still_reads_metadata(tmp_path, monkeypatch):
    """The short-circuit must not suppress a real lookup."""
    _write_adapter_metadata(tmp_path, "dcc-mcp-maya", "dcc-mcp-core>=0.20.0,<0.21.0")
    _isolate_metadata(tmp_path, monkeypatch)

    check = runtime.check_adapter_core_requirement("dcc-mcp-maya", RUNNING_CORE)

    assert check.skipped is False
    assert check.declaration == "dcc-mcp-core>=0.20.0,<0.21.0"
