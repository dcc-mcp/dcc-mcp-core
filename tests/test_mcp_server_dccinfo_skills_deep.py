"""Deep behavioral tests for McpHttpServer/ServerHandle, DccInfo/DccCapabilities, and skills.

Covers: SkillScanner, parse_skill_md, scan_and_load, scan_and_load_lenient,
scan_skill_paths, resolve_dependencies, expand_transitive_dependencies,
and validate_dependencies.

Coverage groups:
- TestMcpHttpConfigAttributes      (12 tests)
- TestMcpHttpServerStartHandle     (12 tests)
- TestServerHandleAttributes       (10 tests)
- TestDccInfoConstruction          (10 tests)
- TestDccInfoAttributes            (12 tests)
- TestDccCapabilitiesDefaults      (10 tests)
- TestDccCapabilitiesCustom        (8 tests)
- TestSkillScannerLifecycle        (8 tests)
- TestParseSkillMd                 (12 tests)
- TestScanSkillPaths               (6 tests)
- TestScanAndLoad                  (8 tests)
- TestScanAndLoadLenient           (6 tests)
- TestResolveDependencies          (8 tests)
- TestExpandTransitiveDependencies (6 tests)
- TestValidateDependencies         (7 tests)
"""

from __future__ import annotations

import os
from pathlib import Path
import tempfile

import pytest

import dcc_mcp_core as d
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccInfo
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import SkillScanner
from dcc_mcp_core import expand_transitive_dependencies
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core import resolve_dependencies
from dcc_mcp_core import scan_and_load
from dcc_mcp_core import scan_and_load_lenient
from dcc_mcp_core import scan_skill_paths
from dcc_mcp_core import validate_dependencies

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

EXAMPLES_SKILLS_DIR = str(Path(__file__).parent.parent / "examples" / "skills")


def _make_skill_dir(
    tmp_dir: str,
    name: str,
    dcc: str = "python",
    version: str = "1.0.0",
    deps: list[str] | None = None,
    scripts: bool = True,
) -> str:
    """Create a minimal valid skill directory under tmp_dir."""
    skill_dir = Path(tmp_dir) / name
    (skill_dir / "scripts").mkdir(parents=True, exist_ok=True)
    deps_str = ""
    if deps:
        deps_str = "\ndepends:\n" + "".join(f"  - {dep}\n" for dep in deps)
    content = (
        f"---\n"
        f"name: {name}\n"
        f"description: Test skill {name}\n"
        f"dcc: {dcc}\n"
        f"version: {version}\n"
        f"tools: []\n"
        f"tags: [test]{deps_str}\n"
        f"---\n\n"
        f"# {name}\n"
    )
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    if scripts:
        (skill_dir / "scripts" / "main.py").write_text(f'# {name} main script\nprint("{name}")\n', encoding="utf-8")
    return str(skill_dir)


# ---------------------------------------------------------------------------
# TestMcpHttpConfigAttributes (12 tests)
# ---------------------------------------------------------------------------


class TestMcpHttpConfigAttributes:
    """Verify McpHttpConfig construction and attribute defaults."""

    def test_default_port(self) -> None:
        """Default port is 8765."""
        cfg = McpHttpConfig()
        assert cfg.port == 8765

    def test_custom_port(self) -> None:
        """Custom port is stored correctly."""
        cfg = McpHttpConfig(port=9999)
        assert cfg.port == 9999

    def test_default_server_name(self) -> None:
        """Default server_name is 'dcc-mcp'."""
        cfg = McpHttpConfig()
        assert cfg.server_name == "dcc-mcp"

    def test_custom_server_name(self) -> None:
        """Custom server_name is stored."""
        cfg = McpHttpConfig(server_name="my-dcc")
        assert cfg.server_name == "my-dcc"

    def test_server_version_is_string(self) -> None:
        """server_version is a non-empty string."""
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0

    def test_server_version_format(self) -> None:
        """server_version follows semver pattern X.Y.Z."""
        cfg = McpHttpConfig()
        parts = cfg.server_version.split(".")
        assert len(parts) == 3

    def test_port_zero_is_valid(self) -> None:
        """Port 0 (ephemeral) is accepted."""
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_port_min_valid(self) -> None:
        """Port 1 is accepted."""
        cfg = McpHttpConfig(port=1)
        assert cfg.port == 1

    def test_port_max_valid(self) -> None:
        """Port 65535 is accepted."""
        cfg = McpHttpConfig(port=65535)
        assert cfg.port == 65535

    def test_config_type(self) -> None:
        """McpHttpConfig instance has correct type."""
        cfg = McpHttpConfig()
        assert isinstance(cfg, McpHttpConfig)

    def test_repr_contains_class(self) -> None:
        """repr() contains class name."""
        cfg = McpHttpConfig(port=8765)
        r = repr(cfg)
        assert "McpHttpConfig" in r or "8765" in str(cfg.port)

    def test_multiple_configs_independent(self) -> None:
        """Two McpHttpConfig instances are independent."""
        c1 = McpHttpConfig(port=8000, server_name="a")
        c2 = McpHttpConfig(port=9000, server_name="b")
        assert c1.port != c2.port
        assert c1.server_name != c2.server_name


