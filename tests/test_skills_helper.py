"""Tests for the canonical skill-helper namespace."""

from __future__ import annotations

import hashlib
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
import socket
import threading
import time

import pytest

import dcc_mcp_core
from dcc_mcp_core import skills_helper
from dcc_mcp_core.skills_helper import HttpStatusError
from dcc_mcp_core.skills_helper import SkillCodecError
from dcc_mcp_core.skills_helper import SkillFileError
from dcc_mcp_core.skills_helper import SkillHttpError
from dcc_mcp_core.skills_helper import ToolValidator
from dcc_mcp_core.skills_helper import normalize_tool_arguments
from dcc_mcp_core.skills_helper import skill_error_from_exception
from dcc_mcp_core.skills_helper import skill_success


def test_skills_helper_json_yaml_codecs_roundtrip() -> None:
    payload = {"name": "café", "frames": [1, 2, 3], "enabled": True}

    encoded = skills_helper.json_dumps(payload, ensure_ascii=False)
    assert "café" in encoded
    assert skills_helper.json_loads(encoded) == payload

    yaml_encoded = skills_helper.yaml_dumps(payload)
    assert skills_helper.yaml_loads(yaml_encoded) == payload


def test_skills_helper_json_codecs_fall_back_without_core(monkeypatch) -> None:
    def missing_core(_name: str):
        raise ModuleNotFoundError("No module named 'dcc_mcp_core._core'", name="dcc_mcp_core._core")

    monkeypatch.setattr(skills_helper, "_core_symbol", missing_core)

    encoded = skills_helper.json_dumps({"name": "café"}, ensure_ascii=False)

    assert "café" in encoded
    assert skills_helper.json_loads(encoded) == {"name": "café"}


def test_legacy_top_level_codecs_reexport_skills_helper() -> None:
    assert dcc_mcp_core.json_dumps is skills_helper.json_dumps
    assert dcc_mcp_core.json_loads is skills_helper.json_loads
    assert dcc_mcp_core.yaml_dumps is skills_helper.yaml_dumps
    assert dcc_mcp_core.yaml_loads is skills_helper.yaml_loads

    assert dcc_mcp_core.json_loads(dcc_mcp_core.json_dumps({"ok": True})) == {"ok": True}


def test_skills_helper_reexports_validation_and_normalization() -> None:
    validator = ToolValidator.from_schema_json(
        skills_helper.json_dumps(
            {
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string"}},
            }
        )
    )

    ok, errors = validator.validate(skills_helper.json_dumps({"name": "maya"}))

    assert ok is True
    assert errors == []
    assert normalize_tool_arguments('{"name":"maya"}') == {"name": "maya"}


def test_skills_helper_reexports_skill_result_helpers() -> None:
    result = skill_success("Created cube", object_name="cube1")

    assert result["success"] is True
    assert result["message"] == "Created cube"
    assert result["context"] == {"object_name": "cube1"}


def test_skill_error_from_exception_uses_standard_skill_error_shape() -> None:
    exc = ValueError("bad radius")

    result = skill_error_from_exception(exc, prompt="Use a positive radius.", radius=-1)

    assert result["success"] is False
    assert result["message"] == "bad radius"
    assert result["error"] == "ValueError"
    assert result["prompt"] == "Use a positive radius."
    assert result["context"] == {"radius": -1}


def test_skills_helper_reports_invalid_json_errors() -> None:
    with pytest.raises(ValueError):
        skills_helper.json_loads("{not json}")


def test_skills_helper_json_file_helpers_add_source_context(tmp_path) -> None:
    path = tmp_path / "nested" / "payload.json"

    written = skills_helper.dump_json_file(path, {"name": "café"}, ensure_ascii=False)

    assert written == path
    assert skills_helper.load_json_file(path, require_mapping=True) == {"name": "café"}
    assert "café" in path.read_text(encoding="utf-8")

    bad = tmp_path / "bad.json"
    bad.write_text("{bad", encoding="utf-8")
    with pytest.raises(SkillCodecError, match=r"bad\.json: json:"):
        skills_helper.load_json_file(bad)


