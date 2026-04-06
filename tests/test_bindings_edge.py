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

    def test_concurrent_unregister(self) -> None:
        """Concurrent register + unregister must not crash."""
        reg = dcc_mcp_core.ActionRegistry()
        # Pre-populate
        for i in range(100):
            reg.register(name=f"action_{i}", dcc="maya")

        errors: list[Exception] = []
        barrier = threading.Barrier(2)

        def remover() -> None:
            try:
                barrier.wait()
                for i in range(100):
                    reg.unregister(f"action_{i}", dcc_name="maya")
            except Exception as e:
                errors.append(e)

        def adder() -> None:
            try:
                barrier.wait()
                for i in range(100, 200):
                    reg.register(name=f"action_{i}", dcc="blender")
            except Exception as e:
                errors.append(e)

        t1 = threading.Thread(target=remover)
        t2 = threading.Thread(target=adder)
        t1.start()
        t2.start()
        t1.join(timeout=10)
        t2.join(timeout=10)
        assert not errors, f"Concurrent unregister errors: {errors}"

    def test_concurrent_search_actions(self) -> None:
        """Multiple threads can search while others write."""
        reg = dcc_mcp_core.ActionRegistry()
        for i in range(50):
            reg.register(name=f"geo_{i}", category="geometry", dcc="maya")

        errors: list[Exception] = []
        barrier = threading.Barrier(4)

        def searcher() -> None:
            try:
                barrier.wait()
                for _ in range(100):
                    results = reg.search_actions(category="geometry")
                    assert isinstance(results, list)
            except Exception as e:
                errors.append(e)

        def writer() -> None:
            try:
                barrier.wait()
                for i in range(50, 100):
                    reg.register(name=f"anim_{i}", category="animation", dcc="maya")
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=searcher),
            threading.Thread(target=searcher),
            threading.Thread(target=writer),
            threading.Thread(target=writer),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent search errors: {errors}"

    def test_concurrent_register_batch(self) -> None:
        """Concurrent register_batch calls must be race-free."""
        reg = dcc_mcp_core.ActionRegistry()
        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def batch_writer(dcc: str, start: int) -> None:
            try:
                barrier.wait()
                actions = [{"name": f"act_{dcc}_{i}", "dcc": dcc, "category": "geo"} for i in range(start, start + 50)]
                reg.register_batch(actions)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=batch_writer, args=("maya", 0)),
            threading.Thread(target=batch_writer, args=("blender", 50)),
            threading.Thread(target=batch_writer, args=("houdini", 100)),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent batch register errors: {errors}"
        assert len(reg) == 150


# ── VersionedRegistry concurrent access ──


class TestConcurrentVersionedRegistry:
    def test_concurrent_register_and_resolve(self) -> None:
        """Concurrent register_versioned + resolve must be thread-safe."""
        vr = dcc_mcp_core.VersionedRegistry()
        errors: list[Exception] = []
        barrier = threading.Barrier(4)

        def writer(dcc: str) -> None:
            try:
                barrier.wait()
                for minor in range(10):
                    vr.register_versioned("action_concurrent", dcc=dcc, version=f"1.{minor}.0")
            except Exception as e:
                errors.append(e)

        def resolver() -> None:
            try:
                barrier.wait()
                for _ in range(50):
                    vr.resolve("action_concurrent", dcc="maya", constraint="*")
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=writer, args=("maya",)),
            threading.Thread(target=writer, args=("blender",)),
            threading.Thread(target=resolver),
            threading.Thread(target=resolver),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent versioned registry errors: {errors}"

    def test_concurrent_resolve_all_and_remove(self) -> None:
        """Concurrent resolve_all + remove must not crash."""
        vr = dcc_mcp_core.VersionedRegistry()
        for minor in range(10):
            vr.register_versioned("my_action", dcc="maya", version=f"1.{minor}.0")

        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def resolver() -> None:
            try:
                barrier.wait()
                for _ in range(100):
                    results = vr.resolve_all("my_action", dcc="maya", constraint="*")
                    assert isinstance(results, list)
            except Exception as e:
                errors.append(e)

        def remover() -> None:
            try:
                barrier.wait()
                for minor in range(5):
                    vr.remove("my_action", dcc="maya", constraint=f"=1.{minor}.0")
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=resolver),
            threading.Thread(target=resolver),
            threading.Thread(target=remover),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent resolve_all/remove errors: {errors}"

    def test_concurrent_keys_and_latest_version(self) -> None:
        """keys() and latest_version() are safe under concurrent write."""
        vr = dcc_mcp_core.VersionedRegistry()
        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def writer() -> None:
            try:
                barrier.wait()
                for i in range(20):
                    vr.register_versioned(f"tool_{i}", dcc="maya", version="1.0.0")
            except Exception as e:
                errors.append(e)

        def reader() -> None:
            try:
                barrier.wait()
                for _ in range(100):
                    keys = vr.keys()
                    assert isinstance(keys, list)
                    # Check latest_version for any existing keys
                    for name, dcc in keys[:5]:
                        lv = vr.latest_version(name, dcc=dcc)
                        assert lv is None or isinstance(lv, str)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=writer),
            threading.Thread(target=reader),
            threading.Thread(target=reader),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent keys/latest_version errors: {errors}"


