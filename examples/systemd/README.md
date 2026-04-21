# systemd — dcc-mcp-gateway

A hardened systemd unit for the standalone `dcc-mcp-server` binary.
See [`docs/guide/production-deployment.md`](../../docs/guide/production-deployment.md)
§3 for the full operational context.

## Install

```bash
# 1. Install the binary (built via `cargo build --release --bin dcc-mcp-server`)
sudo install -m0755 target/release/dcc-mcp-server /usr/local/bin/dcc-mcp-server

# 2. Drop in the unit
sudo install -m0644 examples/systemd/dcc-mcp-gateway.service \
  /etc/systemd/system/dcc-mcp-gateway.service

# 3. Enable & start
sudo systemctl daemon-reload
sudo systemctl enable --now dcc-mcp-gateway.service
```

## Override per host

```bash
sudo systemctl edit dcc-mcp-gateway.service
```

```ini
[Service]
# Put registry on shared storage for HA deployments
Environment=DCC_MCP_REGISTRY_DIR=/mnt/shared/dcc-mcp/registry
# Bind publicly (only when behind a TLS-terminating proxy)
ExecStart=
ExecStart=/usr/local/bin/dcc-mcp-server --host 0.0.0.0 --dcc generic
```

Reload afterwards:

```bash
sudo systemctl daemon-reload
sudo systemctl restart dcc-mcp-gateway.service
```

## Smoke test

```bash
systemctl status dcc-mcp-gateway.service
curl -sf http://127.0.0.1:9765/health
# → {"ok":true}
curl -s http://127.0.0.1:9765/instances | jq
journalctl -u dcc-mcp-gateway.service -n 50 --no-pager
```

## Hardening summary

The unit applies `DynamicUser`, `ProtectSystem=strict`, `NoNewPrivileges`,
full `CapabilityBoundingSet=` (empty), syscall filtering, and read-only
root — the service cannot write outside `/var/lib/dcc-mcp` and
`/var/log/dcc-mcp`.

If you need to relax a restriction (e.g. to mount shared storage from an
NFS helper), copy the setting into your `systemctl edit` override rather
than modifying the shipped unit.
