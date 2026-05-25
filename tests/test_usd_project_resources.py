"""Regression tests for USD project resource conventions (#1209)."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from conftest import McpClient
from dcc_mcp_core import USD_ASSETS_URI
from dcc_mcp_core import USD_JSON_MIME
from dcc_mcp_core import USD_LAYERS_URI
from dcc_mcp_core import USD_STAGE_URI
from dcc_mcp_core import USD_TEXT_MIME
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import build_usd_project_resources
from dcc_mcp_core import register_usd_project_resources


def _post_json(url: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    return McpClient(url).post(body)


def test_build_usd_project_resources_creates_stable_records(tmp_path: Path) -> None:
    stage = tmp_path / "shot.usda"
    layer = tmp_path / "lighting.usda"
    asset = tmp_path / "textures" / "diffuse.png"
    stage.write_text("#usda 1.0\n", encoding="utf-8")
    layer.write_text("#usda 1.0\n", encoding="utf-8")
    asset.parent.mkdir()
    asset.write_bytes(b"png")

    records = build_usd_project_resources(
        project_root=tmp_path,
        stage=stage,
        layers=[layer],
        assets=["textures/diffuse.png"],
        project_label="Shot 010",
    )
    by_uri = {record.uri: record for record in records}

    assert by_uri[USD_STAGE_URI].mime_type == USD_TEXT_MIME
    assert by_uri[USD_STAGE_URI].file_ref["display_name"] == "shot.usda"
    assert by_uri[USD_LAYERS_URI].mime_type == USD_JSON_MIME
    assert by_uri[USD_LAYERS_URI].content["count"] == 1
    assert by_uri[USD_LAYERS_URI].content["resources"][0]["metadata"]["project_root_label"] == "Shot 010"
    assert by_uri[USD_ASSETS_URI].content["resources"][0]["path"] == str(asset)


def test_register_usd_project_resources_surfaces_mcp_metadata(tmp_path: Path) -> None:
    stage = tmp_path / "asset.usda"
    layer = tmp_path / "model.usda"
    stage.write_text("#usda 1.0\n", encoding="utf-8")
    layer.write_text("#usda 1.0\n", encoding="utf-8")

    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="usd-resource-test"))
    provider = register_usd_project_resources(
        server,
        project_root=tmp_path,
        stage=stage,
        layers=[layer],
        validation={"name": "usdchecker.json", "content": {"status": "ok"}},
        project_label="OpenUSD Test",
    )
    assert any(record.uri == USD_STAGE_URI for record in provider.records)

    handle = server.start()
    try:
        url = handle.mcp_url()
        code, listed = _post_json(url, {"jsonrpc": "2.0", "id": 1, "method": "resources/list"})
        assert code == 200
        resources = {item["uri"]: item for item in listed["result"]["resources"]}
        assert resources[USD_STAGE_URI]["name"] == "asset.usda"
        assert resources[USD_STAGE_URI]["mimeType"] == USD_TEXT_MIME
        assert resources[USD_LAYERS_URI]["mimeType"] == USD_JSON_MIME

        code, stage_body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "resources/read",
                "params": {"uri": USD_STAGE_URI},
            },
        )
        assert code == 200
        assert stage_body["result"]["contents"][0]["text"] == "#usda 1.0\n"

        code, layer_manifest = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "resources/read",
                "params": {"uri": USD_LAYERS_URI},
            },
        )
        assert code == 200
        payload = json.loads(layer_manifest["result"]["contents"][0]["text"])
        assert payload["project_label"] == "OpenUSD Test"
        assert payload["resources"][0]["uri"] == "openusd://layers/model"
    finally:
        handle.shutdown()
