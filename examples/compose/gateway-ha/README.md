# Gateway HA — docker-compose

Minimal HA topology for `dcc-mcp-server`:

- `gateway-a`, `gateway-b` — two gateway candidates sharing one
  `FileRegistry` volume. On a single host only one wins the well-known
  port (`9765`); the other is a warm standby that will take over via
  the election protocol if `gateway-a` dies.
- `dcc-maya-1`, `dcc-blender-1` — mock DCC instances registered
  through the same shared volume.

For the full conceptual background see
[`docs/guide/production-deployment.md`](../../../docs/guide/production-deployment.md).

## Build & run

From the **repo root**:

```bash
cd examples/compose/gateway-ha
docker compose build
docker compose up -d
docker compose ps
```

## Smoke test

```bash
# Gateway health
curl -sf http://localhost:9765/health
# → {"ok":true}

# Registered DCC instances
curl -s http://localhost:9765/instances | jq
# → [ { "dcc_type": "maya", ... }, { "dcc_type": "blender", ... } ]

# MCP JSON-RPC — list aggregated tools
curl -sf -X POST http://localhost:9765/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq '.result.tools | length'
```

## Failover test

```bash
# Take the current gateway down; gateway-b takes over within a few seconds.
docker compose stop gateway-a
sleep 8
curl -sf http://localhost:9765/health    # still 200 via gateway-b
docker compose start gateway-a
```

## Tear down

```bash
docker compose down -v   # -v removes the registry volume
```

## Production notes

- This compose file uses the **host port** `9765` for demo convenience.
  Real deployments put nginx or an ALB in front — see the main guide.
- Reduce `DCC_MCP_HEARTBEAT_INTERVAL` / `DCC_MCP_STALE_TIMEOUT` for
  faster failover; do not set either below `1s`.
- The `registry` Docker volume must be on a filesystem with working
  `fsync` — local volumes are fine, some bind mounts on Windows are not.