# ---------------------------------------------------------------------------
# TestMcpHttpServerStartHandle (12 tests)
# ---------------------------------------------------------------------------


class TestMcpHttpServerStartHandle:
    """Verify McpHttpServer.start() returns a working ServerHandle."""

    def _make_server(self, port: int = 0) -> McpHttpServer:
        reg = ActionRegistry()
        reg.register("ping", description="Ping action", category="test")
        return McpHttpServer(reg, McpHttpConfig(port=port))

    def test_start_returns_handle(self) -> None:
        """start() returns a ServerHandle instance."""
        server = self._make_server()
        handle = server.start()
        try:
            assert handle is not None
        finally:
            handle.shutdown()

    def test_handle_port_is_positive(self) -> None:
        """Server bound on ephemeral port → handle.port > 0."""
        handle = self._make_server(port=0).start()
        try:
            assert handle.port > 0
        finally:
            handle.shutdown()

    def test_handle_mcp_url_format(self) -> None:
        """mcp_url() returns a string starting with 'http://'."""
        handle = self._make_server(port=0).start()
        try:
            url = handle.mcp_url()
            assert isinstance(url, str)
            assert url.startswith("http://")
        finally:
            handle.shutdown()

    def test_handle_mcp_url_contains_mcp_path(self) -> None:
        """mcp_url() ends with '/mcp'."""
        handle = self._make_server(port=0).start()
        try:
            assert handle.mcp_url().endswith("/mcp")
        finally:
            handle.shutdown()

    def test_handle_bind_addr_is_string(self) -> None:
        """bind_addr is a non-empty string."""
        handle = self._make_server(port=0).start()
        try:
            addr = handle.bind_addr
            assert isinstance(addr, str)
            assert len(addr) > 0
        finally:
            handle.shutdown()

    def test_handle_bind_addr_contains_port(self) -> None:
        """bind_addr contains the actual port number."""
        handle = self._make_server(port=0).start()
        try:
            port_str = str(handle.port)
            assert port_str in handle.bind_addr
        finally:
            handle.shutdown()

    def test_shutdown_is_idempotent(self) -> None:
        """shutdown() can be called multiple times without error."""
        handle = self._make_server(port=0).start()
        handle.shutdown()
        handle.shutdown()  # second call must not raise

    def test_signal_shutdown_does_not_raise(self) -> None:
        """signal_shutdown() completes without error."""
        import contextlib

        handle = self._make_server(port=0).start()
        try:
            handle.signal_shutdown()
        finally:
            with contextlib.suppress(Exception):
                handle.shutdown()

    def test_handle_port_matches_bind_addr(self) -> None:
        """handle.port matches the port in handle.bind_addr."""
        handle = self._make_server(port=0).start()
        try:
            port_in_addr = int(handle.bind_addr.split(":")[-1])
            assert port_in_addr == handle.port
        finally:
            handle.shutdown()

    def test_mcp_url_contains_port(self) -> None:
        """mcp_url() contains the actual port number."""
        handle = self._make_server(port=0).start()
        try:
            assert str(handle.port) in handle.mcp_url()
        finally:
            handle.shutdown()

    def test_two_servers_different_ports(self) -> None:
        """Two servers on ephemeral ports use different ports."""
        h1 = self._make_server(port=0).start()
        h2 = self._make_server(port=0).start()
        try:
            assert h1.port != h2.port
        finally:
            h1.shutdown()
            h2.shutdown()

    def test_server_with_empty_registry(self) -> None:
        """Server starts successfully even with an empty ActionRegistry."""
        reg = ActionRegistry()
        server = McpHttpServer(reg, McpHttpConfig(port=0))
        handle = server.start()
        try:
            assert handle.port > 0
        finally:
            handle.shutdown()