# ── ActionRecorder concurrent access ──


class TestConcurrentActionRecorder:
    def test_concurrent_start_and_finish(self) -> None:
        """Multiple threads recording distinct actions must not corrupt state."""
        recorder = dcc_mcp_core.ActionRecorder("concurrent_scope")
        errors: list[Exception] = []
        barrier = threading.Barrier(4)

        def record_actions(action_name: str) -> None:
            try:
                barrier.wait()
                for _ in range(20):
                    guard = recorder.start(action_name, "maya")
                    guard.finish(True)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=record_actions, args=(f"action_{i}",)) for i in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent recorder errors: {errors}"
        # Each of 4 actions had 20 invocations
        for i in range(4):
            m = recorder.metrics(f"action_{i}")
            assert m is not None
            assert m.invocation_count == 20
            assert m.success_count == 20

    def test_concurrent_success_and_failure(self) -> None:
        """Mix of successes and failures recorded from multiple threads."""
        recorder = dcc_mcp_core.ActionRecorder("mixed_scope")
        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def record_success() -> None:
            try:
                barrier.wait()
                for _ in range(30):
                    guard = recorder.start("shared_action", "maya")
                    guard.finish(True)
            except Exception as e:
                errors.append(e)

        def record_failure() -> None:
            try:
                barrier.wait()
                for _ in range(30):
                    guard = recorder.start("shared_action", "maya")
                    guard.finish(False)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=record_success),
            threading.Thread(target=record_success),
            threading.Thread(target=record_failure),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent success/failure recorder errors: {errors}"
        m = recorder.metrics("shared_action")
        assert m is not None
        # 2 success threads x 30 + 1 failure thread x 30 = 90 total
        assert m.invocation_count == 90
        assert m.success_count == 60
        assert m.failure_count == 30

    def test_concurrent_reset_and_record(self) -> None:
        """reset() during active recording must not crash."""
        import time

        recorder = dcc_mcp_core.ActionRecorder("reset_scope")
        errors: list[Exception] = []
        barrier = threading.Barrier(2)

        def recorder_thread() -> None:
            try:
                barrier.wait()
                for _ in range(50):
                    guard = recorder.start("act", "maya")
                    guard.finish(True)
            except Exception as e:
                errors.append(e)

        def resetter_thread() -> None:
            try:
                barrier.wait()
                for _ in range(5):
                    time.sleep(0.001)
                    recorder.reset()
            except Exception as e:
                errors.append(e)

        t1 = threading.Thread(target=recorder_thread)
        t2 = threading.Thread(target=resetter_thread)
        t1.start()
        t2.start()
        t1.join(timeout=10)
        t2.join(timeout=10)
        assert not errors, f"Concurrent reset/record errors: {errors}"


# ── TransportManager concurrent access ──


class TestConcurrentTransportManager:
    def test_concurrent_register_and_list(self, tmp_path) -> None:
        """Concurrent register_service + list_all_services must be race-free."""
        tm = dcc_mcp_core.TransportManager(str(tmp_path))
        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def register_services(dcc: str, base_port: int) -> None:
            try:
                barrier.wait()
                for i in range(20):
                    tm.register_service(dcc, "127.0.0.1", base_port + i, version="1.0.0")
            except Exception as e:
                errors.append(e)

        def lister() -> None:
            try:
                barrier.wait()
                for _ in range(100):
                    services = tm.list_all_services()
                    assert isinstance(services, list)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=register_services, args=("maya", 10000)),
            threading.Thread(target=register_services, args=("blender", 10100)),
            threading.Thread(target=lister),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent TransportManager register/list errors: {errors}"

    def test_concurrent_heartbeat_and_list_instances(self, tmp_path) -> None:
        """Concurrent heartbeat + list_instances must be thread-safe."""
        tm = dcc_mcp_core.TransportManager(str(tmp_path))
        instance_id = tm.register_service("maya", "127.0.0.1", 19999, version="1.0.0")

        errors: list[Exception] = []
        barrier = threading.Barrier(3)

        def heartbeater() -> None:
            try:
                barrier.wait()
                for _ in range(50):
                    tm.heartbeat("maya", instance_id)
            except Exception as e:
                errors.append(e)

        def lister() -> None:
            try:
                barrier.wait()
                for _ in range(100):
                    instances = tm.list_instances("maya")
                    assert isinstance(instances, list)
            except Exception as e:
                errors.append(e)

        threads = [
            threading.Thread(target=heartbeater),
            threading.Thread(target=heartbeater),
            threading.Thread(target=lister),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=15)
        assert not errors, f"Concurrent heartbeat/list_instances errors: {errors}"
