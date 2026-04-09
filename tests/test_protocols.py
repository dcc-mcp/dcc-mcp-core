"""Tests for MCP protocol types — full getter/setter coverage."""

# Import future modules
from __future__ import annotations

# Import local modules
import dcc_mcp_core


class TestToolDefinition:
    def test_create(self) -> None:
        td = dcc_mcp_core.ToolDefinition(
            name="create_sphere",
            description="Create a sphere",
            input_schema='{"type": "object"}',
        )
        assert td.name == "create_sphere"
        assert td.description == "Create a sphere"
        assert td.input_schema == '{"type": "object"}'
        assert td.output_schema is None

    def test_create_with_output_schema(self) -> None:
        td = dcc_mcp_core.ToolDefinition(
            name="t",
            description="d",
            input_schema="{}",
            output_schema='{"type": "object"}',
        )
        assert td.output_schema == '{"type": "object"}'

    def test_setters(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="old", description="old", input_schema="{}")
        td.name = "new_name"
        td.description = "new_desc"
        assert td.name == "new_name"
        assert td.description == "new_desc"

    def test_repr(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="test", description="d", input_schema="{}")
        assert "test" in repr(td)

    def test_equality(self) -> None:
        td1 = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        td2 = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        assert td1 == td2

    def test_inequality(self) -> None:
        td1 = dcc_mcp_core.ToolDefinition(name="a", description="d", input_schema="{}")
        td2 = dcc_mcp_core.ToolDefinition(name="b", description="d", input_schema="{}")
        assert td1 != td2

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(title="My Tool", read_only_hint=True)
        td = dcc_mcp_core.ToolDefinition(
            name="list_objects",
            description="List scene objects",
            input_schema="{}",
            annotations=ann,
        )
        assert td.annotations is not None
        assert td.annotations.title == "My Tool"
        assert td.annotations.read_only_hint is True

    def test_annotations_default_none(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        assert td.annotations is None


class TestToolAnnotations:
    def test_defaults(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations()
        assert ann.title is None
        assert ann.read_only_hint is None
        assert ann.destructive_hint is None
        assert ann.idempotent_hint is None
        assert ann.open_world_hint is None

    def test_all_values(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="Tool",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        assert ann.title == "Tool"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is False

    def test_setters(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations()
        ann.title = "New Title"
        ann.read_only_hint = True
        ann.destructive_hint = False
        ann.idempotent_hint = True
        ann.open_world_hint = False
        assert ann.title == "New Title"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is False

    def test_set_none(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(title="x")
        ann.title = None
        assert ann.title is None


class TestResourceAnnotations:
    def test_defaults(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations()
        assert ann.audience == []
        assert ann.priority is None

    def test_with_audience(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user", "assistant"])
        assert "user" in ann.audience
        assert "assistant" in ann.audience

    def test_with_priority(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(priority=0.8)
        assert ann.priority is not None
        assert abs(ann.priority - 0.8) < 1e-6

    def test_full(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"], priority=1.0)
        assert len(ann.audience) == 1
        assert ann.priority is not None
        assert abs(ann.priority - 1.0) < 1e-6

    def test_repr(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"])
        assert "ResourceAnnotations" in repr(ann)


class TestResourceDefinition:
    def test_create(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="file:///test.txt", name="test", description="A test")
        assert rd.uri == "file:///test.txt"
        assert rd.name == "test"
        assert rd.description == "A test"
        assert rd.mime_type == "text/plain"

    def test_custom_mime_type(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="u", name="n", description="d", mime_type="application/json")
        assert rd.mime_type == "application/json"

    def test_setters(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="old", name="old", description="old")
        rd.uri = "new_uri"
        rd.name = "new_name"
        rd.description = "new_desc"
        rd.mime_type = "image/png"
        assert rd.uri == "new_uri"
        assert rd.name == "new_name"
        assert rd.description == "new_desc"
        assert rd.mime_type == "image/png"

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"], priority=0.5)
        rd = dcc_mcp_core.ResourceDefinition(
            uri="scene://current",
            name="scene",
            description="Current scene data",
            mime_type="application/json",
            annotations=ann,
        )
        assert rd.annotations is not None
        assert "user" in rd.annotations.audience

    def test_annotations_default_none(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="u", name="n", description="d")
        assert rd.annotations is None


class TestResourceTemplateDefinition:
    def test_create(self) -> None:
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="file:///{path}",
            name="template",
            description="A template",
        )
        assert rtd.uri_template == "file:///{path}"
        assert rtd.name == "template"
        assert rtd.description == "A template"
        assert rtd.mime_type == "text/plain"

    def test_setters(self) -> None:
        rtd = dcc_mcp_core.ResourceTemplateDefinition(uri_template="old", name="old", description="old")
        rtd.uri_template = "new/{id}"
        rtd.name = "new"
        rtd.description = "new desc"
        rtd.mime_type = "application/xml"
        assert rtd.uri_template == "new/{id}"
        assert rtd.name == "new"
        assert rtd.description == "new desc"
        assert rtd.mime_type == "application/xml"

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(priority=0.9)
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="dcc://{dcc}/{object}",
            name="dcc_object",
            description="DCC scene object",
            annotations=ann,
        )
        assert rtd.annotations is not None
        assert rtd.annotations.priority is not None
        assert abs(rtd.annotations.priority - 0.9) < 1e-6


class TestPromptArgument:
    def test_create_default(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="An argument")
        assert pa.name == "arg1"
        assert pa.description == "An argument"
        assert pa.required is False

    def test_required(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="Req", required=True)
        assert pa.required is True

    def test_setters(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="old", description="old")
        pa.name = "new"
        pa.description = "new desc"
        pa.required = True
        assert pa.name == "new"
        assert pa.description == "new desc"
        assert pa.required is True

    def test_equality(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        pa2 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        assert pa1 == pa2

    def test_inequality(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d")
        pa2 = dcc_mcp_core.PromptArgument(name="y", description="d")
        assert pa1 != pa2

    def test_repr(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="scene_name", description="The scene")
        assert "scene_name" in repr(pa)


class TestPromptDefinition:
    def test_create(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="my_prompt", description="A prompt")
        assert pd.name == "my_prompt"
        assert pd.description == "A prompt"

    def test_setters(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="old", description="old")
        pd.name = "new"
        pd.description = "new desc"
        assert pd.name == "new"
        assert pd.description == "new desc"

    def test_with_arguments(self) -> None:
        args = [
            dcc_mcp_core.PromptArgument(name="scene_path", description="Path to scene", required=True),
            dcc_mcp_core.PromptArgument(name="frame", description="Frame number"),
        ]
        pd = dcc_mcp_core.PromptDefinition(name="render_scene", description="Render a DCC scene", arguments=args)
        assert len(pd.arguments) == 2
        assert pd.arguments[0].name == "scene_path"
        assert pd.arguments[0].required is True
        assert pd.arguments[1].name == "frame"
        assert pd.arguments[1].required is False

    def test_arguments_default_empty(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="p", description="d")
        assert pd.arguments == []

    def test_equality(self) -> None:
        pd1 = dcc_mcp_core.PromptDefinition(name="p", description="d")
        pd2 = dcc_mcp_core.PromptDefinition(name="p", description="d")
        assert pd1 == pd2

    def test_inequality(self) -> None:
        pd1 = dcc_mcp_core.PromptDefinition(name="a", description="d")
        pd2 = dcc_mcp_core.PromptDefinition(name="b", description="d")
        assert pd1 != pd2

    def test_repr(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="render_prompt", description="Render")
        assert "render_prompt" in repr(pd)


class TestEncodeDecodeEnvelope:
    """Tests for the framed MessagePack transport helpers.

    All encode_* functions return bytes in the format:
        [4-byte big-endian length][MessagePack payload]

    decode_envelope() takes the payload WITHOUT the 4-byte prefix.
    """

    # ── encode_request ──────────────────────────────────────────────────────

    def test_encode_request_returns_bytes(self) -> None:
        frame = dcc_mcp_core.encode_request("ping")
        assert isinstance(frame, bytes)
        assert len(frame) >= 4

    def test_encode_request_length_prefix_correct(self) -> None:
        import struct

        frame = dcc_mcp_core.encode_request("ping")
        length = struct.unpack(">I", frame[:4])[0]
        assert length == len(frame) - 4

    def test_encode_request_no_params_roundtrip(self) -> None:
        frame = dcc_mcp_core.encode_request("ping")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "ping"
        assert isinstance(msg["id"], str)
        assert len(msg["id"]) > 0
        assert msg["params"] == b""

    def test_encode_request_with_bytes_params_roundtrip(self) -> None:
        payload = b'{"radius": 1.0}'
        frame = dcc_mcp_core.encode_request("create_sphere", payload)
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "create_sphere"
        assert msg["params"] == payload

    def test_encode_request_empty_method_allowed(self) -> None:
        frame = dcc_mcp_core.encode_request("")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["method"] == ""

    def test_encode_request_generates_unique_ids(self) -> None:
        frame1 = dcc_mcp_core.encode_request("ping")
        frame2 = dcc_mcp_core.encode_request("ping")
        msg1 = dcc_mcp_core.decode_envelope(frame1[4:])
        msg2 = dcc_mcp_core.decode_envelope(frame2[4:])
        assert msg1["id"] != msg2["id"]

    def test_encode_request_id_is_valid_uuid_format(self) -> None:
        import re

        frame = dcc_mcp_core.encode_request("test")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        uuid_pattern = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        assert uuid_pattern.match(msg["id"]), f"Not a UUID: {msg['id']}"

    # ── encode_response ─────────────────────────────────────────────────────

    def test_encode_response_success_roundtrip(self) -> None:
        import uuid

        req_id = str(uuid.uuid4())
        frame = dcc_mcp_core.encode_response(req_id, True, b'{"name": "sphere1"}')
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "response"
        assert msg["id"] == req_id
        assert msg["success"] is True
        assert msg["payload"] == b'{"name": "sphere1"}'
        assert msg["error"] is None

    def test_encode_response_error_roundtrip(self) -> None:
        import uuid

        req_id = str(uuid.uuid4())
        frame = dcc_mcp_core.encode_response(req_id, False, error="object not found")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "response"
        assert msg["success"] is False
        assert msg["error"] == "object not found"
        assert msg["payload"] == b""

    def test_encode_response_no_payload_roundtrip(self) -> None:
        import uuid

        req_id = str(uuid.uuid4())
        frame = dcc_mcp_core.encode_response(req_id, True)
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["success"] is True
        assert msg["payload"] == b""

    def test_encode_response_invalid_uuid_raises_value_error(self) -> None:
        import pytest

        with pytest.raises(ValueError, match="invalid UUID"):
            dcc_mcp_core.encode_response("not-a-uuid", True)

    def test_encode_response_preserves_request_id(self) -> None:
        import uuid

        req_id = "00000000-0000-0000-0000-000000000000"
        frame = dcc_mcp_core.encode_response(req_id, True)
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["id"] == req_id

    # ── encode_notify ────────────────────────────────────────────────────────

    def test_encode_notify_returns_bytes(self) -> None:
        frame = dcc_mcp_core.encode_notify("heartbeat")
        assert isinstance(frame, bytes)
        assert len(frame) >= 4

    def test_encode_notify_no_data_roundtrip(self) -> None:
        frame = dcc_mcp_core.encode_notify("heartbeat")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert msg["topic"] == "heartbeat"
        assert msg["data"] == b""

    def test_encode_notify_with_data_roundtrip(self) -> None:
        data = b'{"scene": "test.mb", "frame": 1}'
        frame = dcc_mcp_core.encode_notify("scene_changed", data)
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert msg["topic"] == "scene_changed"
        assert msg["data"] == data

    def test_encode_notify_id_is_none(self) -> None:
        frame = dcc_mcp_core.encode_notify("render_complete", b"done")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["id"] is None

    def test_encode_notify_length_prefix_correct(self) -> None:
        import struct

        frame = dcc_mcp_core.encode_notify("test", b"payload")
        length = struct.unpack(">I", frame[:4])[0]
        assert length == len(frame) - 4

    # ── decode_envelope error paths ─────────────────────────────────────────

    def test_decode_envelope_invalid_msgpack_raises_runtime_error(self) -> None:
        import pytest

        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(b"not-msgpack")

    def test_decode_envelope_empty_bytes_raises_runtime_error(self) -> None:
        import pytest

        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(b"")

    def test_decode_envelope_returns_dict(self) -> None:
        frame = dcc_mcp_core.encode_request("test")
        result = dcc_mcp_core.decode_envelope(frame[4:])
        assert isinstance(result, dict)

    def test_decode_envelope_request_has_expected_keys(self) -> None:
        frame = dcc_mcp_core.encode_request("execute_python", b"cmds.sphere()")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert "type" in msg
        assert "id" in msg
        assert "method" in msg
        assert "params" in msg

    def test_decode_envelope_response_has_expected_keys(self) -> None:
        import uuid

        frame = dcc_mcp_core.encode_response(str(uuid.uuid4()), True, b"ok")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert "type" in msg
        assert "id" in msg
        assert "success" in msg
        assert "payload" in msg
        assert "error" in msg

    def test_decode_envelope_notify_has_expected_keys(self) -> None:
        frame = dcc_mcp_core.encode_notify("status", b"running")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert "type" in msg
        assert "id" in msg
        assert "topic" in msg
        assert "data" in msg
