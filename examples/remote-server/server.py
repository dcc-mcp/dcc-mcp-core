"""Minimal remote-accessible MCP server example.

Starts a publicly reachable MCP server on 0.0.0.0 with CORS and
optional API-key auth from environment variables.

Usage:
    DCC_MCP_API_KEY=secret python server.py
    # then connect from any MCP client at http://<host>:8765/mcp
"""

from __future__ import annotations

import os
import signal
import time

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server

PORT = int(os.environ.get("DCC_MCP_PORT", "8765"))
HOST = os.environ.get("DCC_MCP_HOST", "0.0.0.0")
API_KEY = os.environ.get("DCC_MCP_API_KEY", "")
SKILL_PATHS = os.environ.get("DCC_MCP_SKILL_PATHS", "")

cfg = McpHttpConfig(
    port=PORT,
    server_name="remote-mcp",
    enable_cors=True,
)
cfg.host = HOST

if API_KEY:
    cfg.api_key = API_KEY

if SKILL_PATHS:
    os.environ["DCC_MCP_SKILL_PATHS"] = SKILL_PATHS

server = create_skill_server("generic", cfg)
handle = server.start()

print(f"MCP server listening at {handle.mcp_url()}")
print(f"  host:     {HOST}")
print(f"  port:     {PORT}")
print(f"  auth:     {'api-key' if API_KEY else 'none (dev mode)'}")
print("  cors:     enabled")
print("Press Ctrl+C to stop.")

_running = True


def _stop(sig, frame):
    global _running
    _running = False


signal.signal(signal.SIGINT, _stop)
signal.signal(signal.SIGTERM, _stop)

while _running:
    time.sleep(1)

handle.shutdown()
print("Server stopped.")