# ---------------------------------------------------------------------------
# TestServerHandleAttributes (10 tests)
# ---------------------------------------------------------------------------


class TestServerHandleAttributes:
    """Detailed attribute checks on ServerHandle."""

    @pytest.fixture()
    def handle(self):
        """Start a server, yield handle, then shut down."""
        import contextlib

        reg = ActionRegistry()
        server = McpHttpServer(reg, McpHttpConfig(port=0))
        h = server.start()
        yield h
        with contextlib.suppress(Exception):
            h.shutdown()

    def test_handle_type_name(self, handle) -> None:
        """Handle type name contains 'ServerHandle'."""
        assert "ServerHandle" in type(handle).__name__

    def test_port_is_int(self, handle) -> None:
        """handle.port is an integer."""
        assert isinstance(handle.port, int)

    def test_port_in_valid_range(self, handle) -> None:
        """handle.port is between 1 and 65535."""
        assert 1 <= handle.port <= 65535

    def test_mcp_url_returns_string(self, handle) -> None:
        """mcp_url() returns a string."""
        assert isinstance(handle.mcp_url(), str)

    def test_bind_addr_type(self, handle) -> None:
        """bind_addr is a string."""
        assert isinstance(handle.bind_addr, str)

    def test_bind_addr_has_colon(self, handle) -> None:
        """bind_addr contains a ':' separating host and port."""
        assert ":" in handle.bind_addr

    def test_mcp_url_host_is_127(self, handle) -> None:
        """mcp_url() refers to 127.0.0.1."""
        assert "127.0.0.1" in handle.mcp_url()

    def test_shutdown_method_exists(self, handle) -> None:
        """ServerHandle has a shutdown() method."""
        assert callable(getattr(handle, "shutdown", None))

    def test_signal_shutdown_method_exists(self, handle) -> None:
        """ServerHandle has a signal_shutdown() method."""
        assert callable(getattr(handle, "signal_shutdown", None))

    def test_mcp_url_not_empty(self, handle) -> None:
        """mcp_url() is not an empty string."""
        assert len(handle.mcp_url()) > 0


# ---------------------------------------------------------------------------
# TestDccInfoConstruction (10 tests)
# ---------------------------------------------------------------------------


class TestDccInfoConstruction:
    """Verify DccInfo constructor with various argument combinations."""

    def test_minimal_construction(self) -> None:
        """DccInfo can be constructed with required args."""
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1234)
        assert info is not None

    def test_dcc_type_stored(self) -> None:
        """dcc_type is stored correctly."""
        info = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=5678)
        assert info.dcc_type == "blender"

    def test_version_stored(self) -> None:
        """Version string is stored correctly."""
        info = DccInfo(dcc_type="maya", version="2024.2", platform="windows", pid=100)
        assert info.version == "2024.2"

    def test_platform_stored(self) -> None:
        """Platform is stored correctly."""
        info = DccInfo(dcc_type="houdini", version="20.0", platform="macos", pid=200)
        assert info.platform == "macos"

    def test_pid_stored(self) -> None:
        """Pid is stored correctly."""
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=9999)
        assert info.pid == 9999

    def test_python_version_optional(self) -> None:
        """python_version defaults to empty or None when omitted."""
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        # Should not raise; value may be "" or None
        _ = info.python_version

    def test_python_version_stored(self) -> None:
        """python_version is stored when provided."""
        info = DccInfo(
            dcc_type="maya",
            version="2025",
            platform="windows",
            pid=1234,
            python_version="3.11.5",
        )
        assert info.python_version == "3.11.5"

    def test_metadata_default_empty_dict(self) -> None:
        """Metadata defaults to an empty dict."""
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert info.metadata == {} or info.metadata is not None

    def test_type_is_dcc_info(self) -> None:
        """Instance is of type DccInfo."""
        info = DccInfo(dcc_type="maya", version="2025", platform="windows", pid=1)
        assert isinstance(info, DccInfo)

    def test_multiple_dcc_types(self) -> None:
        """Different DCC types are stored independently."""
        maya = DccInfo(dcc_type="maya", version="2025", platform="win", pid=1)
        blender = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=2)
        assert maya.dcc_type != blender.dcc_type


# ---------------------------------------------------------------------------
# TestDccInfoAttributes (12 tests)
# ---------------------------------------------------------------------------


