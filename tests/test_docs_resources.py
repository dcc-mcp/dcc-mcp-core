"""Tests for docs:// MCP resource provider (issue #435).

Covers:
- get_builtin_docs_uris: returns list of docs:// URIs
- get_docs_content: returns content dict for known URI
- get_docs_content: returns None for unknown URI
- register_docs_resource: stores content in registry
- register_docs_resource: warns and skips non-docs:// URIs
- register_docs_resources_from_dir: scans directory and registers files
- register_docs_server: registers all built-ins on a server
- All built-in URIs have expected fields
- Public API importable from top-level dcc_mcp_core
"""

from __future__ import annotations

from pathlib import Path
import tempfile
from unittest.mock import MagicMock


def test_builtin_uris_returned():
    from dcc_mcp_core.docs_resources import get_builtin_docs_uris

    uris = get_builtin_docs_uris()
    assert len(uris) > 0
    for uri in uris:
        assert uri.startswith("docs://")


def test_get_docs_content_known():
    from dcc_mcp_core.docs_resources import get_docs_content

    content = get_docs_content("docs://output-format/call-action")
    assert content is not None
    assert "name" in content
    assert "content" in content
    assert "mime" in content


def test_get_docs_content_unknown():
    from dcc_mcp_core.docs_resources import get_docs_content

    assert get_docs_content("docs://nonexistent/resource") is None


def test_all_builtins_have_required_fields():
    from dcc_mcp_core.docs_resources import _DOCS

    for uri, meta in _DOCS.items():
        assert "name" in meta, f"{uri} missing 'name'"
        assert "description" in meta, f"{uri} missing 'description'"
        assert "content" in meta, f"{uri} missing 'content'"
        assert "mime" in meta, f"{uri} missing 'mime'"
        assert meta["content"].strip(), f"{uri} has empty content"


def test_register_docs_resource_stores_in_registry():
    from dcc_mcp_core.docs_resources import get_docs_content
    from dcc_mcp_core.docs_resources import register_docs_resource

    server = MagicMock()
    register_docs_resource(
        server,
        uri="docs://test/custom-doc",
        name="Custom Doc",
        description="A test document.",
        content="# Custom\n\nHello world.",
    )
    content = get_docs_content("docs://test/custom-doc")
    assert content is not None
    assert content["name"] == "Custom Doc"
    assert "Hello world" in content["content"]


def test_register_docs_resource_invalid_uri(caplog):
    import logging

    from dcc_mcp_core.docs_resources import register_docs_resource

    server = MagicMock()
    with caplog.at_level(logging.WARNING):
        register_docs_resource(
            server,
            uri="scene://current",
            name="Wrong scheme",
            description="Should be skipped.",
            content="nope",
        )
    assert "docs://" in caplog.text


def test_register_docs_resources_from_dir():
    from dcc_mcp_core.docs_resources import get_docs_content
    from dcc_mcp_core.docs_resources import register_docs_resources_from_dir

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "guide.md").write_text("# Guide\n\nSome content.", encoding="utf-8")
        (root / "reference.md").write_text("# Reference\n\nRef content.", encoding="utf-8")

        server = MagicMock()
        registered = register_docs_resources_from_dir(
            server,
            directory=tmp,
            uri_prefix="docs://myskill",
        )

        assert len(registered) == 2
        assert all(u.startswith("docs://myskill/") for u in registered)

        for uri in registered:
            assert get_docs_content(uri) is not None


def test_register_docs_resources_from_dir_missing():
    from dcc_mcp_core.docs_resources import register_docs_resources_from_dir

    server = MagicMock()
    result = register_docs_resources_from_dir(server, directory="/nonexistent/path")
    assert result == []


def test_register_docs_server():
    from dcc_mcp_core.docs_resources import get_builtin_docs_uris
    from dcc_mcp_core.docs_resources import register_docs_server

    server = MagicMock()
    register_docs_server(server)

    # add_docs_resource should have been called for each built-in
    expected_count = len(get_builtin_docs_uris())
    assert server.add_docs_resource.call_count == expected_count


def test_importable_from_top_level():
    import dcc_mcp_core

    assert hasattr(dcc_mcp_core, "get_builtin_docs_uris")
    assert hasattr(dcc_mcp_core, "get_docs_content")
    assert hasattr(dcc_mcp_core, "register_docs_resource")
    assert hasattr(dcc_mcp_core, "register_docs_resources_from_dir")
    assert hasattr(dcc_mcp_core, "register_docs_server")
