"""Edge-case and boundary tests for PyO3 Python bindings.

Exercises error paths, type coercion boundaries, concurrent access,
and unusual inputs that stress the Rust↔Python bridge.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path
import threading

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── ActionResultModel edge cases ──


class TestActionResultModelEdge:
    def test_empty_context(self) -> None:
        r = dcc_mcp_core.ActionResultModel(message="ok", context={})
        assert r.context == {}

    def test_deeply_nested_context(self) -> None:
        nested = {"a": {"b": {"c": {"d": [1, 2, {"e": True}]}}}}
        r = dcc_mcp_core.ActionResultModel(message="deep", context=nested)
        assert r.context["a"]["b"]["c"]["d"][2]["e"] is True

    def test_large_message(self) -> None:
        msg = "x" * 100_000
        r = dcc_mcp_core.ActionResultModel(message=msg)
        assert len(r.message) == 100_000

    def test_unicode_message(self) -> None:
        msg = "日本語テスト 🎨 émojis «»"
        r = dcc_mcp_core.ActionResultModel(message=msg)
        assert r.message == msg

    def test_context_with_special_keys(self) -> None:
        ctx = {"": "empty_key", "key with spaces": 1, "key\nwith\nnewlines": 2}
        r = dcc_mcp_core.ActionResultModel(message="ok", context=ctx)
        assert r.context[""] == "empty_key"
        assert r.context["key with spaces"] == 1

    def test_with_error_preserves_original(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(message="ok", context={"k": "v"})
        r2 = r1.with_error("bad")
        assert r1.success is True
        assert r2.success is False
        assert r1.context.get("k") == "v"

    def test_with_context_merges(self) -> None:
        r = dcc_mcp_core.ActionResultModel(message="ok")
        r2 = r.with_context(a=1, b="two", c=[3])
        assert r2.context["a"] == 1
        assert r2.context["b"] == "two"
        assert r2.context["c"] == [3]

    def test_to_dict_roundtrip(self) -> None:
        r = dcc_mcp_core.ActionResultModel(
            success=False,
            message="err",
            prompt="fix it",
            error="oops",
            context={"x": 42},
        )
        d = r.to_dict()
        assert d["success"] is False
        assert d["message"] == "err"
        assert d["prompt"] == "fix it"
        assert d["error"] == "oops"
        assert d["context"]["x"] == 42


# ── Factory function edge cases ──


class TestFactoryEdge:
    def test_success_result_empty_message(self) -> None:
        r = dcc_mcp_core.success_result("")
        assert r.success is True
        assert r.message == ""

    def test_error_result_none_solutions(self) -> None:
        r = dcc_mcp_core.error_result("fail", "err", possible_solutions=None)
        assert r.success is False

    def test_error_result_empty_solutions(self) -> None:
        r = dcc_mcp_core.error_result("fail", "err", possible_solutions=[])
        assert r.context.get("possible_solutions") == []

    def test_from_exception_empty_string(self) -> None:
        r = dcc_mcp_core.from_exception("")
        assert r.success is False

    def test_from_exception_multiline(self) -> None:
        exc = "Traceback (most recent call last):\n  File ...\nValueError: bad"
        r = dcc_mcp_core.from_exception(exc)
        assert r.success is False
        assert "bad" in (r.error or "")

    def test_validate_action_result_none(self) -> None:
        r = dcc_mcp_core.validate_action_result(None)
        assert isinstance(r, dcc_mcp_core.ActionResultModel)

    def test_validate_action_result_bool(self) -> None:
        r = dcc_mcp_core.validate_action_result(True)
        assert r.success is True

    def test_validate_action_result_float(self) -> None:
        r = dcc_mcp_core.validate_action_result(3.14)
        assert r.success is True

    def test_validate_action_result_list(self) -> None:
        r = dcc_mcp_core.validate_action_result([1, 2, 3])
        assert r.success is True

    def test_validate_action_result_dict_missing_fields(self) -> None:
        r = dcc_mcp_core.validate_action_result({"random_key": "value"})
        assert isinstance(r, dcc_mcp_core.ActionResultModel)

    def test_validate_action_result_empty_dict(self) -> None:
        r = dcc_mcp_core.validate_action_result({})
        assert isinstance(r, dcc_mcp_core.ActionResultModel)


# ── ActionRegistry edge cases ──


class TestActionRegistryEdge:
    def test_register_empty_name(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="")
        meta = reg.get_action("")
        assert meta is not None

    def test_register_unicode_name(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="アクション_テスト", dcc="maya")
        meta = reg.get_action("アクション_テスト")
        assert meta is not None
        assert meta["dcc"] == "maya"

    def test_register_special_chars_description(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        desc = 'Line1\nLine2\t"quoted" <html>'
        reg.register(name="special", description=desc)
        assert reg.get_action("special")["description"] == desc

    def test_register_invalid_json_schema(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a", input_schema="not valid json")
        meta = reg.get_action("a")
        assert meta is not None

    def test_register_many_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        for i in range(500):
            reg.register(name=f"action_{i}", dcc=f"dcc_{i % 10}")
        assert len(reg) == 500
        assert len(reg.get_all_dccs()) == 10

    def test_overwrite_changes_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a", dcc="maya")
        reg.register(name="a", dcc="blender")
        assert reg.get_action("a")["dcc"] == "blender"

    def test_reset_idempotent(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.reset()
        reg.reset()
        assert len(reg) == 0


# ── EventBus edge cases ──


class TestEventBusEdge:
    def test_publish_error_in_callback(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        bus.subscribe("evt", lambda: (_ for _ in ()).throw(ValueError("boom")))
        bus.subscribe("evt", lambda: results.append("ok"))
        bus.publish("evt")
        # Second callback should still fire despite first raising
        assert "ok" in results

    def test_subscribe_unsubscribe_resubscribe(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        sid = bus.subscribe("evt", lambda: results.append("a"))
        bus.unsubscribe("evt", sid)
        bus.subscribe("evt", lambda: results.append("b"))
        bus.publish("evt")
        assert results == ["b"]

    def test_many_events(self) -> None:
        bus = dcc_mcp_core.EventBus()
        counts = {}
        for i in range(100):
            name = f"event_{i}"
            counts[name] = 0
            bus.subscribe(name, lambda n=name: counts.__setitem__(n, counts[n] + 1))
        for name in counts:
            bus.publish(name)
        assert all(v == 1 for v in counts.values())

    def test_concurrent_subscribe_publish(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        barrier = threading.Barrier(3)

        def subscriber() -> None:
            barrier.wait()
            for i in range(50):
                bus.subscribe(f"evt_{i}", lambda: results.append(1))

        def publisher() -> None:
            barrier.wait()
            for i in range(50):
                bus.publish(f"evt_{i}")

        t1 = threading.Thread(target=subscriber)
        t2 = threading.Thread(target=subscriber)
        t3 = threading.Thread(target=publisher)
        t1.start()
        t2.start()
        t3.start()
        t1.join(timeout=10)
        t2.join(timeout=10)
        t3.join(timeout=10)
        # No deadlock or crash is the success criterion


# ── Protocol types edge cases ──


class TestProtocolEdge:
    def test_tool_definition_long_schema(self) -> None:
        schema = (
            '{"type": "object", "properties": {' + ", ".join(f'"p{i}": {{"type": "string"}}' for i in range(200)) + "}}"
        )
        td = dcc_mcp_core.ToolDefinition(name="big", description="big schema", input_schema=schema)
        assert "p199" in td.input_schema

    def test_tool_annotations_all_none(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations()
        assert ann.title is None
        assert ann.read_only_hint is None
        assert ann.destructive_hint is None
        assert ann.idempotent_hint is None
        assert ann.open_world_hint is None

    def test_resource_definition_empty_uri(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="", name="empty", description="empty uri")
        assert rd.uri == ""

    def test_resource_template_special_chars(self) -> None:
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="scene://objects/{name}/子ノード",
            name="unicode",
            description="unicode template",
        )
        assert "子ノード" in rtd.uri_template

    def test_prompt_argument_required_false(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="opt", description="optional")
        assert pa.required is False

    def test_prompt_definition_repr(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="test_prompt", description="A test")
        assert "test_prompt" in repr(pd)


# ── SkillScanner edge cases ──


class TestSkillScannerEdge:
    def test_scan_nonexistent_path(self) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        result = scanner.scan(extra_paths=["/this/path/does/not/exist"])
        assert result == []

    def test_scan_empty_extra_paths(self) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        result = scanner.scan(extra_paths=[])
        # Should return skills from default paths (if any) without error
        assert isinstance(result, list)

    def test_scan_duplicate_paths(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(str(tmp_path), "dup-skill")
        scanner = dcc_mcp_core.SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path), str(tmp_path)])
        # Should deduplicate
        names = [Path(d).name for d in result]
        assert names.count("dup-skill") == 1

    def test_parse_skill_md_empty_frontmatter(self, tmp_path: Path) -> None:
        (tmp_path / "SKILL.md").write_text("---\n---\nBody only", encoding="utf-8")
        result = dcc_mcp_core.parse_skill_md(str(tmp_path))
        # Empty frontmatter should return None or a default metadata
        # depending on implementation
        assert result is None or isinstance(result, dcc_mcp_core.SkillMetadata)

    def test_parse_skill_md_unicode_content(self, tmp_path: Path) -> None:
        content = "---\nname: 日本語スキル\ndcc: blender\ntags:\n  - 3Dモデル\n---\n# 説明\n"
        (tmp_path / "SKILL.md").write_text(content, encoding="utf-8")
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path))
        assert meta is not None
        assert meta.name == "日本語スキル"
        assert meta.dcc == "blender"
        assert "3Dモデル" in meta.tags

    def test_skill_metadata_empty_lists(self) -> None:
        sm = dcc_mcp_core.SkillMetadata(name="empty", tags=[], scripts=[], tools=[])
        assert sm.tags == []
        assert sm.scripts == []
        assert sm.tools == []


# ── Type wrapper edge cases ──


class TestTypeWrapperEdge:
    def test_float_wrapper_precision(self) -> None:
        w = dcc_mcp_core.FloatWrapper(0.1 + 0.2)
        assert abs(float(w) - 0.3) < 1e-10

    def test_float_wrapper_zero(self) -> None:
        w = dcc_mcp_core.FloatWrapper(0.0)
        assert float(w) == 0.0

    def test_float_wrapper_negative(self) -> None:
        w = dcc_mcp_core.FloatWrapper(-999.999)
        assert float(w) == -999.999

    def test_int_wrapper_zero(self) -> None:
        w = dcc_mcp_core.IntWrapper(0)
        assert int(w) == 0

    def test_int_wrapper_large(self) -> None:
        w = dcc_mcp_core.IntWrapper(2**31 - 1)
        assert int(w) == 2**31 - 1

    def test_string_wrapper_unicode(self) -> None:
        text = "🎮 DCC ツール 工具"
        w = dcc_mcp_core.StringWrapper(text)
        assert str(w) == text

    def test_string_wrapper_newlines(self) -> None:
        text = "line1\nline2\rline3\r\nline4"
        w = dcc_mcp_core.StringWrapper(text)
        assert str(w) == text

    def test_wrap_unwrap_roundtrip(self) -> None:
        values = [True, False, 42, -1, 3.14, 0.0, "hello", ""]
        for v in values:
            wrapped = dcc_mcp_core.wrap_value(v)
            unwrapped = dcc_mcp_core.unwrap_value(wrapped)
            assert unwrapped == v, f"Roundtrip failed for {v!r}"

    def test_unwrap_parameters_large(self) -> None:
        params = {f"key_{i}": dcc_mcp_core.IntWrapper(i) for i in range(200)}
        result = dcc_mcp_core.unwrap_parameters(params)
        assert len(result) == 200
        assert result["key_199"] == 199


# ── Filesystem edge cases ──


class TestFilesystemEdge:
    def test_get_skills_dir_empty_dcc(self) -> None:
        path = dcc_mcp_core.get_skills_dir("")
        assert isinstance(path, str)

    def test_get_actions_dir_unicode_dcc(self) -> None:
        path = dcc_mcp_core.get_actions_dir("ブレンダー")
        assert isinstance(path, str)

    def test_get_skill_paths_from_env_multiple(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        import os

        d1 = tmp_path / "skills1"
        d2 = tmp_path / "skills2"
        d1.mkdir()
        d2.mkdir()
        sep = os.pathsep
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", f"{d1}{sep}{d2}")
        paths = dcc_mcp_core.get_skill_paths_from_env()
        assert str(d1) in paths
        assert str(d2) in paths


# ── Concurrent registry access ──


class TestConcurrentRegistry:
    def test_concurrent_register_and_read(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        errors: list[Exception] = []
        barrier = threading.Barrier(4)

        def writer(start: int) -> None:
            try:
                barrier.wait()
                for i in range(start, start + 100):
                    reg.register(name=f"action_{i}", dcc="maya")
            except Exception as e:
                errors.append(e)

        def reader() -> None:
            try:
                barrier.wait()
                for _ in range(200):
                    reg.list_actions()
                    reg.get_all_dccs()
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=writer, args=(0,)),
            threading.Thread(target=writer, args=(100,)),
            threading.Thread(target=reader),
            threading.Thread(target=reader),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent access errors: {errors}"
        assert len(reg) == 200