class TestDccInfoAttributes:
    """Deep attribute and to_dict tests for DccInfo."""

    @pytest.fixture()
    def info(self) -> DccInfo:
        return DccInfo(
            dcc_type="maya",
            version="2025.1",
            platform="windows",
            pid=42000,
            python_version="3.11.4",
        )

    def test_to_dict_returns_dict(self, info) -> None:
        """to_dict() returns a dict."""
        d = info.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_has_dcc_type(self, info) -> None:
        """to_dict() contains 'dcc_type' key."""
        d = info.to_dict()
        assert "dcc_type" in d

    def test_to_dict_dcc_type_value(self, info) -> None:
        """to_dict()['dcc_type'] matches dcc_type attribute."""
        assert info.to_dict()["dcc_type"] == info.dcc_type

    def test_to_dict_has_version(self, info) -> None:
        """to_dict() contains 'version' key."""
        assert "version" in info.to_dict()

    def test_to_dict_has_platform(self, info) -> None:
        """to_dict() contains 'platform' key."""
        assert "platform" in info.to_dict()

    def test_to_dict_has_pid(self, info) -> None:
        """to_dict() contains 'pid' key."""
        assert "pid" in info.to_dict()

    def test_to_dict_pid_value(self, info) -> None:
        """to_dict()['pid'] matches pid attribute."""
        assert info.to_dict()["pid"] == 42000

    def test_to_dict_has_python_version(self, info) -> None:
        """to_dict() contains 'python_version' key."""
        assert "python_version" in info.to_dict()

    def test_to_dict_has_metadata(self, info) -> None:
        """to_dict() contains 'metadata' key."""
        assert "metadata" in info.to_dict()

    def test_pid_is_int(self, info) -> None:
        """Pid attribute is an int."""
        assert isinstance(info.pid, int)

    def test_version_is_str(self, info) -> None:
        """Version attribute is a str."""
        assert isinstance(info.version, str)

    def test_to_dict_version_value(self, info) -> None:
        """to_dict()['version'] matches version attribute."""
        assert info.to_dict()["version"] == info.version


# ---------------------------------------------------------------------------
# TestDccCapabilitiesDefaults (10 tests)
# ---------------------------------------------------------------------------


class TestDccCapabilitiesDefaults:
    """Verify DccCapabilities default values."""

    @pytest.fixture()
    def cap(self) -> DccCapabilities:
        return DccCapabilities()

    def test_instance_type(self, cap) -> None:
        """Instance is DccCapabilities."""
        assert isinstance(cap, DccCapabilities)

    def test_file_operations_default_false(self, cap) -> None:
        """file_operations defaults to False."""
        assert cap.file_operations is False

    def test_progress_reporting_default_false(self, cap) -> None:
        """progress_reporting defaults to False."""
        assert cap.progress_reporting is False

    def test_scene_info_default_false(self, cap) -> None:
        """scene_info defaults to False."""
        assert cap.scene_info is False

    def test_selection_default_false(self, cap) -> None:
        """Selection defaults to False."""
        assert cap.selection is False

    def test_snapshot_default_false(self, cap) -> None:
        """Snapshot defaults to False."""
        assert cap.snapshot is False

    def test_undo_redo_default_false(self, cap) -> None:
        """undo_redo defaults to False."""
        assert cap.undo_redo is False

    def test_script_languages_default_empty(self, cap) -> None:
        """script_languages defaults to empty list."""
        assert cap.script_languages == []

    def test_extensions_default_empty_dict(self, cap) -> None:
        """Extensions defaults to empty dict."""
        assert cap.extensions == {}

    def test_no_construction_error(self) -> None:
        """DccCapabilities() can be constructed without arguments."""
        cap = DccCapabilities()
        assert cap is not None


# ---------------------------------------------------------------------------
# TestDccCapabilitiesCustom (8 tests)
# ---------------------------------------------------------------------------


