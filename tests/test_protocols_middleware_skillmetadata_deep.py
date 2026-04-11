"""Deep tests for protocol types, standalone middleware, and SkillMetadata.

Covers:
- ToolDefinition: create / fields / mutation / annotations / repr
- ToolAnnotations: all hint fields / repr
- ResourceDefinition: create / fields / default mime_type / annotations
- ResourceAnnotations: audience / priority / repr
- ResourceTemplateDefinition: create / fields / repr
- PromptDefinition: create / name / description / arguments / repr
- PromptArgument: name / description / required / repr
- LoggingMiddleware: create / log_params / repr
- TimingMiddleware: create / last_elapsed_ms default / repr
- AuditMiddleware: create / records / record_count / repr
- RateLimitMiddleware: create / max_calls / window_ms / call_count / repr
- SkillMetadata: create / defaults / mutation / all fields / repr
"""

from __future__ import annotations

# ---------------------------------------------------------------------------
# ToolDefinition
# ---------------------------------------------------------------------------


class TestToolDefinitionCreate:
    def test_create_basic(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("create_sphere", "Create sphere", "{}")
        assert td.name == "create_sphere"

    def test_description(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "My description", "{}")
        assert td.description == "My description"

    def test_input_schema(self):
        from dcc_mcp_core import ToolDefinition

        schema = '{"type": "object"}'
        td = ToolDefinition("x", "d", schema)
        assert td.input_schema == schema

    def test_output_schema_default_none(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "d", "{}")
        assert td.output_schema is None

    def test_annotations_default_none(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "d", "{}")
        assert td.annotations is None

    def test_repr_is_str(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("create_sphere", "desc", "{}")
        assert isinstance(repr(td), str)

    def test_repr_contains_name(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("my_tool", "desc", "{}")
        assert "my_tool" in repr(td)

    def test_name_mutation(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("old_name", "d", "{}")
        td.name = "new_name"
        assert td.name == "new_name"

    def test_description_mutation(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "old desc", "{}")
        td.description = "new desc"
        assert td.description == "new desc"

    def test_input_schema_mutation(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "d", "{}")
        new_schema = '{"type": "array"}'
        td.input_schema = new_schema
        assert td.input_schema == new_schema

    def test_output_schema_mutation(self):
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "d", "{}")
        td.output_schema = '{"type": "object"}'
        assert td.output_schema == '{"type": "object"}'

    def test_annotations_mutation(self):
        from dcc_mcp_core import ToolAnnotations
        from dcc_mcp_core import ToolDefinition

        td = ToolDefinition("x", "d", "{}")
        ann = ToolAnnotations(
            title="T", read_only_hint=True, destructive_hint=False, idempotent_hint=True, open_world_hint=False
        )
        td.annotations = ann
        assert td.annotations is not None
        assert td.annotations.title == "T"
        assert td.annotations.read_only_hint is True

    def test_with_full_schema(self):
        from dcc_mcp_core import ToolDefinition

        schema = '{"type": "object", "required": ["radius"], "properties": {"radius": {"type": "number"}}}'
        td = ToolDefinition("create_sphere", "Create a sphere", schema)
        assert "radius" in td.input_schema


# ---------------------------------------------------------------------------
# ToolAnnotations
# ---------------------------------------------------------------------------


class TestToolAnnotations:
    def test_create(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=True, destructive_hint=False, idempotent_hint=True, open_world_hint=False
        )
        assert ann is not None

    def test_title(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="My Tool", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.title == "My Tool"

    def test_read_only_hint_true(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=True, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.read_only_hint is True

    def test_read_only_hint_false(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.read_only_hint is False

    def test_destructive_hint_true(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=True, idempotent_hint=False, open_world_hint=False
        )
        assert ann.destructive_hint is True

    def test_destructive_hint_false(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.destructive_hint is False

    def test_idempotent_hint_true(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=True, open_world_hint=False
        )
        assert ann.idempotent_hint is True

    def test_idempotent_hint_false(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.idempotent_hint is False

    def test_open_world_hint_true(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=True
        )
        assert ann.open_world_hint is True

    def test_open_world_hint_false(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=False, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert ann.open_world_hint is False

    def test_repr_is_str(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=True, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert isinstance(repr(ann), str)

    def test_repr_contains_title(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="MyTitle", read_only_hint=True, destructive_hint=False, idempotent_hint=False, open_world_hint=False
        )
        assert "MyTitle" in repr(ann)

    def test_tool_definition_with_annotations(self):
        from dcc_mcp_core import ToolAnnotations
        from dcc_mcp_core import ToolDefinition

        ann = ToolAnnotations(
            title="Safe Read", read_only_hint=True, destructive_hint=False, idempotent_hint=True, open_world_hint=False
        )
        td = ToolDefinition("list_objects", "List scene objects", "{}", annotations=ann)
        assert td.annotations.title == "Safe Read"
        assert td.annotations.read_only_hint is True

    def test_all_hints_true(self):
        from dcc_mcp_core import ToolAnnotations

        ann = ToolAnnotations(
            title="T", read_only_hint=True, destructive_hint=True, idempotent_hint=True, open_world_hint=True
        )
        assert ann.read_only_hint is True
        assert ann.destructive_hint is True
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is True


# ---------------------------------------------------------------------------
# ResourceDefinition
# ---------------------------------------------------------------------------


class TestResourceDefinition:
    def test_create_basic(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///scene.mb", name="scene", description="Main scene")
        assert rd.uri == "file:///scene.mb"

    def test_name(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="my-scene", description="d")
        assert rd.name == "my-scene"

    def test_description(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="n", description="A scene file")
        assert rd.description == "A scene file"

    def test_mime_type_default_text_plain(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="n", description="d")
        assert rd.mime_type == "text/plain"

    def test_mime_type_custom(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="n", description="d", mime_type="application/json")
        assert rd.mime_type == "application/json"

    def test_mime_type_octet_stream(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="n", description="d", mime_type="application/octet-stream")
        assert rd.mime_type == "application/octet-stream"

    def test_annotations_default_none(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="n", description="d")
        assert rd.annotations is None

    def test_with_annotations(self):
        from dcc_mcp_core import ResourceAnnotations
        from dcc_mcp_core import ResourceDefinition

        ann = ResourceAnnotations(audience=["user"], priority=0.9)
        rd = ResourceDefinition(uri="file:///x", name="n", description="d", annotations=ann)
        assert rd.annotations is not None
        assert rd.annotations.audience == ["user"]

    def test_repr_is_str(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="scene", description="d")
        assert isinstance(repr(rd), str)

    def test_repr_contains_name(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///x", name="my-resource", description="d")
        assert "my-resource" in repr(rd)

    def test_repr_contains_uri(self):
        from dcc_mcp_core import ResourceDefinition

        rd = ResourceDefinition(uri="file:///scene.mb", name="n", description="d")
        assert "file:///scene.mb" in repr(rd)


# ---------------------------------------------------------------------------
# ResourceAnnotations
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    def test_create(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=0.5)
        assert ann is not None

    def test_audience_single(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=0.5)
        assert ann.audience == ["user"]

    def test_audience_multiple(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user", "assistant"], priority=0.5)
        assert "user" in ann.audience
        assert "assistant" in ann.audience

    def test_priority_value(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=0.75)
        assert abs(ann.priority - 0.75) < 1e-6

    def test_priority_zero(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=0.0)
        assert ann.priority == 0.0

    def test_priority_one(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=1.0)
        assert ann.priority == 1.0

    def test_repr_is_str(self):
        from dcc_mcp_core import ResourceAnnotations

        ann = ResourceAnnotations(audience=["user"], priority=0.5)
        assert isinstance(repr(ann), str)


# ---------------------------------------------------------------------------
# ResourceTemplateDefinition
# ---------------------------------------------------------------------------


class TestResourceTemplateDefinition:
    def test_create(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://scene/{name}", name="scene-obj", description="d")
        assert rtd is not None

    def test_uri_template(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://scene/{name}", name="n", description="d")
        assert rtd.uri_template == "maya://scene/{name}"

    def test_name(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://x/{y}", name="my-template", description="d")
        assert rtd.name == "my-template"

    def test_description(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://x/{y}", name="n", description="Scene template")
        assert rtd.description == "Scene template"

    def test_mime_type_custom(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(
            uri_template="maya://x/{y}", name="n", description="d", mime_type="application/json"
        )
        assert rtd.mime_type == "application/json"

    def test_repr_is_str(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://x/{y}", name="n", description="d")
        assert isinstance(repr(rtd), str)

    def test_repr_contains_name(self):
        from dcc_mcp_core import ResourceTemplateDefinition

        rtd = ResourceTemplateDefinition(uri_template="maya://x/{y}", name="scene-tmpl", description="d")
        assert "scene-tmpl" in repr(rtd)


# ---------------------------------------------------------------------------
# PromptArgument + PromptDefinition
# ---------------------------------------------------------------------------


class TestPromptArgument:
    def test_create(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("obj_name", "Name of the object", required=True)
        assert arg is not None

    def test_name(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("my_arg", "desc", required=False)
        assert arg.name == "my_arg"

    def test_description(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("x", "My description", required=True)
        assert arg.description == "My description"

    def test_required_true(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("x", "d", required=True)
        assert arg.required is True

    def test_required_false(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("x", "d", required=False)
        assert arg.required is False

    def test_repr_is_str(self):
        from dcc_mcp_core import PromptArgument

        arg = PromptArgument("x", "d", required=True)
        assert isinstance(repr(arg), str)


class TestPromptDefinition:
    def test_create(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        arg = PromptArgument("obj", "Name", required=True)
        pd = PromptDefinition("review_model", "Review a 3D model", [arg])
        assert pd is not None

    def test_name(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        pd = PromptDefinition("my_prompt", "desc", [])
        assert pd.name == "my_prompt"

    def test_description(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        pd = PromptDefinition("p", "My prompt description", [])
        assert pd.description == "My prompt description"

    def test_arguments_empty(self):
        from dcc_mcp_core import PromptDefinition

        pd = PromptDefinition("p", "d", [])
        assert pd.arguments == []

    def test_arguments_single(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        arg = PromptArgument("x", "d", required=True)
        pd = PromptDefinition("p", "d", [arg])
        assert len(pd.arguments) == 1

    def test_arguments_count_two(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        a1 = PromptArgument("name", "Name", required=True)
        a2 = PromptArgument("format", "Format", required=False)
        pd = PromptDefinition("export_model", "Export model", [a1, a2])
        assert len(pd.arguments) == 2

    def test_argument_required_first(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        a1 = PromptArgument("name", "Name", required=True)
        a2 = PromptArgument("format", "Format", required=False)
        pd = PromptDefinition("p", "d", [a1, a2])
        assert pd.arguments[0].required is True

    def test_argument_optional_second(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        a1 = PromptArgument("name", "Name", required=True)
        a2 = PromptArgument("format", "Format", required=False)
        pd = PromptDefinition("p", "d", [a1, a2])
        assert pd.arguments[1].required is False

    def test_repr_is_str(self):
        from dcc_mcp_core import PromptDefinition

        pd = PromptDefinition("review_model", "Review a model", [])
        assert isinstance(repr(pd), str)

    def test_repr_contains_name(self):
        from dcc_mcp_core import PromptDefinition

        pd = PromptDefinition("my_review_prompt", "d", [])
        assert "my_review_prompt" in repr(pd)

    def test_repr_contains_argument_count(self):
        from dcc_mcp_core import PromptArgument
        from dcc_mcp_core import PromptDefinition

        a1 = PromptArgument("x", "d", required=True)
        pd = PromptDefinition("p", "d", [a1])
        r = repr(pd)
        assert "1" in r


# ---------------------------------------------------------------------------
# LoggingMiddleware (standalone)
# ---------------------------------------------------------------------------


class TestLoggingMiddlewareStandalone:
    def test_create_log_params_false(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=False)
        assert lm is not None

    def test_create_log_params_true(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=True)
        assert lm is not None

    def test_log_params_false(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=False)
        assert lm.log_params is False

    def test_log_params_true(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=True)
        assert lm.log_params is True

    def test_repr_is_str(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=False)
        assert isinstance(repr(lm), str)

    def test_repr_contains_log_params(self):
        from dcc_mcp_core import LoggingMiddleware

        lm = LoggingMiddleware(log_params=True)
        assert "log_params" in repr(lm)


# ---------------------------------------------------------------------------
# TimingMiddleware (standalone)
# ---------------------------------------------------------------------------


class TestTimingMiddlewareStandalone:
    def test_create(self):
        from dcc_mcp_core import TimingMiddleware

        tm = TimingMiddleware()
        assert tm is not None

    def test_last_elapsed_ms_unknown_none(self):
        from dcc_mcp_core import TimingMiddleware

        tm = TimingMiddleware()
        assert tm.last_elapsed_ms("unknown_action") is None

    def test_last_elapsed_ms_any_name_none(self):
        from dcc_mcp_core import TimingMiddleware

        tm = TimingMiddleware()
        assert tm.last_elapsed_ms("create_sphere") is None

    def test_repr_is_str(self):
        from dcc_mcp_core import TimingMiddleware

        tm = TimingMiddleware()
        assert isinstance(repr(tm), str)

    def test_multiple_queries_all_none(self):
        from dcc_mcp_core import TimingMiddleware

        tm = TimingMiddleware()
        for name in ["action_a", "action_b", "action_c"]:
            assert tm.last_elapsed_ms(name) is None


# ---------------------------------------------------------------------------
# AuditMiddleware (standalone)
# ---------------------------------------------------------------------------


class TestAuditMiddlewareStandalone:
    def test_create(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        assert am is not None

    def test_records_empty_initially(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        assert am.records() == []

    def test_record_count_zero_initially(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        assert am.record_count() == 0

    def test_records_returns_list(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=True)
        assert isinstance(am.records(), list)

    def test_record_count_is_int(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=True)
        assert isinstance(am.record_count(), int)

    def test_records_for_action_empty(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        result = am.records_for_action("create_sphere")
        assert result == []

    def test_clear_on_empty(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        am.clear()
        assert am.record_count() == 0

    def test_repr_is_str(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        assert isinstance(repr(am), str)

    def test_repr_contains_count(self):
        from dcc_mcp_core import AuditMiddleware

        am = AuditMiddleware(record_params=False)
        assert "0" in repr(am)


# ---------------------------------------------------------------------------
# RateLimitMiddleware (standalone)
# ---------------------------------------------------------------------------


class TestRateLimitMiddlewareStandalone:
    def test_create(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert rm is not None

    def test_max_calls(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=5, window_ms=500)
        assert rm.max_calls == 5

    def test_window_ms(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=10, window_ms=2000)
        assert rm.window_ms == 2000

    def test_call_count_zero_initially(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert rm.call_count("create_sphere") == 0

    def test_call_count_unknown_zero(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=10, window_ms=1000)
        assert rm.call_count("nonexistent_action") == 0

    def test_repr_is_str(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=5, window_ms=500)
        assert isinstance(repr(rm), str)

    def test_repr_contains_max_calls(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=7, window_ms=500)
        assert "7" in repr(rm)

    def test_repr_contains_window_ms(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=5, window_ms=999)
        assert "999" in repr(rm)

    def test_large_max_calls(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=10000, window_ms=60000)
        assert rm.max_calls == 10000

    def test_small_window_ms(self):
        from dcc_mcp_core import RateLimitMiddleware

        rm = RateLimitMiddleware(max_calls=1, window_ms=100)
        assert rm.window_ms == 100


# ---------------------------------------------------------------------------
# SkillMetadata
# ---------------------------------------------------------------------------


class TestSkillMetadataCreate:
    def test_create_with_name(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("my-skill")
        assert sm.name == "my-skill"

    def test_description_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.description == ""

    def test_dcc_default_python(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.dcc == "python"

    def test_version_default(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.version == "1.0.0"

    def test_tools_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.tools == []

    def test_tags_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.tags == []

    def test_scripts_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.scripts == []

    def test_skill_path_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.skill_path == ""

    def test_allowed_tools_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.allowed_tools == []

    def test_license_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.license == ""

    def test_compatibility_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.compatibility == ""

    def test_depends_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.depends == []

    def test_metadata_files_default_empty(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        assert sm.metadata_files == []

    def test_repr_is_str(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("my-skill")
        assert isinstance(repr(sm), str)

    def test_repr_contains_name(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("my-geometry-skill")
        assert "my-geometry-skill" in repr(sm)


class TestSkillMetadataMutation:
    def test_set_description(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.description = "A geometry skill"
        assert sm.description == "A geometry skill"

    def test_set_dcc(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.dcc = "maya"
        assert sm.dcc == "maya"

    def test_set_version(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.version = "2.5.0"
        assert sm.version == "2.5.0"

    def test_set_tags(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.tags = ["geometry", "maya"]
        assert "geometry" in sm.tags
        assert "maya" in sm.tags

    def test_set_scripts(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.scripts = ["/path/to/create_sphere.py", "/path/to/delete_mesh.py"]
        assert len(sm.scripts) == 2

    def test_set_skill_path(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.skill_path = "/studio/skills/maya-geometry"
        assert sm.skill_path == "/studio/skills/maya-geometry"

    def test_set_allowed_tools(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.allowed_tools = ["Bash", "Read", "Write"]
        assert "Bash" in sm.allowed_tools

    def test_set_license(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.license = "MIT"
        assert sm.license == "MIT"

    def test_set_compatibility(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.compatibility = "Python>=3.9, Maya 2022+"
        assert "Python" in sm.compatibility

    def test_set_depends(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.depends = ["base-skill", "utils-skill"]
        assert "base-skill" in sm.depends

    def test_set_metadata_files(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.metadata_files = ["/path/depends.md"]
        assert len(sm.metadata_files) == 1

    def test_all_fields_set(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("maya-geometry")
        sm.description = "Maya geometry tools"
        sm.dcc = "maya"
        sm.version = "3.0.0"
        sm.tags = ["geo", "maya"]
        sm.scripts = ["/s/create_sphere.py"]
        sm.skill_path = "/studio/skills/maya-geometry"
        sm.allowed_tools = ["Bash", "Read"]
        sm.license = "MIT"
        sm.compatibility = "Maya 2022+"
        sm.depends = ["base"]
        assert sm.name == "maya-geometry"
        assert sm.description == "Maya geometry tools"
        assert sm.dcc == "maya"
        assert sm.version == "3.0.0"

    def test_repr_after_mutation(self):
        from dcc_mcp_core import SkillMetadata

        sm = SkillMetadata("x")
        sm.dcc = "blender"
        r = repr(sm)
        assert isinstance(r, str)
        assert "blender" in r
