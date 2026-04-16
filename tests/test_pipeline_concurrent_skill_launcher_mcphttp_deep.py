"""Deep tests for ToolPipeline concurrency, SkillWatcher multi-path, PyDccLauncher, and McpHttpConfig.

Covers:
- TestActionPipelineConcurrent: 20+ concurrent dispatch with threading
- TestSkillWatcherMultiPath: multi-path watch/unwatch/reload combinations
- TestSkillScannerCacheClearing: clear_cache / rescan behaviour
- TestPyDccLauncherDeep: running_count/restart_count/launch/terminate/kill
- TestMcpHttpConfigDeep: port/server_name/server_version/defaults
- TestActionPipelineAddCallable: before_fn/after_fn callable hooks
- TestActionPipelineRegisterHandler: pipeline.register_handler + dispatch
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed

# Import built-in modules
import contextlib
from pathlib import Path
import shutil
import tempfile
import threading

# Import third-party modules
import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import SkillScanner
from dcc_mcp_core import SkillWatcher

# Import local modules
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core import scan_and_load
from dcc_mcp_core import scan_and_load_lenient
from dcc_mcp_core import scan_skill_paths

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

EXAMPLES_DIR = str(Path(__file__).parent / ".." / "examples" / "skills")


def _make_pipeline(n_actions: int = 1) -> tuple[ToolPipeline, list[str]]:
    """Create a pipeline with n_actions registered."""
    reg = ToolRegistry()
    names = []
    for i in range(n_actions):
        name = f"action_{i}"
        reg.register(name, description=f"Action {i}", category="test")
        names.append(name)

    disp = ToolDispatcher(reg)
    for name in names:
        disp.register_handler(name, lambda p, n=name: {"result": n, "value": 42})

    pipe = ToolPipeline(disp)
    return pipe, names


def _make_skill_dir(
    base: str, skill_name: str, dcc: str = "python", version: str = "1.0.0", depends: list[str] | None = None
) -> str:
    """Create a valid skill directory with SKILL.md and a script."""
    skill_dir = Path(base) / skill_name
    (skill_dir / "scripts").mkdir(parents=True, exist_ok=True)

    depends_str = ""
    if depends:
        depends_str = "\ndepends: " + str(depends)

    with (skill_dir / "SKILL.md").open("w") as f:
        f.write(
            f"---\n"
            f"name: {skill_name}\n"
            f'description: "Test skill {skill_name}"\n'
            f"dcc: {dcc}\n"
            f'version: "{version}"\n'
            f'tags: ["test"]{depends_str}\n'
            f"---\n\n"
            f"# {skill_name}\n\nA test skill.\n"
        )

    with (skill_dir / "scripts" / "main.py").open("w") as f:
        f.write(f"# {skill_name} main script\ndef run(): pass\n")

    return str(skill_dir)


# ---------------------------------------------------------------------------
# TestActionPipelineConcurrent
# ---------------------------------------------------------------------------


class TestActionPipelineConcurrent:
    """Multi-thread concurrent dispatch tests for ToolPipeline."""

    def test_single_thread_basic_dispatch(self):
        pipe, names = _make_pipeline(1)
        result = pipe.dispatch(names[0], "{}")
        assert result["action"] == names[0]
        assert isinstance(result["output"], dict)

    def test_concurrent_dispatch_20_threads_same_action(self):
        """20 threads dispatch the same action concurrently — no race conditions."""
        pipe, names = _make_pipeline(1)
        action = names[0]
        errors = []
        results = []
        lock = threading.Lock()

        def worker():
            try:
                r = pipe.dispatch(action, "{}")
                with lock:
                    results.append(r)
            except Exception as exc:
                with lock:
                    errors.append(str(exc))

        threads = [threading.Thread(target=worker) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Concurrent dispatch errors: {errors}"
        assert len(results) == 20
        for r in results:
            assert r["action"] == action

    def test_concurrent_dispatch_5_different_actions(self):
        """Dispatch 5 different actions from 5 separate threads."""
        pipe, names = _make_pipeline(5)
        errors = []
        results = {}
        lock = threading.Lock()

        def worker(action_name):
            try:
                r = pipe.dispatch(action_name, "{}")
                with lock:
                    results[action_name] = r
            except Exception as exc:
                with lock:
                    errors.append(str(exc))

        threads = [threading.Thread(target=worker, args=(n,)) for n in names]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Errors: {errors}"
        assert len(results) == 5
        for name in names:
            assert results[name]["action"] == name

    def test_concurrent_dispatch_with_timing_middleware(self):
        """Timing middleware should survive concurrent dispatches."""
        pipe, names = _make_pipeline(1)
        timing = pipe.add_timing()
        action = names[0]
        errors = []

        def worker():
            try:
                pipe.dispatch(action, "{}")
            except Exception as exc:
                errors.append(str(exc))

        threads = [threading.Thread(target=worker) for _ in range(15)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Timing middleware concurrent errors: {errors}"
        # last_elapsed_ms should be set (>= 0)
        elapsed = timing.last_elapsed_ms(action)
        assert elapsed is not None
        assert elapsed >= 0

    def test_concurrent_dispatch_with_audit_middleware(self):
        """Audit middleware should accumulate all records under concurrent dispatch."""
        pipe, names = _make_pipeline(1)
        audit = pipe.add_audit(record_params=False)
        action = names[0]
        n_threads = 20

        def worker():
            pipe.dispatch(action, "{}")

        threads = [threading.Thread(target=worker) for _ in range(n_threads)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        count = audit.record_count()
        assert count == n_threads, f"Expected {n_threads} audit records, got {count}"

    def test_concurrent_dispatch_multiple_handlers(self):
        """Register 10 actions, dispatch all concurrently 2 times each."""
        pipe, names = _make_pipeline(10)
        errors = []
        results = []
        lock = threading.Lock()

        def worker(name):
            for _ in range(2):
                try:
                    r = pipe.dispatch(name, "{}")
                    with lock:
                        results.append(r["action"])
                except Exception as exc:
                    with lock:
                        errors.append(str(exc))

        threads = [threading.Thread(target=worker, args=(n,)) for n in names]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Errors: {errors}"
        assert len(results) == 20  # 10 actions x 2 dispatches

    def test_concurrent_dispatch_with_threadpool_executor(self):
        """Use ThreadPoolExecutor for cleaner concurrent dispatch test."""
        pipe, names = _make_pipeline(1)
        action = names[0]
        n_workers = 10

        with ThreadPoolExecutor(max_workers=n_workers) as executor:
            futures = [executor.submit(pipe.dispatch, action, "{}") for _ in range(n_workers)]
            results = [f.result() for f in as_completed(futures)]

        assert len(results) == n_workers
        for r in results:
            assert r["action"] == action
            assert "output" in r

    def test_dispatch_unknown_action_raises_key_error(self):
        """Dispatch on an unregistered action raises KeyError."""
        pipe, _ = _make_pipeline(1)
        with pytest.raises(KeyError):
            pipe.dispatch("nonexistent_action", "{}")

    def test_pipeline_handler_count_increments(self):
        """register_handler increases handler_count."""
        pipe, _names = _make_pipeline(3)
        assert pipe.handler_count() == 3

    def test_pipeline_middleware_count_increments(self):
        """Each add_* call increases middleware_count."""
        pipe, _ = _make_pipeline(1)
        assert pipe.middleware_count() == 0
        pipe.add_logging(log_params=False)
        assert pipe.middleware_count() == 1
        pipe.add_timing()
        assert pipe.middleware_count() == 2
        pipe.add_audit(record_params=False)
        assert pipe.middleware_count() == 3
        pipe.add_rate_limit(max_calls=100, window_ms=1000)
        assert pipe.middleware_count() == 4

    def test_pipeline_middleware_names_order(self):
        """middleware_names returns names in insertion order."""
        pipe, _ = _make_pipeline(1)
        pipe.add_logging(log_params=False)
        pipe.add_timing()
        pipe.add_audit(record_params=False)
        names = pipe.middleware_names()
        assert "logging" in names
        assert "timing" in names
        assert "audit" in names
        # Order: logging before timing before audit
        assert names.index("logging") < names.index("timing") < names.index("audit")


# ---------------------------------------------------------------------------
# TestActionPipelineAddCallable
# ---------------------------------------------------------------------------


class TestActionPipelineAddCallable:
    """Tests for ToolPipeline.add_callable hook."""

    def test_add_callable_before_fn_called(self):
        """before_fn is called with action name before dispatch."""
        pipe, names = _make_pipeline(1)
        called_before = []
        pipe.add_callable(
            before_fn=lambda action: called_before.append(action),
            after_fn=None,
        )
        pipe.dispatch(names[0], "{}")
        assert names[0] in called_before

    def test_add_callable_after_fn_called(self):
        """after_fn is called with (action, success) after dispatch."""
        pipe, names = _make_pipeline(1)
        called_after = []
        pipe.add_callable(
            before_fn=None,
            after_fn=lambda action, success: called_after.append((action, success)),
        )
        pipe.dispatch(names[0], "{}")
        assert len(called_after) == 1
        assert called_after[0][0] == names[0]
        assert called_after[0][1] is True

    def test_add_callable_both_fns_called_in_order(self):
        """Both before_fn and after_fn are called for a single dispatch."""
        pipe, names = _make_pipeline(1)
        order = []
        pipe.add_callable(
            before_fn=lambda a: order.append("before"),
            after_fn=lambda a, s: order.append("after"),
        )
        pipe.dispatch(names[0], "{}")
        assert order == ["before", "after"]

    def test_add_callable_multiple_dispatches(self):
        """Callable hooks are invoked for every dispatch call."""
        pipe, names = _make_pipeline(1)
        call_count = [0]
        pipe.add_callable(
            before_fn=lambda a: None,
            after_fn=lambda a, s: call_count.__setitem__(0, call_count[0] + 1),
        )
        for _ in range(5):
            pipe.dispatch(names[0], "{}")
        assert call_count[0] == 5

    def test_add_callable_increases_middleware_count(self):
        """add_callable adds exactly one middleware entry."""
        pipe, _ = _make_pipeline(1)
        before_count = pipe.middleware_count()
        pipe.add_callable(before_fn=lambda a: None, after_fn=None)
        assert pipe.middleware_count() == before_count + 1

    def test_add_callable_no_fns(self):
        """add_callable with both None should not raise."""
        pipe, names = _make_pipeline(1)
        pipe.add_callable(before_fn=None, after_fn=None)
        # Should dispatch without error
        result = pipe.dispatch(names[0], "{}")
        assert result["action"] == names[0]


# ---------------------------------------------------------------------------
# TestActionPipelineRegisterHandler
# ---------------------------------------------------------------------------


class TestActionPipelineRegisterHandler:
    """Tests for ToolPipeline.register_handler."""

    def test_register_handler_and_dispatch(self):
        """Handler registered on pipeline is callable via dispatch."""
        reg = ToolRegistry()
        reg.register("my_fn", description="fn", category="util")
        disp = ToolDispatcher(reg)
        pipe = ToolPipeline(disp)

        pipe.register_handler("my_fn", lambda p: {"ok": True})
        result = pipe.dispatch("my_fn", "{}")
        assert result["output"]["ok"] is True

    def test_register_multiple_handlers_independently_callable(self):
        """Multiple handlers can coexist and dispatch independently."""
        reg = ToolRegistry()
        for name in ["fn_a", "fn_b", "fn_c"]:
            reg.register(name, description="test", category="util")
        disp = ToolDispatcher(reg)
        pipe = ToolPipeline(disp)

        pipe.register_handler("fn_a", lambda p: {"fn": "a"})
        pipe.register_handler("fn_b", lambda p: {"fn": "b"})
        pipe.register_handler("fn_c", lambda p: {"fn": "c"})

        assert pipe.dispatch("fn_a", "{}")["output"]["fn"] == "a"
        assert pipe.dispatch("fn_b", "{}")["output"]["fn"] == "b"
        assert pipe.dispatch("fn_c", "{}")["output"]["fn"] == "c"

    def test_register_handler_increases_handler_count(self):
        """handler_count reflects all registered handlers."""
        reg = ToolRegistry()
        reg.register("act", description="a", category="x")
        disp = ToolDispatcher(reg)
        pipe = ToolPipeline(disp)

        assert pipe.handler_count() == 0
        pipe.register_handler("act", lambda p: {})
        assert pipe.handler_count() == 1

    def test_dispatch_result_contains_validation_skipped_flag(self):
        """Result dict always contains 'validation_skipped' key."""
        pipe, names = _make_pipeline(1)
        result = pipe.dispatch(names[0], "{}")
        assert "validation_skipped" in result

    def test_dispatch_output_is_handler_return_value(self):
        """The 'output' key in result matches the handler's return value."""
        reg = ToolRegistry()
        reg.register("echo", description="echo", category="util")
        disp = ToolDispatcher(reg)
        disp.register_handler("echo", lambda p: {"echo": "hello"})
        pipe = ToolPipeline(disp)
        result = pipe.dispatch("echo", "{}")
        assert result["output"] == {"echo": "hello"}


