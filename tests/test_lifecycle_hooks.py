"""Tests for the typed lifecycle-hook framework (issue #1337)."""

from __future__ import annotations

import logging

import pytest

from dcc_mcp_core import HookContext
from dcc_mcp_core import HookDeny
from dcc_mcp_core import HookEvent
from dcc_mcp_core import LifecycleHooks


class TestHookEvent:
    def test_policy_events_only_contain_before_events(self) -> None:
        policy = HookEvent.policy_events()
        assert HookEvent.BEFORE_SKILL_LOAD in policy
        assert HookEvent.BEFORE_TOOL_CALL in policy
        assert HookEvent.BEFORE_SEARCH in policy
        # after_* and on_session_* are observational
        assert HookEvent.AFTER_SEARCH not in policy
        assert HookEvent.AFTER_SKILL_LOAD not in policy
        assert HookEvent.AFTER_TOOL_CALL not in policy
        assert HookEvent.SESSION_START not in policy
        assert HookEvent.SESSION_END not in policy

    def test_event_values_are_snake_case_strings(self) -> None:
        for event in HookEvent:
            assert event.value == event.value.lower()
            assert " " not in event.value


class TestHookDeny:
    def test_deny_carries_reason_and_optional_hint(self) -> None:
        deny = HookDeny("blocked by policy", hint="load typed skill foo first")
        assert deny.reason == "blocked by policy"
        assert deny.hint == "load typed skill foo first"
        assert "blocked by policy" in repr(deny)

    def test_deny_without_hint(self) -> None:
        deny = HookDeny("nope")
        assert deny.hint is None


class TestLifecycleHooks:
    def test_on_rejects_non_callable(self) -> None:
        hooks = LifecycleHooks()
        with pytest.raises(TypeError):
            hooks.on(HookEvent.BEFORE_SEARCH, "not callable")  # type: ignore[arg-type]

    def test_handlers_fire_in_registration_order(self) -> None:
        hooks = LifecycleHooks()
        order: list[int] = []
        hooks.on(HookEvent.AFTER_SEARCH, lambda ctx: order.append(1))
        hooks.on(HookEvent.AFTER_SEARCH, lambda ctx: order.append(2))
        hooks.on(HookEvent.AFTER_SEARCH, lambda ctx: order.append(3))

        hooks.dispatch(HookContext(event=HookEvent.AFTER_SEARCH, dcc_name="any"))

        assert order == [1, 2, 3]

    def test_off_removes_handler(self) -> None:
        hooks = LifecycleHooks()
        seen: list[str] = []

        def handler(ctx: HookContext) -> None:
            seen.append("called")

        hooks.on(HookEvent.AFTER_SKILL_LOAD, handler)
        assert hooks.off(HookEvent.AFTER_SKILL_LOAD, handler) is True
        hooks.dispatch(HookContext(event=HookEvent.AFTER_SKILL_LOAD, dcc_name="any"))

        assert seen == []
        # Removing again returns False
        assert hooks.off(HookEvent.AFTER_SKILL_LOAD, handler) is False

    def test_non_policy_handler_exception_is_swallowed(self, caplog) -> None:
        hooks = LifecycleHooks()
        seen: list[str] = []

        def broken(ctx: HookContext) -> None:
            raise RuntimeError("boom")

        def good(ctx: HookContext) -> None:
            seen.append("ok")

        hooks.on(HookEvent.AFTER_TOOL_CALL, broken)
        hooks.on(HookEvent.AFTER_TOOL_CALL, good)

        with caplog.at_level(logging.WARNING, logger="dcc_mcp_core.lifecycle_hooks"):
            hooks.dispatch(HookContext(event=HookEvent.AFTER_TOOL_CALL, dcc_name="any"))

        assert seen == ["ok"]
        assert any("after_tool_call" in record.message for record in caplog.records)

    def test_policy_deny_propagates(self) -> None:
        hooks = LifecycleHooks()
        hooks.on(
            HookEvent.BEFORE_SKILL_LOAD,
            lambda ctx: (_ for _ in ()).throw(HookDeny("policy says no")),
        )

        with pytest.raises(HookDeny) as info:
            hooks.dispatch(
                HookContext(
                    event=HookEvent.BEFORE_SKILL_LOAD,
                    dcc_name="maya",
                    payload={"skill_name": "foo"},
                )
            )
        assert info.value.reason == "policy says no"

    def test_non_policy_deny_is_logged_and_swallowed(self, caplog) -> None:
        hooks = LifecycleHooks()
        hooks.on(
            HookEvent.AFTER_SEARCH,
            lambda ctx: (_ for _ in ()).throw(HookDeny("oops")),
        )

        with caplog.at_level(logging.WARNING, logger="dcc_mcp_core.lifecycle_hooks"):
            hooks.dispatch(HookContext(event=HookEvent.AFTER_SEARCH, dcc_name="any"))

        assert any("non-policy event after_search" in r.message for r in caplog.records)

    def test_handlers_snapshot_is_immutable_view(self) -> None:
        hooks = LifecycleHooks()
        h = lambda ctx: None  # noqa: E731
        hooks.on(HookEvent.SESSION_START, h)

        snapshot = hooks.handlers(HookEvent.SESSION_START)
        assert snapshot == (h,)
        assert isinstance(snapshot, tuple)
        # Mutating the snapshot does not change registry
        hooks.on(HookEvent.SESSION_START, lambda ctx: None)
        assert len(hooks.handlers(HookEvent.SESSION_START)) == 2
        # original snapshot stayed the same
        assert len(snapshot) == 1

    def test_dispatch_unregistered_event_is_noop(self) -> None:
        hooks = LifecycleHooks()
        # No handlers — must not raise.
        hooks.dispatch(HookContext(event=HookEvent.SESSION_END, dcc_name="any"))

    def test_context_payload_defaults_to_empty_dict(self) -> None:
        ctx = HookContext(event=HookEvent.BEFORE_SEARCH, dcc_name="blender")
        assert ctx.payload == {}
        assert ctx.session_id is None


