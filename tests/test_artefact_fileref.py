"""End-to-end tests for the FileRef + artefact:// resource scheme (issue #349).

Exercises both the low-level helpers (``artefact_put_file`` /
``artefact_get_bytes``) and the wiring of the ``artefact://`` URI scheme
through a live ``McpHttpServer`` with ``enable_artefact_resources=True``.
"""

from __future__ import annotations

import base64
import json
from pathlib import Path
import tempfile
from typing import Any
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import FileRef
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import artefact_get_bytes
from dcc_mcp_core import artefact_list
from dcc_mcp_core import artefact_put_bytes
from dcc_mcp_core import artefact_put_file


def _post_json(
    url: str,
    body: dict[str, Any],
) -> tuple[int, dict[str, Any]]:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


class TestFileRefType:
    def test_put_bytes_returns_fileref_with_metadata(self):
        fr = artefact_put_bytes(b"abc123", mime="application/octet-stream")
        assert isinstance(fr, FileRef)
        assert fr.uri.startswith("artefact://sha256/")
        assert fr.size_bytes == 6
        assert fr.digest is not None and fr.digest.startswith("sha256:")
        assert fr.mime == "application/octet-stream"
        # RFC-3339 timestamp.
        assert "T" in fr.created_at

    def test_put_bytes_is_content_addressed(self):
        a = artefact_put_bytes(b"same-content")
        b = artefact_put_bytes(b"same-content")
        assert a.uri == b.uri
        assert a.digest == b.digest

    def test_put_file_and_get_bytes_round_trip(self):
        with tempfile.NamedTemporaryFile(delete=False, suffix=".bin") as f:
            f.write(b"payload-bytes")
            path = Path(f.name)
        try:
            fr = artefact_put_file(str(path), mime="application/octet-stream")
            got = artefact_get_bytes(fr.uri)
            assert got == b"payload-bytes"
        finally:
            path.unlink(missing_ok=True)

    def test_get_bytes_unknown_uri_raises(self):
        with pytest.raises(IOError):
            artefact_get_bytes("artefact://sha256/ffffffffffffffffffffffffffffffff")

    def test_artefact_list_includes_freshly_put(self):
        fr = artefact_put_bytes(b"list-me-please-" + str(id(object())).encode())
        uris = {entry.uri for entry in artefact_list()}
        assert fr.uri in uris


class TestArtefactResourceScheme:
    @pytest.fixture(scope="class")
    def artefact_server(self):
        reg = ToolRegistry()
        reg.register(
            "noop",
            description="placeholder",
            category="test",
            tags=[],
            dcc="test",
            version="1.0.0",
        )
        cfg = McpHttpConfig(port=0, server_name="artefact-e2e")
        cfg.enable_artefact_resources = True
        assert cfg.enable_artefact_resources is True
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        yield server, handle, handle.mcp_url()
        handle.shutdown()

    def test_resources_list_contains_put_artefact(self, artefact_server):
        _, _, url = artefact_server
        # Put an artefact via the process-global helper. When the server is
        # co-located in-process, its resources registry reads from the same
        # FilesystemArtefactStore root, so the URI is discoverable.
        fr = artefact_put_bytes(b"mcp-visible-artefact", mime="text/plain")

        code, body = _post_json(
            url,
            {"jsonrpc": "2.0", "id": 1, "method": "resources/list"},
        )
        assert code == 200
        resources = body["result"]["resources"]
        uris = {r["uri"] for r in resources}
        assert fr.uri in uris, f"{fr.uri} not in {uris}"

    def test_resources_read_returns_original_bytes(self, artefact_server):
        _, _, url = artefact_server
        payload = b"round-trip-through-mcp"
        fr = artefact_put_bytes(payload, mime="application/octet-stream")

        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "resources/read",
                "params": {"uri": fr.uri},
            },
        )
        assert code == 200
        item = body["result"]["contents"][0]
        assert item["uri"] == fr.uri
        assert "blob" in item
        decoded = base64.b64decode(item["blob"])
        assert decoded == payload

    def test_resources_read_unknown_uri_errors(self, artefact_server):
        _, _, url = artefact_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "resources/read",
                "params": {
                    "uri": "artefact://sha256/00000000000000000000000000000000",
                },
            },
        )
        assert code == 200
        assert "error" in body
        # RESOURCE_NOT_ENABLED_ERROR code in protocol.rs is -32002, reused
        # for "not found" when the scheme is valid but the URI isn't stored.
        assert body["error"]["code"] == -32002
