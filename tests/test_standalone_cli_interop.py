"""Exercise the pinned standalone client against the wheel built by Core CI."""

import json
import os
import subprocess

import pytest


@pytest.mark.skipif(not os.environ.get("DCC_MCP_TEST_CLI"), reason="requires the standalone CLI")
def test_standalone_cli_discovers_a_registered_tool(tmp_path):
    from dcc_mcp_core import McpHttpConfig
    from dcc_mcp_core import McpHttpServer
    from dcc_mcp_core import ToolRegistry

    registry = ToolRegistry()
    registry.register(
        "scene_info", description="Read scene metadata", category="scene", tags=[], dcc="test", version="1"
    )
    config = McpHttpConfig(port=0, server_name="standalone-cli-interop")
    config.registry_dir = str(tmp_path / "registry")
    server = McpHttpServer(registry, config)
    server.register_handler("scene_info", lambda _params: {"objects": []})
    handle = server.start()
    try:

        def invoke(*args):
            command = [os.environ["DCC_MCP_TEST_CLI"], "--output", "json", "mcp", *args, "--url", handle.mcp_url()]
            env = dict(
                os.environ,
                HOME=str(tmp_path),
                USERPROFILE=str(tmp_path),
                APPDATA=str(tmp_path),
                LOCALAPPDATA=str(tmp_path),
                XDG_DATA_HOME=str(tmp_path),
                DCC_MCP_CLI_NO_UPDATE_MAINTENANCE="true",
                DCC_MCP_CLI_AUTO_UPDATE="false",
            )
            output = subprocess.check_output(command, env=env, timeout=30)
            return json.loads(output)

        assert invoke("initialize")["serverInfo"]["name"] == "standalone-cli-interop"
        tools = invoke("tools", "list")["tools"]
        assert "scene_info" in {tool["name"] for tool in tools}
    finally:
        handle.shutdown()
