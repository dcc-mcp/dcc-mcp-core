"""Regression tests for :mod:`dcc_mcp_core.skills.builtin`.

Guards the bug where ``register_all_builtin_skills`` called
``register_recipes_tools`` without the now-required keyword-only ``skills``
argument. That raised ``TypeError`` mid-way through registration, aborting the
later steps (Qt UI inspector + script materialization) and surfacing only as a
swallowed warning at the ``DccServerBase`` level.
"""

from __future__ import annotations

from unittest.mock import MagicMock

from dcc_mcp_core.skills import builtin


def _make_server() -> MagicMock:
    server = MagicMock()
    server.registry = MagicMock()
    handlers: dict = {}
    server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
    return server


def test_register_all_builtin_skills_runs_every_step(monkeypatch):
    """All six built-in steps must run; recipes must receive ``skills``."""
    calls: list[str] = []
    recipes_kwargs: dict = {}

    def _recorder(name):
        def _inner(*_args, **_kwargs):
            calls.append(name)

        return _inner

    def _recipes(*_args, **kwargs):
        calls.append("recipes")
        recipes_kwargs.update(kwargs)

    monkeypatch.setattr(builtin, "register_diagnostic_mcp_tools", _recorder("diagnostics"))
    monkeypatch.setattr(builtin, "register_introspect_tools", _recorder("introspect"))
    monkeypatch.setattr(builtin, "register_feedback_tool", _recorder("feedback"))
    monkeypatch.setattr(builtin, "register_recipes_tools", _recipes)
    monkeypatch.setattr(builtin, "register_qt_ui_inspector", _recorder("qt"))
    monkeypatch.setattr(builtin, "register_script_materialization_tools", _recorder("materialize"))

    builtin.register_all_builtin_skills(_make_server(), dcc_name="maya")

    # Every step, including the two that previously got skipped after the
    # TypeError, must have executed in order.
    assert calls == ["diagnostics", "introspect", "feedback", "recipes", "qt", "materialize"]
    # recipes must be invoked with an (empty) skills list, never omitted.
    assert "skills" in recipes_kwargs
    assert recipes_kwargs["skills"] == []


def test_register_all_builtin_skills_forwards_skills(monkeypatch):
    """When the caller supplies skills they reach ``register_recipes_tools``."""
    seen: dict = {}

    monkeypatch.setattr(builtin, "register_diagnostic_mcp_tools", lambda *a, **k: None)
    monkeypatch.setattr(builtin, "register_introspect_tools", lambda *a, **k: None)
    monkeypatch.setattr(builtin, "register_feedback_tool", lambda *a, **k: None)
    monkeypatch.setattr(builtin, "register_qt_ui_inspector", lambda *a, **k: None)
    monkeypatch.setattr(builtin, "register_script_materialization_tools", lambda *a, **k: None)
    monkeypatch.setattr(
        builtin,
        "register_recipes_tools",
        lambda *a, **k: seen.update(k),
    )

    sentinel = [MagicMock(name="skill-a"), MagicMock(name="skill-b")]
    builtin.register_all_builtin_skills(_make_server(), dcc_name="blender", skills=sentinel)

    assert seen["skills"] is sentinel


def test_register_all_builtin_skills_with_real_recipes_does_not_raise():
    """End-to-end: the real recipes registration must not raise on empty skills."""
    server = _make_server()
    # Should complete without TypeError even though no skills are scanned yet.
    builtin.register_all_builtin_skills(server, dcc_name="houdini")
    registered = [c.kwargs.get("name") for c in server.registry.register.call_args_list]
    assert "recipes__list" in registered
    assert "recipes__get" in registered