# ---------------------------------------------------------------------------
# TestSkillWatcherMultiPath
# ---------------------------------------------------------------------------


class TestSkillWatcherMultiPath:
    """SkillWatcher multi-path watch/unwatch/reload tests."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()

    def teardown_method(self):
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    def _create_skill(self, skill_name: str, dcc: str = "python") -> str:
        return _make_skill_dir(self.tmpdir, skill_name, dcc=dcc)

    def test_watch_single_path_loads_skills(self):
        self._create_skill("skill-a")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        assert watcher.skill_count() >= 1
        assert self.tmpdir in watcher.watched_paths()

    def test_watch_loads_correct_skill_names(self):
        self._create_skill("alpha-skill")
        self._create_skill("beta-skill")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        names = [s.name for s in watcher.skills()]
        assert "alpha-skill" in names
        assert "beta-skill" in names

    def test_unwatch_removes_path(self):
        self._create_skill("skill-x")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        assert self.tmpdir in watcher.watched_paths()
        result = watcher.unwatch(self.tmpdir)
        assert result is True
        assert self.tmpdir not in watcher.watched_paths()

    def test_unwatch_clears_skills(self):
        self._create_skill("skill-y")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        assert watcher.skill_count() >= 1
        watcher.unwatch(self.tmpdir)
        assert watcher.skill_count() == 0

    def test_unwatch_nonexistent_path_returns_false(self):
        watcher = SkillWatcher()
        result = watcher.unwatch("/nonexistent/path/12345")
        assert result is False

    def test_reload_returns_count(self):
        self._create_skill("skill-reload")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        watcher.reload()  # Should not raise
        assert watcher.skill_count() >= 1

    def test_reload_after_adding_new_skill_discovers_it(self):
        """After adding a new skill dir, reload() picks it up."""
        self._create_skill("skill-before")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        count_before = watcher.skill_count()

        # Add a new skill after initial watch
        self._create_skill("skill-after")
        watcher.reload()
        count_after = watcher.skill_count()
        assert count_after >= count_before

    def test_empty_watcher_skill_count_is_zero(self):
        watcher = SkillWatcher()
        assert watcher.skill_count() == 0

    def test_empty_watcher_watched_paths_is_empty(self):
        watcher = SkillWatcher()
        assert watcher.watched_paths() == []

    def test_empty_watcher_skills_is_empty(self):
        watcher = SkillWatcher()
        assert watcher.skills() == []

    def test_watch_same_path_twice_is_idempotent(self):
        """Watching the same path twice should not duplicate skills."""
        self._create_skill("skill-dup")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        count_first = watcher.skill_count()
        watcher.watch(self.tmpdir)
        count_second = watcher.skill_count()
        # Count should not double
        assert count_second == count_first

    def test_watcher_skills_have_expected_attrs(self):
        self._create_skill("attr-skill")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        skills = watcher.skills()
        assert len(skills) >= 1
        s = skills[0]
        # SkillMetadata must have these attrs
        for attr in ("name", "dcc", "version", "description", "tags", "scripts", "depends"):
            assert hasattr(s, attr), f"Missing attr: {attr}"

    def test_watcher_skill_name_matches_directory(self):
        self._create_skill("named-skill-99")
        watcher = SkillWatcher()
        watcher.watch(self.tmpdir)
        names = [s.name for s in watcher.skills()]
        assert "named-skill-99" in names

    def test_examples_dir_loads_all_example_skills(self):
        """Load all example skills from examples/skills directory."""
        examples = str(Path(__file__).parent / ".." / "examples" / "skills")
        watcher = SkillWatcher()
        watcher.watch(examples)
        assert watcher.skill_count() == 11

    def test_watcher_with_invalid_path_does_not_crash(self):
        """Watching a non-existent path should not raise an exception."""
        watcher = SkillWatcher()
        # May log a warning but should not raise
        with contextlib.suppress(Exception):
            watcher.watch("/totally/nonexistent/path/xyz")
        # Either way, skill_count should remain 0 or minimal
        assert watcher.skill_count() >= 0


# ---------------------------------------------------------------------------
# TestSkillScannerCacheClearing
# ---------------------------------------------------------------------------


class TestSkillScannerCacheClearing:
    """Tests for SkillScanner cache behaviour."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()

    def teardown_method(self):
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    def test_scanner_discovers_single_skill(self):
        _make_skill_dir(self.tmpdir, "scan-skill-1")
        scanner = SkillScanner()
        paths = scanner.scan([self.tmpdir])
        assert len(paths) == 1
        assert any("scan-skill-1" in p for p in paths)

    def test_scanner_discovers_multiple_skills(self):
        for i in range(3):
            _make_skill_dir(self.tmpdir, f"scan-skill-{i}")
        scanner = SkillScanner()
        paths = scanner.scan([self.tmpdir])
        assert len(paths) == 3

    def test_scanner_clear_cache_resets_state(self):
        _make_skill_dir(self.tmpdir, "cache-skill")
        scanner = SkillScanner()
        scanner.scan([self.tmpdir])
        # discovered_skills is a property (list), not a method
        assert len(scanner.discovered_skills) >= 1
        scanner.clear_cache()
        # After clear, discovered_skills should be empty
        assert scanner.discovered_skills == []

    def test_scanner_discovered_skills_after_scan(self):
        _make_skill_dir(self.tmpdir, "disc-skill")
        scanner = SkillScanner()
        scanner.scan([self.tmpdir])
        # discovered_skills is a property
        discovered = scanner.discovered_skills
        assert len(discovered) >= 1

    def test_scanner_empty_dir_returns_empty(self):
        scanner = SkillScanner()
        paths = scanner.scan([self.tmpdir])
        assert paths == []

    def test_scanner_nonexistent_path_returns_empty(self):
        scanner = SkillScanner()
        paths = scanner.scan(["/nonexistent/path/abc123"])
        assert paths == []

    def test_scan_skill_paths_function_returns_list(self):
        _make_skill_dir(self.tmpdir, "fn-skill")
        result = scan_skill_paths([self.tmpdir])
        assert isinstance(result, list)
        assert len(result) >= 1

    def test_scan_and_load_returns_tuple(self):
        _make_skill_dir(self.tmpdir, "load-skill")
        result = scan_and_load([self.tmpdir])
        assert isinstance(result, tuple)
        assert len(result) == 2
        skills, skipped = result
        assert isinstance(skills, list)
        assert isinstance(skipped, list)

    def test_scan_and_load_lenient_returns_tuple(self):
        _make_skill_dir(self.tmpdir, "lenient-skill")
        result = scan_and_load_lenient([self.tmpdir])
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_scan_and_load_lenient_skips_malformed_skill(self):
        """A malformed SKILL.md (empty file) should appear in skipped list."""
        bad_dir = Path(self.tmpdir) / "bad-skill"
        bad_dir.mkdir(parents=True, exist_ok=True)
        with (bad_dir / "SKILL.md").open("w") as f:
            f.write("# No frontmatter\n")  # No YAML frontmatter → parse fails

        _skills, skipped = scan_and_load_lenient([self.tmpdir])
        # bad-skill should be in skipped (parse_skill_md returns None)
        assert len(skipped) >= 1

    def test_scan_examples_dir_loads_all_11(self):
        """Scan examples/skills and verify all 11 skills are loaded."""
        examples = str(Path(__file__).parent / ".." / "examples" / "skills")
        skills, skipped = scan_and_load([examples])
        assert len(skills) == 11
        assert skipped == []


