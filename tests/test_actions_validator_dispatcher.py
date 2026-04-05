"""pytest tests for ActionValidator and ActionDispatcher PyO3 bindings.

These tests exercise the Python-level API without requiring a running DCC.
All tests use the mock/in-process path; no maturin compilation required at
runtime (the native module must already be installed).
"""

from __future__ import annotations

import json

import pytest

# Skip the entire module gracefully if the native extension is not compiled.
_core = pytest.importorskip("dcc_mcp_core._core", reason="native extension not compiled")


# ── Helpers ───────────────────────────────────────────────────────────────────


def _require(name: str):
    """Import a class from _core or skip the test."""
    obj = getattr(_core, name, None)
    if obj is None:
        pytest.skip(f"{name} not available in _core (old build?)")
    return obj


# ── ActionValidator tests ─────────────────────────────────────────────────────


class TestActionValidator:
    """Tests for the ActionValidator PyO3 class."""

    @pytest.fixture
    def validator_cls(self):
        return _require("ActionValidator")

    @pytest.fixture
    def registry_cls(self):
        return _require("ActionRegistry")

    # ── from_schema_json ──────────────────────────────────────────────────────

    def test_from_schema_json_valid(self, validator_cls):
        schema = json.dumps({"type": "object"})
        v = validator_cls.from_schema_json(schema)
        assert v is not None

    def test_from_schema_json_invalid_json_raises(self, validator_cls):
        with pytest.raises((ValueError, RuntimeError)):
            validator_cls.from_schema_json("{not valid json")

    # ── validate ──────────────────────────────────────────────────────────────

    def test_validate_passes_empty_schema(self, validator_cls):
        v = validator_cls.from_schema_json("{}")
        ok, errors = v.validate('{"any": "thing"}')
        assert ok
        assert errors == []

    def test_validate_passes_matching_params(self, validator_cls):
        schema = json.dumps(
            {
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number", "minimum": 0.0}},
            }
        )
        v = validator_cls.from_schema_json(schema)
        ok, errors = v.validate('{"radius": 1.5}')
        assert ok
        assert errors == []

    def test_validate_fails_missing_required(self, validator_cls):
        schema = json.dumps({"type": "object", "required": ["name"]})
        v = validator_cls.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert not ok
        assert any("name" in e for e in errors)

    def test_validate_fails_type_mismatch(self, validator_cls):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"radius": {"type": "number"}},
            }
        )
        v = validator_cls.from_schema_json(schema)
        ok, errors = v.validate('{"radius": "big"}')
        assert not ok
        assert len(errors) >= 1

    def test_validate_fails_minimum_constraint(self, validator_cls):
        schema = json.dumps({"type": "number", "minimum": 0.0})
        v = validator_cls.from_schema_json(schema)
        ok, errors = v.validate("-1.0")
        assert not ok
        assert len(errors) >= 1

    def test_validate_fails_maximum_constraint(self, validator_cls):
        schema = json.dumps({"type": "number", "maximum": 100.0})
        v = validator_cls.from_schema_json(schema)
        ok, _errors = v.validate("200.0")
        assert not ok

    def test_validate_fails_max_length(self, validator_cls):
        schema = json.dumps({"type": "string", "maxLength": 3})
        v = validator_cls.from_schema_json(schema)
        ok, _errors = v.validate('"toolong"')
        assert not ok

    def test_validate_passes_enum(self, validator_cls):
        schema = json.dumps({"enum": ["low", "medium", "high"]})
        v = validator_cls.from_schema_json(schema)
        ok, _ = v.validate('"medium"')
        assert ok

    def test_validate_fails_enum(self, validator_cls):
        schema = json.dumps({"enum": ["low", "medium", "high"]})
        v = validator_cls.from_schema_json(schema)
        ok, _ = v.validate('"extreme"')
        assert not ok

    def test_validate_invalid_params_json_raises(self, validator_cls):
        v = validator_cls.from_schema_json("{}")
        with pytest.raises((ValueError, RuntimeError)):
            v.validate("{not json}")

    def test_validate_additional_properties_false(self, validator_cls):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "additionalProperties": False,
            }
        )
        v = validator_cls.from_schema_json(schema)
        ok, errors = v.validate('{"name": "x", "extra": 1}')
        assert not ok
        assert any("extra" in e for e in errors)

    def test_validate_returns_multiple_errors(self, validator_cls):
        schema = json.dumps({"type": "object", "required": ["a", "b", "c"]})
        v = validator_cls.from_schema_json(schema)
        ok, _errors = v.validate("{}")
        assert not ok
        assert len(_errors) == 3  # missing a, b, c

    # ── from_action_registry ──────────────────────────────────────────────────

    def test_from_action_registry_found(self, validator_cls, registry_cls):
        reg = registry_cls()
        schema = json.dumps({"type": "object"})
        reg.register("my_action", input_schema=schema)
        v = validator_cls.from_action_registry(reg, "my_action")
        assert v is not None

    def test_from_action_registry_not_found_raises(self, validator_cls, registry_cls):
        reg = registry_cls()
        with pytest.raises((KeyError, RuntimeError)):
            validator_cls.from_action_registry(reg, "nonexistent")

    def test_from_action_registry_validates(self, validator_cls, registry_cls):
        reg = registry_cls()
        schema = json.dumps({"type": "object", "required": ["x"]})
        reg.register("check_x", input_schema=schema)
        v = validator_cls.from_action_registry(reg, "check_x")
        ok, _ = v.validate('{"x": 1}')
        assert ok
        ok, _errors = v.validate("{}")
        assert not ok

    # ── repr ──────────────────────────────────────────────────────────────────

    def test_repr(self, validator_cls):
        v = validator_cls.from_schema_json("{}")
        r = repr(v)
        assert "ActionValidator" in r or "Validator" in r or r  # at least non-empty


