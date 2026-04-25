# Gateway Election API

Generic gateway failover election for any DCC MCP server.

When the current gateway instance becomes unreachable, non-gateway instances automatically run a first-wins election to take over and maintain service availability.

**Exported symbols:** `DccGatewayElection`

## DccGatewayElection

### Constructor

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `dcc_name` | `str` | (required) | Short DCC identifier for log messages |
| `server` | `Any` | (required) | DCC server instance |
| `gateway_host` | `str` | `"127.0.0.1"` | Gateway bind address |
| `gateway_port` | `int` | `9765` | Gateway port to compete for |
| `probe_interval` | `int` | `5` | Seconds between health probes |
| `probe_timeout` | `float` | `2.0` | Timeout per probe in seconds |
| `probe_failures` | `int` | `3` | Consecutive failures before election |
| `on_promote` | `callable \| None` | `None` | Callback invoked after winning election |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_running` | `bool` | Whether the election thread is active |
| `consecutive_failures` | `int` | Current consecutive gateway probe failure count |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start()` | `None` | Start the background election thread |
| `stop()` | `None` | Gracefully stop the election thread |
| `get_status()` | `dict` | Return `{running, consecutive_failures, gateway_host, gateway_port}` |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DCC_MCP_GATEWAY_PROBE_INTERVAL` | `5` | Seconds between health probes |
| `DCC_MCP_GATEWAY_PROBE_TIMEOUT` | `2` | Timeout per probe in seconds |
| `DCC_MCP_GATEWAY_PROBE_FAILURES` | `3` | Consecutive failures before election |

```python
from dcc_mcp_core import DccGatewayElection

election = DccGatewayElection(dcc_name="blender", server=blender_server)
election.start()
# ... runs in background ...
election.stop()
```
