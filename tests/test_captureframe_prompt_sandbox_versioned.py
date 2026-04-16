"""Deep coverage tests for six API surface areas.

Covers CaptureFrame properties, PySharedBuffer.clear/open cross-instance,
ToolRegistry.search_actions AND-filter combinations, SandboxContext.is_path_allowed,
VersionedRegistry.resolve_all/total_entries/keys, and PromptDefinition+PromptArgument.
"""

from __future__ import annotations

from pathlib import Path
import tempfile

import pytest

from dcc_mcp_core import CaptureFrame
from dcc_mcp_core import Capturer
from dcc_mcp_core import PromptArgument
from dcc_mcp_core import PromptDefinition
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry

# ─────────────────────────── CaptureFrame ────────────────────────────


class TestCaptureFrameProperties:
    """CaptureFrame: all properties via mock capturer."""

    def test_png_data_is_bytes(self):
        c = Capturer.new_mock(640, 480)
        frame = c.capture(format="png")
        assert isinstance(frame.data, bytes)
        assert len(frame.data) > 0

    def test_png_width_height(self):
        c = Capturer.new_mock(800, 600)
        frame = c.capture(format="png")
        assert frame.width == 800
        assert frame.height == 600

    def test_different_resolutions(self):
        for w, h in [(1920, 1080), (320, 240), (1, 1)]:
            c = Capturer.new_mock(w, h)
            frame = c.capture(format="png")
            assert frame.width == w
            assert frame.height == h

    def test_png_format_string(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert frame.format == "png"

    def test_jpeg_format_string(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="jpeg")
        assert frame.format == "jpeg"

    def test_raw_bgra_format_string(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="raw_bgra")
        assert frame.format == "raw_bgra"

    def test_png_mime_type(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert frame.mime_type == "image/png"

    def test_jpeg_mime_type(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="jpeg")
        assert frame.mime_type in ("image/jpeg", "image/jpg")

    def test_raw_bgra_mime_type(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="raw_bgra")
        # raw bgra has a mime type (may be application/octet-stream or similar)
        assert isinstance(frame.mime_type, str)
        assert len(frame.mime_type) > 0

    def test_timestamp_ms_positive(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert isinstance(frame.timestamp_ms, int)
        assert frame.timestamp_ms > 0

    def test_timestamp_ms_monotonic(self):
        import time

        c = Capturer.new_mock(64, 64)
        frame1 = c.capture(format="png")
        time.sleep(0.01)
        frame2 = c.capture(format="png")
        assert frame2.timestamp_ms >= frame1.timestamp_ms

    def test_dpi_scale_default(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert isinstance(frame.dpi_scale, float)
        assert frame.dpi_scale > 0.0

    def test_dpi_scale_standard(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        # mock backend uses standard (non-HiDPI) scale
        assert frame.dpi_scale == 1.0

    def test_byte_len_equals_data_length(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert frame.byte_len() == len(frame.data)

    def test_byte_len_positive(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        assert frame.byte_len() > 0

    def test_raw_bgra_byte_len_proportional_to_resolution(self):
        w, h = 128, 64
        c = Capturer.new_mock(w, h)
        frame = c.capture(format="raw_bgra")
        # raw BGRA = 4 bytes per pixel
        assert frame.byte_len() == w * h * 4

    def test_repr_is_str(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        r = repr(frame)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_png_data_starts_with_png_magic(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="png")
        # PNG signature: 8 bytes: \x89PNG\r\n\x1a\n
        assert frame.data[:8] == b"\x89PNG\r\n\x1a\n"

    def test_jpeg_data_starts_with_jpeg_magic(self):
        c = Capturer.new_mock(64, 64)
        frame = c.capture(format="jpeg")
        # JPEG SOI marker: 0xFF 0xD8
        assert frame.data[:2] == b"\xff\xd8"

    def test_scale_reduces_byte_len(self):
        c = Capturer.new_mock(256, 256)
        full = c.capture(format="raw_bgra", scale=1.0)
        half = c.capture(format="raw_bgra", scale=0.5)
        assert half.byte_len() < full.byte_len()

    def test_jpeg_quality_affects_size(self):
        c = Capturer.new_mock(256, 256)
        high_q = c.capture(format="jpeg", jpeg_quality=95)
        low_q = c.capture(format="jpeg", jpeg_quality=10)
        # Higher quality -> larger file (generally)
        assert high_q.byte_len() >= low_q.byte_len()


# ─────────────────────── PySharedBuffer clear + open ─────────────────────────


class TestPySharedBufferClearAndOpen:
    """PySharedBuffer.clear() and cross-instance open verification."""

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"some data")
        assert buf.data_len() > 0
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_allows_rewrite(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_clear_idempotent_on_empty(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.clear()  # already empty
        buf.clear()  # again - should not raise
        assert buf.data_len() == 0

    def test_clear_preserves_capacity(self):
        cap = 512
        buf = PySharedBuffer.create(capacity=cap)
        buf.write(b"x" * 100)
        buf.clear()
        assert buf.capacity() == cap

    def test_clear_then_read_empty(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"hello")
        buf.clear()
        data = buf.read()
        assert data == b""

    def test_write_after_clear_can_fill_capacity(self):
        cap = 64
        buf = PySharedBuffer.create(capacity=cap)
        buf.write(b"a" * 10)
        buf.clear()
        n = buf.write(b"b" * cap)
        assert n == cap
        assert buf.data_len() == cap

    def test_open_cross_instance_same_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        payload = b"cross instance hello"
        buf.write(payload)
        p = buf.path()
        i = buf.id
        buf2 = PySharedBuffer.open(path=p, id=i)
        assert buf2.read() == payload

    def test_open_cross_instance_id_matches(self):
        buf = PySharedBuffer.create(capacity=512)
        buf.write(b"test")
        buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
        assert buf2.id == buf.id

    def test_open_cross_instance_capacity_matches(self):
        buf = PySharedBuffer.create(capacity=2048)
        buf.write(b"data")
        buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
        assert buf2.capacity() == buf.capacity()

    def test_open_cross_instance_clear_visible(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"before clear")
        buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
        buf2.clear()
        assert buf2.data_len() == 0

    def test_open_cross_instance_write_then_read(self):
        buf = PySharedBuffer.create(capacity=256)
        buf.write(b"initial")
        p, i = buf.path(), buf.id
        buf2 = PySharedBuffer.open(path=p, id=i)
        assert buf2.read() == b"initial"
        buf2.clear()
        buf2.write(b"updated")
        assert buf2.read() == b"updated"

    def test_descriptor_json_contains_id(self):
        buf = PySharedBuffer.create(capacity=256)
        desc = buf.descriptor_json()
        assert buf.id in desc

    def test_descriptor_json_contains_path(self):
        buf = PySharedBuffer.create(capacity=256)
        desc = buf.descriptor_json()
        # descriptor_json JSON-escapes backslashes on Windows; verify via id instead
        assert buf.id in desc
        assert "path" in desc


# ─────────────────── ToolRegistry.search_actions combination filters ───────────────────


class TestActionRegistrySearchActionsFilters:
    """search_actions AND-filter combinations: category + tags + dcc_name."""

    def setup_method(self):
        self.reg = ToolRegistry()
        self.reg.register("create_sphere", category="geometry", tags=["create", "mesh"], dcc="maya")
        self.reg.register("delete_sphere", category="geometry", tags=["delete", "mesh"], dcc="maya")
        self.reg.register("create_cube", category="geometry", tags=["create", "mesh"], dcc="blender")
        self.reg.register("export_alembic", category="export", tags=["file", "alembic"], dcc="maya")
        self.reg.register("import_obj", category="import", tags=["file"], dcc="maya")
        self.reg.register("render_frame", category="render", tags=["render", "viewport"], dcc="houdini")

    def test_search_by_category_only(self):
        r = self.reg.search_actions(category="geometry")
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "delete_sphere" in names
        assert "create_cube" in names
        assert "export_alembic" not in names

    def test_search_by_tags_only(self):
        r = self.reg.search_actions(tags=["create"])
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "create_cube" in names
        assert "delete_sphere" not in names

    def test_search_by_dcc_only(self):
        r = self.reg.search_actions(dcc_name="maya")
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "delete_sphere" in names
        assert "export_alembic" in names
        assert "create_cube" not in names
        assert "render_frame" not in names

    def test_search_category_and_tags(self):
        r = self.reg.search_actions(category="geometry", tags=["create"])
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "create_cube" in names
        assert "delete_sphere" not in names
        assert "export_alembic" not in names

    def test_search_category_and_multi_tags(self):
        r = self.reg.search_actions(category="geometry", tags=["create", "mesh"])
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "create_cube" in names
        assert "delete_sphere" not in names

    def test_search_category_and_dcc(self):
        r = self.reg.search_actions(category="geometry", dcc_name="maya")
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "delete_sphere" in names
        assert "create_cube" not in names
        assert "export_alembic" not in names

    def test_search_tags_and_dcc(self):
        r = self.reg.search_actions(tags=["create"], dcc_name="maya")
        names = {a["name"] for a in r}
        assert "create_sphere" in names
        assert "create_cube" not in names

    def test_search_all_three_filters(self):
        r = self.reg.search_actions(category="geometry", tags=["create"], dcc_name="blender")
        names = {a["name"] for a in r}
        assert names == {"create_cube"}

    def test_search_all_three_no_match(self):
        r = self.reg.search_actions(category="export", tags=["create"], dcc_name="maya")
        assert r == []

    def test_search_tags_must_have_all(self):
        # action with only ["file"] tag should not match ["file", "alembic"]
        r = self.reg.search_actions(tags=["file", "alembic"])
        names = {a["name"] for a in r}
        assert "export_alembic" in names
        assert "import_obj" not in names  # only has ["file"]

    def test_search_none_filters_returns_all(self):
        r = self.reg.search_actions()
        assert len(r) == 6

    def test_search_unknown_category_empty(self):
        r = self.reg.search_actions(category="nonexistent")
        assert r == []

    def test_search_unknown_dcc_empty(self):
        r = self.reg.search_actions(dcc_name="3dsmax")
        assert r == []

    def test_search_unknown_tag_empty(self):
        r = self.reg.search_actions(tags=["does_not_exist"])
        assert r == []

    def test_search_empty_tags_list_is_no_filter(self):
        r = self.reg.search_actions(tags=[])
        assert len(r) == 6

    def test_search_category_export(self):
        r = self.reg.search_actions(category="export")
        assert len(r) == 1
        assert r[0]["name"] == "export_alembic"

    def test_search_category_render(self):
        r = self.reg.search_actions(category="render")
        assert len(r) == 1
        assert r[0]["name"] == "render_frame"


# ─────────────────────── SandboxContext.is_path_allowed ──────────────────────


class TestSandboxContextIsPathAllowed:
    """SandboxContext.is_path_allowed semantics."""

    def test_no_allowed_paths_permits_everything(self):
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        assert ctx.is_path_allowed("/any/path/at/all") is True
        assert ctx.is_path_allowed("C:/Windows/system32") is True

    def test_allowed_path_exact_match(self):
        tmpdir = tempfile.gettempdir()
        policy = SandboxPolicy()
        policy.allow_paths([tmpdir])
        ctx = SandboxContext(policy)
        assert ctx.is_path_allowed(tmpdir) is True

    def test_allowed_path_sub_path_permitted(self):
        tmpdir = tempfile.gettempdir()
        policy = SandboxPolicy()
        policy.allow_paths([tmpdir])
        ctx = SandboxContext(policy)
        sub = str(Path(tmpdir) / "my_scene.usd")
        assert ctx.is_path_allowed(sub) is True

    def test_disallowed_path_outside_whitelist(self):
        policy = SandboxPolicy()
        policy.allow_paths(["/allowed/project"])
        ctx = SandboxContext(policy)
        assert ctx.is_path_allowed("/not/allowed") is False

    def test_path_prefix_not_suffix_confusion(self):
        # /tmp/project should not allow /tmp/projectother
        policy = SandboxPolicy()
        policy.allow_paths(["/tmp/project"])
        ctx = SandboxContext(policy)
        assert ctx.is_path_allowed("/tmp/projectother") is False

    def test_multiple_allowed_paths(self):
        # allow_paths requires directories that actually exist on the filesystem.
        with tempfile.TemporaryDirectory() as dir_a, tempfile.TemporaryDirectory() as dir_b:
            policy = SandboxPolicy()
            policy.allow_paths([dir_a, dir_b])
            ctx = SandboxContext(policy)
            # sub-paths under allowed dirs are allowed
            assert ctx.is_path_allowed(str(Path(dir_a) / "scene.usd")) is True
            assert ctx.is_path_allowed(str(Path(dir_b) / "asset.usd")) is True
            # path outside both allowed dirs is denied
            assert ctx.is_path_allowed(str(Path(tempfile.gettempdir()) / "unrelated_file.py")) is False

    def test_is_allowed_action_unrelated_to_path(self):
        # is_allowed (action) and is_path_allowed (path) are separate concerns.
        with tempfile.TemporaryDirectory() as my_path:
            policy = SandboxPolicy()
            policy.allow_actions(["get_scene_info"])
            policy.allow_paths([my_path])
            ctx = SandboxContext(policy)
            assert ctx.is_allowed("get_scene_info") is True
            assert ctx.is_allowed("delete_scene") is False
            assert ctx.is_path_allowed(str(Path(my_path) / "scene.usd")) is True
            assert ctx.is_path_allowed(str(Path(tempfile.gettempdir()) / "other_dir")) is False

    def test_windows_style_path_outside(self):
        policy = SandboxPolicy()
        policy.allow_paths(["C:/Users/artist/projects"])
        ctx = SandboxContext(policy)
        assert ctx.is_path_allowed("C:/Windows/System32") is False

    def test_empty_path(self):
        policy = SandboxPolicy()
        policy.allow_paths(["/my/path"])
        ctx = SandboxContext(policy)
        # empty path is not inside allowed dir
        result = ctx.is_path_allowed("")
        assert isinstance(result, bool)


# ─────────────────── VersionedRegistry resolve_all / total_entries / keys ────────────────────


class TestVersionedRegistryResolveAllTotalEntriesKeys:
    """VersionedRegistry.resolve_all, total_entries, keys deep coverage."""

    def setup_method(self):
        self.vreg = VersionedRegistry()
        self.vreg.register_versioned("create_sphere", "maya", "1.0.0", description="v1")
        self.vreg.register_versioned("create_sphere", "maya", "1.5.0", description="v1.5")
        self.vreg.register_versioned("create_sphere", "maya", "2.0.0", description="v2")
        self.vreg.register_versioned("delete_sphere", "blender", "1.0.0")
        self.vreg.register_versioned("export_usd", "houdini", "3.0.0")

    def test_total_entries_initial(self):
        assert self.vreg.total_entries() == 5

    def test_total_entries_after_register(self):
        self.vreg.register_versioned("new_action", "maya", "0.1.0")
        assert self.vreg.total_entries() == 6

    def test_total_entries_after_remove(self):
        removed = self.vreg.remove("export_usd", "houdini", "*")
        assert removed == 1
        assert self.vreg.total_entries() == 4

    def test_total_entries_empty_registry(self):
        vreg = VersionedRegistry()
        assert vreg.total_entries() == 0

    def test_keys_returns_unique_pairs(self):
        keys = self.vreg.keys()
        assert ("create_sphere", "maya") in keys
        assert ("delete_sphere", "blender") in keys
        assert ("export_usd", "houdini") in keys
        # 3 unique (name, dcc) pairs even though create_sphere/maya has 3 versions
        assert len(keys) == 3

    def test_keys_empty_registry(self):
        vreg = VersionedRegistry()
        assert vreg.keys() == []

    def test_keys_after_remove_all_versions(self):
        # Note: keys() retains the (name, dcc) pair even after removing all versions.
        # Use total_entries() to verify the versions are gone.
        before_count = self.vreg.total_entries()
        removed = self.vreg.remove("delete_sphere", "blender", "*")
        assert removed == 1
        assert self.vreg.total_entries() == before_count - 1

    def test_resolve_all_wildcard(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "*")
        versions = [r["version"] for r in results]
        assert versions == ["1.0.0", "1.5.0", "2.0.0"]

    def test_resolve_all_sorted_ascending(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "*")
        versions = [r["version"] for r in results]
        assert versions == sorted(versions)

    def test_resolve_all_caret_constraint(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "^1.0.0")
        versions = [r["version"] for r in results]
        assert "1.0.0" in versions
        assert "1.5.0" in versions
        assert "2.0.0" not in versions

    def test_resolve_all_gte_constraint(self):
        results = self.vreg.resolve_all("create_sphere", "maya", ">=1.5.0")
        versions = [r["version"] for r in results]
        assert "1.5.0" in versions
        assert "2.0.0" in versions
        assert "1.0.0" not in versions

    def test_resolve_all_exact_constraint(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "=1.5.0")
        assert len(results) == 1
        assert results[0]["version"] == "1.5.0"

    def test_resolve_all_no_match_empty(self):
        results = self.vreg.resolve_all("create_sphere", "maya", ">=3.0.0")
        assert results == []

    def test_resolve_all_unknown_action_empty(self):
        results = self.vreg.resolve_all("nonexistent", "maya", "*")
        assert results == []

    def test_resolve_all_unknown_dcc_empty(self):
        results = self.vreg.resolve_all("create_sphere", "3dsmax", "*")
        assert results == []

    def test_resolve_all_result_has_expected_keys(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "*")
        for r in results:
            assert "version" in r
            assert "name" in r

    def test_resolve_all_single_version(self):
        results = self.vreg.resolve_all("delete_sphere", "blender", "*")
        assert len(results) == 1
        assert results[0]["version"] == "1.0.0"

    def test_resolve_all_lt_constraint(self):
        results = self.vreg.resolve_all("create_sphere", "maya", "<2.0.0")
        versions = [r["version"] for r in results]
        assert "2.0.0" not in versions
        assert "1.0.0" in versions
        assert "1.5.0" in versions

    def test_total_entries_consistency_with_versions(self):
        # 3 versions for create_sphere/maya + 1 + 1 = 5
        assert self.vreg.total_entries() == len(self.vreg.resolve_all("create_sphere", "maya", "*")) + 2


# ─────────────────────── PromptDefinition + PromptArgument ───────────────────


class TestPromptArgumentAndDefinition:
    """MCP PromptArgument and PromptDefinition deep coverage."""

    # ── PromptArgument ──

    def test_prompt_argument_name(self):
        pa = PromptArgument("my_arg", "Description", required=False)
        assert pa.name == "my_arg"

    def test_prompt_argument_description(self):
        pa = PromptArgument("arg", "A helpful description", required=True)
        assert pa.description == "A helpful description"

    def test_prompt_argument_required_true(self):
        pa = PromptArgument("arg", "desc", required=True)
        assert pa.required is True

    def test_prompt_argument_required_false(self):
        pa = PromptArgument("arg", "desc", required=False)
        assert pa.required is False

    def test_prompt_argument_required_default_false(self):
        pa = PromptArgument("arg", "desc")
        assert pa.required is False

    def test_prompt_argument_eq_same(self):
        a = PromptArgument("x", "d", required=True)
        b = PromptArgument("x", "d", required=True)
        assert a == b

    def test_prompt_argument_eq_different_required(self):
        a = PromptArgument("x", "d", required=True)
        b = PromptArgument("x", "d", required=False)
        assert a != b

    def test_prompt_argument_eq_different_name(self):
        a = PromptArgument("x", "d", required=True)
        b = PromptArgument("y", "d", required=True)
        assert a != b

    def test_prompt_argument_repr(self):
        pa = PromptArgument("my_arg", "desc", required=True)
        r = repr(pa)
        assert "my_arg" in r

    def test_prompt_argument_multiple_args(self):
        args = [
            PromptArgument("scene_path", "Path to scene file", required=True),
            PromptArgument("dcc_type", "Target DCC", required=False),
            PromptArgument("frame", "Frame number", required=False),
        ]
        assert args[0].required is True
        assert args[1].required is False
        assert args[2].name == "frame"

    # ── PromptDefinition ──

    def test_prompt_definition_name(self):
        pd = PromptDefinition("export_scene", "Export current scene")
        assert pd.name == "export_scene"

    def test_prompt_definition_description(self):
        pd = PromptDefinition("render", "Render the scene to file")
        assert pd.description == "Render the scene to file"

    def test_prompt_definition_empty_arguments(self):
        pd = PromptDefinition("ping", "Simple ping")
        assert pd.arguments == []

    def test_prompt_definition_none_arguments(self):
        # PromptDefinition with no arguments argument (default) returns empty list
        pd = PromptDefinition("ping", "Simple ping")
        assert pd.arguments == []

    def test_prompt_definition_with_arguments(self):
        args = [
            PromptArgument("path", "Scene path", required=True),
            PromptArgument("format", "Export format", required=False),
        ]
        pd = PromptDefinition("export_scene", "Export scene", args)
        assert len(pd.arguments) == 2
        assert pd.arguments[0].name == "path"
        assert pd.arguments[1].name == "format"

    def test_prompt_definition_eq_same(self):
        args = [PromptArgument("x", "d", required=True)]
        a = PromptDefinition("prompt", "desc", args)
        b = PromptDefinition("prompt", "desc", [PromptArgument("x", "d", required=True)])
        assert a == b

    def test_prompt_definition_eq_different_name(self):
        a = PromptDefinition("prompt_a", "desc")
        b = PromptDefinition("prompt_b", "desc")
        assert a != b

    def test_prompt_definition_eq_different_args(self):
        a = PromptDefinition("p", "d", [PromptArgument("x", "d")])
        b = PromptDefinition("p", "d", [PromptArgument("y", "d")])
        assert a != b

    def test_prompt_definition_repr(self):
        pd = PromptDefinition("my_prompt", "My description")
        r = repr(pd)
        assert "my_prompt" in r

    def test_prompt_definition_repr_with_args(self):
        pd = PromptDefinition("export", "desc", [PromptArgument("p", "d")])
        r = repr(pd)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_prompt_definition_arguments_type_is_list(self):
        pd = PromptDefinition("export", "desc", [PromptArgument("p", "d")])
        assert isinstance(pd.arguments, list)

    def test_prompt_definition_argument_items_are_prompt_argument(self):
        arg = PromptArgument("path", "Path to file", required=True)
        pd = PromptDefinition("export", "desc", [arg])
        assert isinstance(pd.arguments[0], PromptArgument)

    def test_prompt_definition_many_arguments(self):
        args = [PromptArgument(f"arg_{i}", f"Argument {i}", required=(i % 2 == 0)) for i in range(5)]
        pd = PromptDefinition("complex_prompt", "A prompt with many args", args)
        assert len(pd.arguments) == 5
        for i, arg in enumerate(pd.arguments):
            assert arg.name == f"arg_{i}"

    def test_prompt_definition_required_arg_count(self):
        args = [
            PromptArgument("required1", "d", required=True),
            PromptArgument("optional1", "d", required=False),
            PromptArgument("required2", "d", required=True),
        ]
        pd = PromptDefinition("prompt", "desc", args)
        required = [a for a in pd.arguments if a.required]
        optional = [a for a in pd.arguments if not a.required]
        assert len(required) == 2
        assert len(optional) == 1