def test_skills_helper_yaml_file_helpers_support_empty_and_unicode_roots(tmp_path) -> None:
    path = tmp_path / "payload.yaml"

    skills_helper.dump_yaml_file(path, {"label": "动画", "items": [1, 2]})

    assert skills_helper.load_yaml_file(path, require_mapping=True) == {"label": "动画", "items": [1, 2]}
    assert skills_helper.load_yaml_text("", source="empty.yaml") is None


def test_skills_helper_mapping_root_validation_reports_actual_root_type(tmp_path) -> None:
    path = tmp_path / "list.yaml"
    path.write_text("- a\n- b\n", encoding="utf-8")

    with pytest.raises(SkillCodecError, match="expected a mapping root, got list"):
        skills_helper.load_yaml_file(path, require_mapping=True)


def test_skills_helper_text_helpers_are_utf8_and_bounded(tmp_path) -> None:
    path = tmp_path / "note.txt"

    skills_helper.dump_text(path, "hello 世界")

    assert skills_helper.load_text(path) == "hello 世界"
    with pytest.raises(SkillCodecError, match="exceeding max_bytes=4"):
        skills_helper.load_text(path, max_bytes=4)


def test_skills_helper_atomic_write_and_digest_helpers(tmp_path) -> None:
    root = tmp_path / "workspace"
    root.mkdir()
    path = root / "nested" / "payload.txt"

    written = skills_helper.atomic_write_text(
        "nested/payload.txt",
        "hello 世界",
        root=root,
    )

    assert written == path.resolve()
    assert path.read_text(encoding="utf-8") == "hello 世界"
    expected = hashlib.sha256("hello 世界".encode()).hexdigest()
    assert skills_helper.file_digest("nested/payload.txt", root=root) == expected
    assert skills_helper.bytes_digest(b"hello") == hashlib.sha256(b"hello").hexdigest()

    bytes_path = skills_helper.atomic_write_bytes(root / "data.bin", b"\x00\x01")
    assert bytes_path.read_bytes() == b"\x00\x01"


def test_skills_helper_safe_paths_reject_traversal(tmp_path) -> None:
    root = tmp_path / "workspace"
    outside = tmp_path / "outside.txt"
    root.mkdir()
    outside.write_text("nope", encoding="utf-8")

    with pytest.raises(SkillFileError, match="escapes root"):
        skills_helper.ensure_within_root(root, outside)

    with pytest.raises(SkillFileError, match="escapes root"):
        skills_helper.atomic_write_text("../outside.txt", "nope", root=root)


def test_skills_helper_file_helpers_are_bounded(tmp_path) -> None:
    root = tmp_path / "workspace"
    root.mkdir()
    path = root / "payload.bin"
    path.write_bytes(b"abcdef")

    with pytest.raises(SkillFileError, match="exceeding max_bytes=4"):
        skills_helper.atomic_write_bytes(path, b"abcdef", max_bytes=4)

    with pytest.raises(SkillFileError, match="exceeding max_bytes=4"):
        skills_helper.file_digest(path, max_bytes=4)


def test_skills_helper_lz4_roundtrip_and_limits() -> None:
    payload = b"payload-" * 2048

    compressed = skills_helper.compress_bytes(payload)

    assert isinstance(compressed, bytes)
    assert skills_helper.decompress_bytes(compressed) == payload
    with pytest.raises(SkillFileError, match="exceeds max_bytes=16"):
        skills_helper.decompress_bytes(compressed, max_bytes=16)
    with pytest.raises(SkillFileError, match="unsupported algorithm"):
        skills_helper.compress_bytes(payload, algorithm="gzip")


