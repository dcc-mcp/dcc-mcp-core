"""Composition root for an isolated native Inkscape MCP example."""

import json
import os
from pathlib import Path
import signal
import time

from dcc_mcp_core import AdapterReadinessBinder
from dcc_mcp_core import DccServerBase
from dcc_mcp_core import DccServerOptions
from runtime import configured_runtime

HERE = Path(__file__).resolve().parent


def start_server():
    """Start a standalone controller without binding to an unrelated GUI PID."""
    runtime = configured_runtime()
    gateway_port = int(os.environ["DCC_MCP_INKSCAPE_GATEWAY_PORT"])
    registry_dir = os.environ["DCC_MCP_INKSCAPE_REGISTRY_DIR"]
    if not 1024 <= gateway_port <= 65535 or gateway_port == 9765:
        raise ValueError("Configure an isolated non-default gateway port")
    # Declarative subprocess scripts import this example's reusable bridge.
    os.environ["PYTHONPATH"] = str(HERE) + os.pathsep + os.environ.get("PYTHONPATH", "")
    options = DccServerOptions.from_env(
        "inkscape",
        HERE / "skills",
        server_name="native-inkscape-example",
        instance_type="standalone",
        adapter_version="0.1.0",
        dcc_version=runtime.capabilities()["version"],
        registry_dir=registry_dir,
        gateway_port=gateway_port,
        enable_gateway_failover=False,
    )
    server = DccServerBase(options)
    AdapterReadinessBinder.bind_headless(server)
    server.register_builtin_actions(include_bundled=False)
    server.start()
    return server


if __name__ == "__main__":
    server = start_server()
    print(
        json.dumps(
            {
                "instance_id": server.instance_id,
                "status": "ready",
                "dcc": "inkscape",
                "mcp_url": server.mcp_url,
                "gateway_url": server.gateway_url,
            }
        ),
        flush=True,
    )
    stopped = False

    def stop(signum, frame):
        global stopped
        stopped = True

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        while not stopped:
            time.sleep(0.25)
    finally:
        server.stop()