# ── ActionDispatcher tests ────────────────────────────────────────────────────


class TestActionDispatcher:
    """Tests for the ActionDispatcher PyO3 class."""

    @pytest.fixture
    def dispatcher_cls(self):
        return _require("ActionDispatcher")

    @pytest.fixture
    def registry_cls(self):
        return _require("ActionRegistry")

    @pytest.fixture
    def empty_dispatcher(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        return dispatcher_cls(reg), reg

    @pytest.fixture
    def dispatcher_with_echo(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        reg.register("echo")
        d = dispatcher_cls(reg)
        d.register_handler("echo", lambda params: params)
        return d, reg

    # ── construction ─────────────────────────────────────────────────────────

    def test_new_empty_dispatcher(self, empty_dispatcher):
        d, _ = empty_dispatcher
        assert d.handler_count() == 0

    def test_repr(self, empty_dispatcher):
        d, _ = empty_dispatcher
        r = repr(d)
        assert "ActionDispatcher" in r or "Dispatcher" in r or r

    # ── register_handler ─────────────────────────────────────────────────────

    def test_register_handler_increases_count(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("act", lambda p: p)
        assert d.handler_count() == 1

    def test_register_non_callable_raises(self, empty_dispatcher):
        d, _ = empty_dispatcher
        with pytest.raises((TypeError, RuntimeError)):
            d.register_handler("bad", "not_a_callable")  # type: ignore[arg-type]

    def test_register_replaces_handler(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("act", lambda _: "v1")
        d.register_handler("act", lambda _: "v2")
        assert d.handler_count() == 1

    # ── has_handler ──────────────────────────────────────────────────────────

    def test_has_handler_false_initially(self, empty_dispatcher):
        d, _ = empty_dispatcher
        assert not d.has_handler("missing")

    def test_has_handler_true_after_register(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("present", lambda p: p)
        assert d.has_handler("present")

    # ── remove_handler ───────────────────────────────────────────────────────

    def test_remove_handler_returns_true(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("x", lambda p: p)
        assert d.remove_handler("x") is True
        assert not d.has_handler("x")

    def test_remove_handler_returns_false_not_found(self, empty_dispatcher):
        d, _ = empty_dispatcher
        assert d.remove_handler("nonexistent") is False

    # ── handler_names ─────────────────────────────────────────────────────────

    def test_handler_names_sorted(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("z", lambda p: p)
        d.register_handler("a", lambda p: p)
        d.register_handler("m", lambda p: p)
        names = d.handler_names()
        assert names == sorted(names)
        assert set(names) == {"a", "m", "z"}

    # ── skip_empty_schema_validation ─────────────────────────────────────────

    def test_skip_empty_schema_default_true(self, empty_dispatcher):
        d, _ = empty_dispatcher
        assert d.skip_empty_schema_validation is True

    def test_skip_empty_schema_settable(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.skip_empty_schema_validation = False
        assert d.skip_empty_schema_validation is False

    # ── dispatch ─────────────────────────────────────────────────────────────

    def test_dispatch_echo(self, dispatcher_with_echo):
        d, _ = dispatcher_with_echo
        result = d.dispatch("echo", '{"msg": "hello"}')
        assert result["action"] == "echo"
        assert result["output"]["msg"] == "hello"

    def test_dispatch_handler_returns_dict(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        d = dispatcher_cls(reg)
        d.register_handler("sphere", lambda p: {"created": True, "radius": p.get("radius", 1.0)})
        result = d.dispatch("sphere", '{"radius": 5.0}')
        assert result["output"]["created"] is True
        assert result["output"]["radius"] == 5.0

    def test_dispatch_no_handler_raises_key_error(self, empty_dispatcher):
        d, _ = empty_dispatcher
        with pytest.raises((KeyError, RuntimeError)):
            d.dispatch("missing")

    def test_dispatch_with_schema_validation_passes(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        schema = json.dumps(
            {
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number", "minimum": 0.0}},
            }
        )
        reg.register("sphere", input_schema=schema)
        d = dispatcher_cls(reg)
        d.register_handler("sphere", lambda p: {"ok": True})
        result = d.dispatch("sphere", '{"radius": 2.0}')
        assert result["output"]["ok"] is True
        assert result["validation_skipped"] is False

    def test_dispatch_with_schema_validation_fails(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        schema = json.dumps({"type": "object", "required": ["radius"]})
        reg.register("sphere", input_schema=schema)
        d = dispatcher_cls(reg)
        d.register_handler("sphere", lambda _: {"ok": True})
        with pytest.raises((ValueError, RuntimeError)):
            d.dispatch("sphere", "{}")  # missing required "radius"

    def test_dispatch_empty_schema_skips_validation(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        reg.register("act")  # no schema
        d = dispatcher_cls(reg)
        d.register_handler("act", lambda _: "ok")
        result = d.dispatch("act", '{"anything": "goes"}')
        assert result["validation_skipped"] is True

    def test_dispatch_handler_raises_propagated(self, empty_dispatcher):
        d, _ = empty_dispatcher

        def bad_handler(_params):
            msg = "something went wrong"
            raise RuntimeError(msg)

        d.register_handler("bad", bad_handler)
        with pytest.raises((RuntimeError, Exception)):
            d.dispatch("bad")

    def test_dispatch_invalid_json_raises(self, dispatcher_with_echo):
        d, _ = dispatcher_with_echo
        with pytest.raises((ValueError, RuntimeError)):
            d.dispatch("echo", "{not json}")

    def test_dispatch_null_params(self, empty_dispatcher):
        d, _ = empty_dispatcher
        d.register_handler("noop", lambda _: "done")
        result = d.dispatch("noop")  # default params_json="null"
        assert result["output"] == "done"

    def test_dispatch_returns_validation_skipped_false_with_schema(self, dispatcher_cls, registry_cls):
        reg = registry_cls()
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"x": {"type": "number"}},
            }
        )
        reg.register("typed", input_schema=schema)
        d = dispatcher_cls(reg)
        d.register_handler("typed", lambda p: p)
        result = d.dispatch("typed", '{"x": 1.0}')
        # Schema is non-empty, so validation_skipped should be False
        assert result["validation_skipped"] is False