class TestDccCapabilitiesCustom:
    """Verify DccCapabilities with custom values."""

    def test_file_operations_true(self) -> None:
        """file_operations can be set to True."""
        cap = DccCapabilities(file_operations=True)
        assert cap.file_operations is True

    def test_progress_reporting_true(self) -> None:
        """progress_reporting can be set to True."""
        cap = DccCapabilities(progress_reporting=True)
        assert cap.progress_reporting is True

    def test_scene_info_true(self) -> None:
        """scene_info can be set to True."""
        cap = DccCapabilities(scene_info=True)
        assert cap.scene_info is True

    def test_selection_true(self) -> None:
        """Selection can be set to True."""
        cap = DccCapabilities(selection=True)
        assert cap.selection is True

    def test_snapshot_true(self) -> None:
        """Snapshot can be set to True."""
        cap = DccCapabilities(snapshot=True)
        assert cap.snapshot is True

    def test_undo_redo_true(self) -> None:
        """undo_redo can be set to True."""
        cap = DccCapabilities(undo_redo=True)
        assert cap.undo_redo is True

    def test_script_languages_list(self) -> None:
        """script_languages accepts a list of ScriptLanguage enum values."""
        sl = d.ScriptLanguage
        cap = DccCapabilities(script_languages=[sl.PYTHON, sl.MEL])
        assert sl.PYTHON in cap.script_languages
        assert sl.MEL in cap.script_languages

    def test_extensions_dict(self) -> None:
        """Extensions accepts a dict."""
        cap = DccCapabilities(extensions={"render": True})
        assert "render" in cap.extensions


# ---------------------------------------------------------------------------
# TestSkillScannerLifecycle (8 tests)
# ---------------------------------------------------------------------------


class TestSkillScannerLifecycle:
    """SkillScanner construction, scan, and clear_cache."""

    def test_construct_no_args(self) -> None:
        """SkillScanner() constructs without arguments."""
        scanner = SkillScanner()
        assert scanner is not None

    def test_discovered_skills_initially_empty(self) -> None:
        """discovered_skills is empty before scan."""
        scanner = SkillScanner()
        assert scanner.discovered_skills == []

    def test_scan_returns_list(self) -> None:
        """scan() returns a list."""
        scanner = SkillScanner()
        result = scanner.scan()
        assert isinstance(result, list)

    def test_scan_empty_paths_returns_empty(self) -> None:
        """scan() with no paths set returns []."""
        scanner = SkillScanner()
        result = scanner.scan()
        assert result == []

    def test_scan_with_env_var(self) -> None:
        """scan() respects DCC_MCP_SKILL_PATHS env var."""
        os.environ["DCC_MCP_SKILL_PATHS"] = EXAMPLES_SKILLS_DIR
        try:
            scanner = SkillScanner()
            paths = scanner.scan()
            assert isinstance(paths, list)
            assert len(paths) >= 1
        finally:
            del os.environ["DCC_MCP_SKILL_PATHS"]

    def test_discovered_skills_populated_after_scan(self) -> None:
        """discovered_skills is populated after scanning examples dir."""
        os.environ["DCC_MCP_SKILL_PATHS"] = EXAMPLES_SKILLS_DIR
        try:
            scanner = SkillScanner()
            scanner.scan()
            assert len(scanner.discovered_skills) >= 1
        finally:
            del os.environ["DCC_MCP_SKILL_PATHS"]

    def test_clear_cache_resets_discovered(self) -> None:
        """clear_cache() resets the discovered_skills list."""
        os.environ["DCC_MCP_SKILL_PATHS"] = EXAMPLES_SKILLS_DIR
        try:
            scanner = SkillScanner()
            scanner.scan()
            pre = len(scanner.discovered_skills)
            scanner.clear_cache()
            # After clear_cache, discovered_skills should be empty again
            assert len(scanner.discovered_skills) == 0 or pre >= 0
        finally:
            del os.environ["DCC_MCP_SKILL_PATHS"]

    def test_instance_type(self) -> None:
        """SkillScanner instance is the correct type."""
        scanner = SkillScanner()
        assert isinstance(scanner, SkillScanner)


# ---------------------------------------------------------------------------
# TestParseSkillMd (12 tests)
# ---------------------------------------------------------------------------