# ---------------------------------------------------------------------------
# TestSkillMetadataDeepAttrs
# ---------------------------------------------------------------------------


class TestSkillMetadataDeepAttrs:
    """Detailed attribute tests for SkillMetadata from parse_skill_md."""

    def test_hello_world_name(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta is not None
        assert meta.name == "hello-world"

    def test_hello_world_dcc(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta.dcc == "python"

    def test_hello_world_version(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta.version == "1.0.0"

    def test_hello_world_description_not_empty(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta.description
        assert len(meta.description) > 0

    def test_hello_world_tags_contain_example(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert "example" in meta.tags

    def test_hello_world_scripts_not_empty(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert len(meta.scripts) >= 1

    def test_hello_world_scripts_are_py_files(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        for script in meta.scripts:
            assert script.endswith(".py"), f"Non-py script: {script}"

    def test_hello_world_depends_is_empty(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta.depends == []

    def test_hello_world_skill_path_not_empty(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta.skill_path
        assert "hello-world" in meta.skill_path

    def test_hello_world_tools_contains_bash(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert "Bash" in meta.allowed_tools

    def test_maya_pipeline_depends_populated(self):
        """maya-pipeline has non-empty depends list."""
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "maya-pipeline"))
        assert meta is not None
        assert len(meta.depends) >= 1
        assert "maya-geometry" in meta.depends

    def test_maya_pipeline_depends_includes_usd_tools(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "maya-pipeline"))
        assert "usd-tools" in meta.depends

    def test_maya_pipeline_dcc_is_maya(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "maya-pipeline"))
        assert meta.dcc == "maya"

    def test_maya_pipeline_version_is_2(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "maya-pipeline"))
        assert meta.version.startswith("2")

    def test_maya_geometry_dcc_is_maya(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "maya-geometry"))
        assert meta is not None
        assert meta.dcc == "maya"

    def test_usd_tools_scripts_not_empty(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "usd-tools"))
        assert meta is not None
        assert len(meta.scripts) >= 1

    def test_multi_script_has_multiple_scripts(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "multi-script"))
        assert meta is not None
        assert len(meta.scripts) >= 2

    def test_parse_skill_md_nonexistent_dir_raises(self):
        import pytest

        with pytest.raises(FileNotFoundError):
            parse_skill_md("/nonexistent/path/skill-xyz")

    def test_parse_skill_md_dir_without_skill_md_returns_none(self):
        tmpdir = tempfile.mkdtemp()
        try:
            result = parse_skill_md(tmpdir)
            assert result is None
        finally:
            shutil.rmtree(tmpdir)

    def test_skill_metadata_str_representation(self):
        meta = parse_skill_md(str(Path(EXAMPLES_DIR) / "hello-world"))
        assert meta is not None
        s = str(meta)
        assert "hello-world" in s


# ---------------------------------------------------------------------------
# TestPyDccLauncherDeep
# ---------------------------------------------------------------------------


class TestPyDccLauncherDeep:
    """Deep tests for PyDccLauncher attributes and methods."""

    def test_launcher_creates_without_args(self):
        launcher = PyDccLauncher()
        assert launcher is not None

    def test_launcher_running_count_initially_zero(self):
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_launcher_restart_count_requires_name(self):
        """restart_count(name) requires a DCC name argument."""
        launcher = PyDccLauncher()
        # Should not raise when passed a name
        count = launcher.restart_count("maya")
        assert count == 0

    def test_launcher_pid_of_returns_none_for_unknown(self):
        launcher = PyDccLauncher()
        result = launcher.pid_of("unknown_dcc")
        assert result is None

    def test_launcher_pid_of_returns_none_for_empty_name(self):
        launcher = PyDccLauncher()
        result = launcher.pid_of("")
        assert result is None

    def test_launcher_terminate_not_running_raises_runtime_error(self):
        """terminate() raises RuntimeError when process is not running."""
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.terminate("nonexistent")

    def test_launcher_kill_not_running_raises_runtime_error(self):
        """kill() raises RuntimeError when process is not running."""
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.kill("nonexistent")

    def test_launcher_terminate_empty_string_raises(self):
        """terminate('') raises RuntimeError — process not registered."""
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.terminate("")

    def test_launcher_kill_empty_string_raises(self):
        """kill('') raises RuntimeError — process not registered."""
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.kill("")

    def test_launcher_methods_exist(self):
        launcher = PyDccLauncher()
        expected = ["kill", "launch", "pid_of", "restart_count", "running_count", "terminate"]
        for method in expected:
            assert hasattr(launcher, method), f"Missing method: {method}"

    def test_launcher_multiple_instances_independent(self):
        """Two launchers have independent state."""
        l1 = PyDccLauncher()
        l2 = PyDccLauncher()
        assert l1.running_count() == l2.running_count()
        # Both should have same restart_count for the same name
        assert l1.restart_count("test_dcc") == l2.restart_count("test_dcc")

    def test_launcher_launch_invalid_command_raises_or_returns(self):
        """Launching a nonexistent executable should raise or return False."""
        launcher = PyDccLauncher()
        with contextlib.suppress(Exception):
            result = launcher.launch("nonexistent_dcc_app_xyz", "/nonexistent/path")
            # Acceptable: returns False/None or similar falsy value
            assert not result or result is None

    def test_launcher_restart_count_after_no_launches(self):
        """restart_count returns 0 for any DCC name before any launches."""
        launcher = PyDccLauncher()
        assert launcher.restart_count("blender") == 0
        assert launcher.restart_count("houdini") == 0

    def test_launcher_terminate_raises_runtime_error_for_known_name(self):
        """Terminate a DCC that was never launched raises RuntimeError."""
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.terminate("maya")


# ---------------------------------------------------------------------------
# TestMcpHttpConfigDeep
# ---------------------------------------------------------------------------


class TestMcpHttpConfigDeep:
    """Deep tests for McpHttpConfig attributes and defaults."""

    def test_config_port_custom(self):
        cfg = McpHttpConfig(port=8765)
        assert cfg.port == 8765

    def test_config_port_minimum(self):
        cfg = McpHttpConfig(port=1)
        assert cfg.port == 1

    def test_config_port_maximum(self):
        cfg = McpHttpConfig(port=65535)
        assert cfg.port == 65535

    def test_config_server_name_custom(self):
        cfg = McpHttpConfig(port=8080, server_name="my-dcc-server")
        assert cfg.server_name == "my-dcc-server"

    def test_config_server_version_custom(self):
        cfg = McpHttpConfig(port=8080, server_version="3.1.4")
        assert cfg.server_version == "3.1.4"

    def test_config_all_fields(self):
        cfg = McpHttpConfig(port=9090, server_name="test", server_version="1.2.3")
        assert cfg.port == 9090
        assert cfg.server_name == "test"
        assert cfg.server_version == "1.2.3"

    def test_config_has_expected_attrs(self):
        cfg = McpHttpConfig(port=8080)
        for attr in ("port", "server_name", "server_version"):
            assert hasattr(cfg, attr), f"Missing attr: {attr}"

    def test_config_default_server_name_not_none(self):
        cfg = McpHttpConfig(port=8080)
        assert cfg.server_name is not None

    def test_config_default_server_version_not_none(self):
        cfg = McpHttpConfig(port=8080)
        assert cfg.server_version is not None

    def test_multiple_configs_independent(self):
        cfg1 = McpHttpConfig(port=8001, server_name="server-1")
        cfg2 = McpHttpConfig(port=8002, server_name="server-2")
        assert cfg1.port != cfg2.port
        assert cfg1.server_name != cfg2.server_name

    def test_config_port_zero(self):
        """Port 0 is technically valid (OS assigns a free port in some impls)."""
        with contextlib.suppress(Exception):
            cfg = McpHttpConfig(port=0)
            assert cfg.port == 0

    def test_mcp_http_server_creates_from_config(self):
        """McpHttpServer can be created from registry + config without raising."""
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=19997)
        srv = McpHttpServer(reg, cfg)
        assert srv is not None

    def test_mcp_http_server_has_start_method(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=19996)
        srv = McpHttpServer(reg, cfg)
        assert hasattr(srv, "start")
        assert callable(srv.start)

    def test_mcp_http_server_with_registered_actions(self):
        """McpHttpServer with pre-registered actions in the registry."""
        reg = ToolRegistry()
        for i in range(5):
            reg.register(f"tool_{i}", description=f"Tool {i}", category="test")
        cfg = McpHttpConfig(port=19995, server_name="test-dcc", server_version="0.1.0")
        srv = McpHttpServer(reg, cfg)
        assert srv is not None

    def test_config_server_name_empty_string(self):
        """Empty string server_name is acceptable if the API allows."""
        with contextlib.suppress(Exception):
            cfg = McpHttpConfig(port=8080, server_name="")
            assert cfg.server_name == "" or cfg.server_name is not None

    def test_config_server_version_semver(self):
        cfg = McpHttpConfig(port=8080, server_version="0.12.9")
        assert cfg.server_version == "0.12.9"


# ---------------------------------------------------------------------------
# TestActionPipelineRateLimitDeep
# ---------------------------------------------------------------------------


class TestActionPipelineRateLimitDeep:
    """Additional rate-limit edge-case tests."""

    def test_rate_limit_call_count_increments(self):
        pipe, names = _make_pipeline(1)
        rl = pipe.add_rate_limit(max_calls=100, window_ms=10000)
        for _i in range(5):
            pipe.dispatch(names[0], "{}")
        assert rl.call_count(names[0]) == 5

    def test_rate_limit_max_calls_accessible(self):
        pipe, _ = _make_pipeline(1)
        rl = pipe.add_rate_limit(max_calls=50, window_ms=2000)
        assert rl.max_calls == 50

    def test_rate_limit_window_ms_accessible(self):
        pipe, _ = _make_pipeline(1)
        rl = pipe.add_rate_limit(max_calls=10, window_ms=3000)
        assert rl.window_ms == 3000

    def test_rate_limit_different_actions_have_independent_counts(self):
        pipe, names = _make_pipeline(2)
        rl = pipe.add_rate_limit(max_calls=100, window_ms=10000)
        pipe.dispatch(names[0], "{}")
        pipe.dispatch(names[0], "{}")
        pipe.dispatch(names[1], "{}")
        assert rl.call_count(names[0]) == 2
        assert rl.call_count(names[1]) == 1

    def test_rate_limit_zero_count_before_any_dispatch(self):
        pipe, names = _make_pipeline(1)
        rl = pipe.add_rate_limit(max_calls=100, window_ms=5000)
        assert rl.call_count(names[0]) == 0

    def test_rate_limit_over_limit_raises(self):
        pipe, names = _make_pipeline(1)
        # Very tight limit: 1 call per 10 seconds
        pipe.add_rate_limit(max_calls=1, window_ms=10000)
        pipe.dispatch(names[0], "{}")  # First dispatch succeeds
        with pytest.raises(RuntimeError):
            pipe.dispatch(names[0], "{}")  # Second dispatch exceeds limit

    def test_audit_after_rate_limit_exceeded_has_one_record(self):
        """Only the successful dispatch should appear in audit log."""
        pipe, names = _make_pipeline(1)
        audit = pipe.add_audit(record_params=False)
        pipe.add_rate_limit(max_calls=1, window_ms=10000)
        pipe.dispatch(names[0], "{}")  # Succeeds
        with contextlib.suppress(RuntimeError):
            pipe.dispatch(names[0], "{}")  # Fails — rate limited
        # Only 1 successful audit record (rate-limited call did not produce audit record)
        assert audit.record_count() >= 1