class _SkillHelperHttpHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path.startswith("/json"):
            self._send_json(
                200,
                {
                    "ok": True,
                    "path": self.path,
                    "auth": self.headers.get("Authorization"),
                },
            )
        elif self.path.startswith("/error"):
            self._send_json(418, {"ok": False, "error": "teapot"})
        elif self.path.startswith("/slow"):
            time.sleep(0.2)
            self._send_json(200, {"ok": True})
        elif self.path.startswith("/text"):
            self._send_text(200, "not json")
        elif self.path.startswith("/large"):
            self._send_text(200, "abcdef")
        else:
            self._send_json(404, {"ok": False})

    def do_POST(self) -> None:
        length = int(self.headers.get("content-length") or "0")
        body = self.rfile.read(length).decode("utf-8")
        self._send_text(200, body, content_type="application/json")

    def log_message(self, _format: str, *_args) -> None:
        return None

    def _send_json(self, status: int, payload) -> None:
        self._send_text(status, skills_helper.json_dumps(payload, ensure_ascii=False), content_type="application/json")

    def _send_text(self, status: int, text: str, *, content_type: str = "text/plain") -> None:
        data = text.encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


@pytest.fixture
def http_server():
    server = ThreadingHTTPServer(("127.0.0.1", 0), _SkillHelperHttpHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2.0)


def test_http_request_returns_structured_response(http_server) -> None:
    response = skills_helper.http_request(
        "GET",
        f"{http_server}/json",
        headers={"Authorization": "Bearer secret"},
        query={"name": "cube", "tag": ["a", "b"]},
    )

    assert response.status == 200
    assert response.truncated is False
    assert response.headers["content-type"] == "application/json"
    assert response.json()["ok"] is True
    assert "name=cube" in response.url
    assert "tag=a" in response.url
    assert response.elapsed_ms >= 0


def test_http_post_json_uses_rust_json_codec(http_server) -> None:
    payload = {"name": "café", "enabled": True}

    result = skills_helper.http_post_json(f"{http_server}/echo", payload)

    assert result == payload


def test_http_error_can_raise_structured_status_error(http_server) -> None:
    with pytest.raises(HttpStatusError) as raised:
        skills_helper.http_request("GET", f"{http_server}/error", raise_for_status=True)

    err = raised.value
    assert err.status == 418
    assert err.response.status == 418
    assert err.kind == "http-status"
    assert err.to_skill_error()["error"] == "http-status"


def test_http_timeout_raises_structured_http_error(http_server) -> None:
    with pytest.raises(SkillHttpError) as raised:
        skills_helper.http_get_json(f"{http_server}/slow", timeout_ms=10)

    assert raised.value.kind == "timeout"
    assert raised.value.url.startswith(http_server)


def test_http_invalid_json_uses_codec_error(http_server) -> None:
    with pytest.raises(SkillCodecError, match="response: json:"):
        skills_helper.http_get_json(f"{http_server}/text")


def test_http_oversized_response_marks_truncation(http_server) -> None:
    response = skills_helper.http_request("GET", f"{http_server}/large", max_bytes=4)

    assert response.bytes == b"abcd"
    assert response.text == "abcd"
    assert response.truncated is True
    with pytest.raises(SkillHttpError) as raised:
        response.json()
    assert raised.value.kind == "response-truncated"


def test_http_header_redaction_masks_common_secret_headers() -> None:
    redacted = skills_helper.redact_http_headers(
        {
            "Authorization": "Bearer secret",
            "X-Api-Key": "secret",
            "Accept": "application/json",
        }
    )

    assert redacted["Authorization"] == "[REDACTED]"
    assert redacted["X-Api-Key"] == "[REDACTED]"
    assert redacted["Accept"] == "application/json"


def test_http_transport_error_is_structured() -> None:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        unused_port = sock.getsockname()[1]

    with pytest.raises(SkillHttpError) as raised:
        skills_helper.http_get_json(f"http://127.0.0.1:{unused_port}/missing", timeout_ms=200)

    assert raised.value.kind in {"connect", "request", "timeout"}
    assert "127.0.0.1" in str(raised.value)