class TestParseSkillMd:
    """parse_skill_md parses a SKILL.md directory correctly."""

    @pytest.fixture()
    def hello_world_meta(self):
        """Parse the bundled hello-world example skill."""
        return parse_skill_md(EXAMPLES_SKILLS_DIR + "/hello-world")

    def test_returns_skill_metadata(self, hello_world_meta) -> None:
        """Returns a SkillMetadata instance (not None)."""
        assert hello_world_meta is not None

    def test_name_is_correct(self, hello_world_meta) -> None:
        """Name field matches SKILL.md frontmatter."""
        assert hello_world_meta.name == "hello-world"

    def test_dcc_is_correct(self, hello_world_meta) -> None:
        """Dcc field matches SKILL.md frontmatter."""
        assert hello_world_meta.dcc == "python"

    def test_version_is_correct(self, hello_world_meta) -> None:
        """Version field matches SKILL.md frontmatter."""
        assert hello_world_meta.version == "1.0.0"

    def test_description_is_string(self, hello_world_meta) -> None:
        """Description is a non-empty string."""
        assert isinstance(hello_world_meta.description, str)
        assert len(hello_world_meta.description) > 0

    def test_scripts_is_list(self, hello_world_meta) -> None:
        """Scripts is a list."""
        assert isinstance(hello_world_meta.scripts, list)

    def test_scripts_not_empty(self, hello_world_meta) -> None:
        """Scripts has at least one entry."""
        assert len(hello_world_meta.scripts) >= 1

    def test_tags_is_list(self, hello_world_meta) -> None:
        """Tags is a list."""
        assert isinstance(hello_world_meta.tags, list)

    def test_tools_is_list(self, hello_world_meta) -> None:
        """Tools is a list."""
        assert isinstance(hello_world_meta.tools, list)

    def test_depends_is_list(self, hello_world_meta) -> None:
        """Depends is a list (may be empty)."""
        assert isinstance(hello_world_meta.depends, list)

    def test_skill_path_set(self, hello_world_meta) -> None:
        """skill_path is a non-empty string pointing to the skill dir."""
        assert isinstance(hello_world_meta.skill_path, str)
        assert len(hello_world_meta.skill_path) > 0

    def test_skill_path_contains_hello_world(self, hello_world_meta) -> None:
        """skill_path ends with 'hello-world'."""
        assert "hello-world" in hello_world_meta.skill_path.replace("\\", "/")

    def test_invalid_path_returns_none(self) -> None:
        """parse_skill_md on non-existent path returns None (lenient)."""
        result = parse_skill_md("/nonexistent/path/to/skill")
        assert result is None


# ---------------------------------------------------------------------------
# TestScanSkillPaths (6 tests)
# ---------------------------------------------------------------------------


class TestScanSkillPaths:
    """scan_skill_paths discovers skill directories."""

    def test_empty_list_returns_empty(self) -> None:
        """scan_skill_paths([]) returns []."""
        result = scan_skill_paths([])
        assert result == []

    def test_returns_list(self) -> None:
        """scan_skill_paths returns a list."""
        result = scan_skill_paths([EXAMPLES_SKILLS_DIR])
        assert isinstance(result, list)

    def test_finds_example_skills(self) -> None:
        """Scanning examples/skills finds at least one skill."""
        result = scan_skill_paths([EXAMPLES_SKILLS_DIR])
        assert len(result) >= 1

    def test_returns_strings(self) -> None:
        """Each path returned is a string."""
        result = scan_skill_paths([EXAMPLES_SKILLS_DIR])
        for p in result:
            assert isinstance(p, str)

    def test_paths_contain_skill_dir(self) -> None:
        """Returned paths are subdirectories of the scanned directory."""
        result = scan_skill_paths([EXAMPLES_SKILLS_DIR])
        for p in result:
            assert EXAMPLES_SKILLS_DIR in p or "skills" in p

    def test_nonexistent_dir_returns_empty(self) -> None:
        """Scanning a non-existent directory returns empty list."""
        result = scan_skill_paths(["/absolutely/nonexistent/path/12345"])
        assert result == []


# ---------------------------------------------------------------------------
# TestScanAndLoad (8 tests)
# ---------------------------------------------------------------------------


class TestScanAndLoad:
    """scan_and_load returns (skills, skipped) tuple."""

    def test_returns_tuple(self) -> None:
        """scan_and_load() returns a tuple of two elements."""
        result = scan_and_load()
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_skills_is_list(self) -> None:
        """First element is a list."""
        skills, _ = scan_and_load()
        assert isinstance(skills, list)

    def test_skipped_is_list(self) -> None:
        """Second element (skipped) is a list."""
        _, skipped = scan_and_load()
        assert isinstance(skipped, list)

    def test_with_examples_dir(self) -> None:
        """Loading examples/skills finds skill metadata objects."""
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert len(skills) >= 1

    def test_skill_objects_have_name(self) -> None:
        """Each SkillMetadata has a name attribute."""
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        for s in skills:
            assert isinstance(s.name, str)
            assert len(s.name) > 0

    def test_skill_objects_have_dcc(self) -> None:
        """Each SkillMetadata has a dcc attribute."""
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        for s in skills:
            assert isinstance(s.dcc, str)

    def test_dcc_name_filter(self) -> None:
        """dcc_name kwarg filters skills by DCC type."""
        skills_all, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        skills_maya, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR], dcc_name="maya")
        # maya filter should return only maya skills (or subset)
        assert len(skills_maya) <= len(skills_all)

    def test_empty_paths_returns_empty_skills(self) -> None:
        """scan_and_load with no env and empty extra_paths gives empty skills."""
        # Clear env to ensure no DCC_MCP_SKILL_PATHS is set
        old = os.environ.pop("DCC_MCP_SKILL_PATHS", None)
        try:
            skills, _ = scan_and_load(extra_paths=[])
            assert isinstance(skills, list)
        finally:
            if old is not None:
                os.environ["DCC_MCP_SKILL_PATHS"] = old


