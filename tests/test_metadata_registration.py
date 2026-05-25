"""Tests for skill metadata-driven extension registration helpers."""

from __future__ import annotations

import logging
import sys
import types
from typing import List
from typing import Optional
from typing import Tuple

import pytest

import dcc_mcp_core
from dcc_mcp_core.metadata_registration import MetadataExtensionRegistration
from dcc_mcp_core.metadata_registration import imported_metadata_extension
from dcc_mcp_core.metadata_registration import metadata_extension
from dcc_mcp_core.metadata_registration import register_metadata_driven_tools


class _Registry:
    def __init__(self) -> None:
        self.names: List[str] = []

    def register(self, **kwargs: object) -> None:
        self.names.append(str(kwargs["name"]))


class _Server:
    def __init__(self) -> None:
        self.registry = _Registry()
        self.handlers: List[str] = []
        self.calls: List[Tuple[str, List[object], str]] = []

    def register_handler(self, name: str, _handler: object) -> None:
        self.handlers.append(name)


def test_register_metadata_driven_tools_scans_once_and_runs_callbacks() -> None:
    server = _Server()
    skill = object()
    scan_calls: List[Tuple[Optional[List[str]], str]] = []

    def scan(*, extra_paths: Optional[List[str]], dcc_name: str):
        scan_calls.append((extra_paths, dcc_name))
        return [skill], ["bad-skill"]

    def register_alpha(target: _Server, *, skills: List[object], dcc_name: str) -> None:
        target.calls.append(("alpha", skills, dcc_name))

    report = register_metadata_driven_tools(
        server,
        dcc_name="maya",
        extra_paths=["/studio/skills"],
        scan=scan,
        registrations=[metadata_extension("alpha", register_alpha)],
        phase="adapter-startup",
    )

    assert scan_calls == [(["/studio/skills"], "maya")]
    assert server.calls == [("alpha", [skill], "maya")]
    assert report.phase == "adapter-startup"
    assert report.skills == [skill]
    assert report.skipped == ["bad-skill"]
    assert report.registered_count == 1
    assert report.ok is True


def test_register_metadata_driven_tools_uses_supplied_skills_without_scanning() -> None:
    server = _Server()
    skill = object()

    def fail_scan(**_kwargs: object):
        raise AssertionError("scan should not be called")

    def register_beta(target: _Server, *, skills: List[object], dcc_name: str) -> None:
        target.calls.append(("beta", skills, dcc_name))

    report = register_metadata_driven_tools(
        server,
        skills=[skill],
        skipped=["ignored-skill"],
        dcc_name="photoshop",
        scan=fail_scan,
        registrations=[("beta", register_beta)],
    )

    assert server.calls == [("beta", [skill], "photoshop")]
    assert report.skipped == ["ignored-skill"]
    assert report.scan_error is None


def test_register_metadata_driven_tools_records_import_and_runtime_failures(caplog: pytest.LogCaptureFixture) -> None:
    caplog.set_level(logging.WARNING)
    server = _Server()

    def explode(_server: _Server, *, skills: List[object], dcc_name: str) -> None:
        raise RuntimeError(f"boom {dcc_name} {len(skills)}")

    def ok(target: _Server, *, skills: List[object], dcc_name: str) -> None:
        target.calls.append(("ok", skills, dcc_name))

    report = register_metadata_driven_tools(
        server,
        skills=[],
        dcc_name="zbrush",
        registrations=[
            imported_metadata_extension("missing", "dcc_mcp_core.nope", "register"),
            metadata_extension("explode", explode),
            metadata_extension("ok", ok),
        ],
    )

    assert [(item.name, item.status) for item in report.extensions] == [
        ("missing", "skipped"),
        ("explode", "failed"),
        ("ok", "registered"),
    ]
    assert report.ok is False
    assert server.calls == [("ok", [], "zbrush")]
    assert "missing import failed" in caplog.text
    assert "explode registration failed" in caplog.text


def test_default_metadata_extensions_register_builtin_tools(monkeypatch: pytest.MonkeyPatch) -> None:
    recipes = types.ModuleType("dcc_mcp_core.recipes")
    refs = types.ModuleType("dcc_mcp_core.skill_reference_docs")

    def register_recipes_tools(target: _Server, *, skills: List[object], dcc_name: str) -> None:
        target.calls.append(("recipes", skills, dcc_name))
        target.register_handler("recipes__list", object())
        target.register_handler("recipes__get", object())
        target.register_handler("recipes__search", object())
        target.register_handler("recipes__validate", object())
        target.register_handler("recipes__apply", object())

    def register_skill_reference_docs_tools(target: _Server, *, skills: List[object], dcc_name: str) -> None:
        target.calls.append(("skill-reference-docs", skills, dcc_name))
        target.register_handler("skill_refs__list", object())
        target.register_handler("skill_refs__read", object())

    recipes.register_recipes_tools = register_recipes_tools  # type: ignore[attr-defined]
    refs.register_skill_reference_docs_tools = register_skill_reference_docs_tools  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "dcc_mcp_core.recipes", recipes)
    monkeypatch.setitem(sys.modules, "dcc_mcp_core.skill_reference_docs", refs)

    server = _Server()

    report = register_metadata_driven_tools(server, skills=[], dcc_name="houdini")

    assert [(item.name, item.status) for item in report.extensions] == [
        ("recipes", "registered"),
        ("skill-reference-docs", "registered"),
    ]
    assert {
        "recipes__list",
        "recipes__get",
        "recipes__search",
        "recipes__validate",
        "recipes__apply",
        "skill_refs__list",
        "skill_refs__read",
    }.issubset(set(server.handlers))


def test_metadata_registration_public_exports_are_lazy() -> None:
    assert dcc_mcp_core.register_metadata_driven_tools is register_metadata_driven_tools
    assert dcc_mcp_core.metadata_extension is metadata_extension
    assert isinstance(
        imported_metadata_extension("x", "dcc_mcp_core.metadata_registration", "metadata_extension"),
        MetadataExtensionRegistration,
    )