class _FakeInnerServer:
    """Minimal stand-in for the Rust skill server."""

    def __init__(self) -> None:
        self.transform = None
        self.after_hook = None

    def set_skill_load_transform(self, transform):
        self.transform = transform

    def clear_skill_load_transform(self):
        self.transform = None

    def set_after_load_skill_hook(self, hook):
        self.after_hook = hook

    def clear_after_load_skill_hook(self):
        self.after_hook = None


class _FakeSkill:
    def __init__(self, name: str) -> None:
        self.name = name


class TestDccServerBaseBridge:
    """``DccServerBase.register_lifecycle_hooks`` must bridge load events."""

    def _make_server(self):
        from dcc_mcp_core._testing import make_test_server

        return make_test_server(server=_FakeInnerServer(), dcc_name="bridge-dcc")

    def test_register_bridges_before_and_after_load(self) -> None:
        server = self._make_server()
        hooks = LifecycleHooks()
        seen: list[tuple[str, dict]] = []

        hooks.on(HookEvent.BEFORE_SKILL_LOAD, lambda ctx: seen.append(("before", ctx.payload)))
        hooks.on(HookEvent.AFTER_SKILL_LOAD, lambda ctx: seen.append(("after", ctx.payload)))

        returned = server.register_lifecycle_hooks(hooks)
        assert returned is hooks
        assert server.lifecycle_hooks() is hooks

        # Inner server received bridge callables
        skill = _FakeSkill("usd-import")
        server._server.transform(skill)
        server._server.after_hook(skill, ["import", "validate"])

        assert seen[0] == ("before", {"skill_name": "usd-import"})
        assert seen[1] == ("after", {"skill_name": "usd-import", "registered_actions": ["import", "validate"]})

    def test_register_propagates_hook_deny_from_before_skill_load(self) -> None:
        server = self._make_server()
        hooks = LifecycleHooks()
        hooks.on(
            HookEvent.BEFORE_SKILL_LOAD,
            lambda ctx: (_ for _ in ()).throw(HookDeny("not-allowed", hint="load typed first")),
        )

        server.register_lifecycle_hooks(hooks)
        with pytest.raises(HookDeny) as info:
            server._server.transform(_FakeSkill("blocked"))
        assert info.value.reason == "not-allowed"
        assert info.value.hint == "load typed first"