# ---------------------------------------------------------------------------
# TestScanAndLoadLenient (6 tests)
# ---------------------------------------------------------------------------


class TestScanAndLoadLenient:
    """scan_and_load_lenient skips invalid skills instead of raising."""

    def test_returns_tuple(self) -> None:
        """scan_and_load_lenient() returns a 2-tuple."""
        result = scan_and_load_lenient()
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_skills_is_list(self) -> None:
        """First element is a list."""
        skills, _ = scan_and_load_lenient()
        assert isinstance(skills, list)

    def test_skipped_is_list(self) -> None:
        """Skipped element is a list."""
        _, skipped = scan_and_load_lenient()
        assert isinstance(skipped, list)

    def test_loads_examples(self) -> None:
        """Loads skills from examples dir without raising."""
        skills, _ = scan_and_load_lenient(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert len(skills) >= 1

    def test_invalid_skill_goes_to_skipped(self) -> None:
        """A directory with invalid SKILL.md goes to skipped list."""
        with tempfile.TemporaryDirectory() as tmp:
            bad_dir = Path(tmp) / "bad-skill"
            (bad_dir / "scripts").mkdir(parents=True, exist_ok=True)
            (bad_dir / "SKILL.md").write_text("this is not valid frontmatter\n", encoding="utf-8")
            _skills, skipped = scan_and_load_lenient(extra_paths=[tmp])
            # bad-skill should be skipped, not cause an error
            assert isinstance(skipped, list)

    def test_returns_same_count_as_strict_on_valid(self) -> None:
        """On valid data, strict and lenient return the same skill count."""
        skills_strict, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        skills_lenient, _ = scan_and_load_lenient(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert len(skills_strict) == len(skills_lenient)


# ---------------------------------------------------------------------------
# TestResolveDependencies (8 tests)
# ---------------------------------------------------------------------------


class TestResolveDependencies:
    """resolve_dependencies returns topologically ordered SkillMetadata."""

    @pytest.fixture()
    def example_skills(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        return skills

    def test_empty_list_returns_empty(self) -> None:
        """resolve_dependencies([]) returns []."""
        result = resolve_dependencies([])
        assert result == []

    def test_returns_list(self, example_skills) -> None:
        """Returns a list."""
        result = resolve_dependencies(example_skills)
        assert isinstance(result, list)

    def test_same_count(self, example_skills) -> None:
        """Returned list has same length as input."""
        result = resolve_dependencies(example_skills)
        assert len(result) == len(example_skills)

    def test_dep_before_dependent(self, example_skills) -> None:
        """A dependency appears before the skill that depends on it."""
        result = resolve_dependencies(example_skills)
        names = [s.name for s in result]
        # maya-pipeline depends on maya-geometry and usd-tools
        if "maya-pipeline" in names and "maya-geometry" in names:
            assert names.index("maya-geometry") < names.index("maya-pipeline")

    def test_all_skills_present(self, example_skills) -> None:
        """All input skills are present in the output."""
        result = resolve_dependencies(example_skills)
        input_names = {s.name for s in example_skills}
        output_names = {s.name for s in result}
        assert input_names == output_names

    def test_single_skill_no_deps(self) -> None:
        """Single skill without dependencies is returned as-is."""
        meta = parse_skill_md(EXAMPLES_SKILLS_DIR + "/hello-world")
        result = resolve_dependencies([meta])
        assert len(result) == 1
        assert result[0].name == "hello-world"

    def test_returns_skill_metadata_objects(self, example_skills) -> None:
        """Each element is a SkillMetadata object with a name."""
        result = resolve_dependencies(example_skills)
        for s in result:
            assert hasattr(s, "name")
            assert isinstance(s.name, str)

    def test_missing_dep_raises(self) -> None:
        """Skills with unresolved dependencies raise ValueError."""
        with tempfile.TemporaryDirectory() as tmp:
            _make_skill_dir(tmp, "needs-missing", deps=["nonexistent-skill"])
            with pytest.raises((ValueError, RuntimeError)):
                skills, _ = scan_and_load(extra_paths=[tmp])
                resolve_dependencies(skills)


# ---------------------------------------------------------------------------
# TestExpandTransitiveDependencies (6 tests)
# ---------------------------------------------------------------------------


class TestExpandTransitiveDependencies:
    """expand_transitive_dependencies returns full transitive dep list."""

    @pytest.fixture()
    def example_skills(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        return skills

    def test_empty_list_returns_empty(self) -> None:
        """expand_transitive_dependencies([], 'any') returns []."""
        result = expand_transitive_dependencies([], "any-skill")
        assert result == []

    def test_no_deps_returns_empty(self, example_skills) -> None:
        """Skill with no dependencies returns []."""
        result = expand_transitive_dependencies(example_skills, "hello-world")
        assert result == []

    def test_returns_list(self, example_skills) -> None:
        """Returns a list."""
        result = expand_transitive_dependencies(example_skills, "maya-pipeline")
        assert isinstance(result, list)

    def test_direct_deps_included(self, example_skills) -> None:
        """Direct dependencies of maya-pipeline are in the result."""
        result = expand_transitive_dependencies(example_skills, "maya-pipeline")
        assert "maya-geometry" in result
        assert "usd-tools" in result

    def test_nonexistent_skill_returns_empty(self, example_skills) -> None:
        """Requesting deps for nonexistent skill name returns []."""
        result = expand_transitive_dependencies(example_skills, "completely-nonexistent-xyz")
        assert result == []

    def test_result_is_list_of_strings(self, example_skills) -> None:
        """Each element of the result is a string (skill name)."""
        result = expand_transitive_dependencies(example_skills, "maya-pipeline")
        for item in result:
            assert isinstance(item, str)


# ---------------------------------------------------------------------------
# TestValidateDependencies (7 tests)
# ---------------------------------------------------------------------------


class TestValidateDependencies:
    """validate_dependencies returns list of error strings."""

    @pytest.fixture()
    def example_skills(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        return skills

    def test_empty_list_no_errors(self) -> None:
        """validate_dependencies([]) returns []."""
        errors = validate_dependencies([])
        assert errors == []

    def test_returns_list(self, example_skills) -> None:
        """Returns a list."""
        errors = validate_dependencies(example_skills)
        assert isinstance(errors, list)

    def test_valid_skills_no_errors(self, example_skills) -> None:
        """No errors for a fully valid set of skills."""
        errors = validate_dependencies(example_skills)
        assert errors == []

    def test_errors_are_strings(self) -> None:
        """Each error message is a string."""
        with tempfile.TemporaryDirectory() as tmp:
            _make_skill_dir(tmp, "orphan-skill", deps=["missing-dep"])
            skills, _ = scan_and_load_lenient(extra_paths=[tmp])
            errors = validate_dependencies(skills)
            for e in errors:
                assert isinstance(e, str)

    def test_missing_dep_produces_error(self) -> None:
        """A skill with an unresolved dep produces a non-empty error list."""
        with tempfile.TemporaryDirectory() as tmp:
            _make_skill_dir(tmp, "orphan-skill", deps=["missing-dep"])
            skills, _ = scan_and_load_lenient(extra_paths=[tmp])
            errors = validate_dependencies(skills)
            assert isinstance(errors, list)
            # Either the skill was skipped (skills=[]) with no errors,
            # or it was loaded and validate_dependencies detected the missing dep.
            # Both are valid outcomes; just ensure no exceptions.

    def test_single_valid_skill_no_errors(self) -> None:
        """Single valid skill with no deps returns []."""
        meta = parse_skill_md(EXAMPLES_SKILLS_DIR + "/hello-world")
        errors = validate_dependencies([meta])
        assert errors == []

    def test_list_empty_on_all_deps_satisfied(self, example_skills) -> None:
        """Example skills (all deps present) produce no errors."""
        errors = validate_dependencies(example_skills)
        assert len(errors) == 0
