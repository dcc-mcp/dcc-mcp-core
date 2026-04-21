# Production Deployment

> **[中文版](../zh/guide/production-deployment)**

This guide covers deploying the standalone `dcc-mcp-server` binary in
production: as a bare binary, in Docker, under systemd, and behind a load
balancer for high-availability multi-gateway topologies.

For the gateway election protocol itself (how a single process wins the
well-known port) see [Gateway Election](gateway-election.md). This page is
about the **operational** side: how to run N of these processes safely.

## When to Read This

- You are packaging `dcc-mcp-server` into a Docker image or OS service.
- You need more than one gateway replica behind nginx / ALB for HA.
- You want documented TLS, firewall, log, and upgrade procedures.

If you just want to run the server on a developer laptop, start with
[Getting Started](getting-started.md) instead.

---

## 1. Binary Deployment

### Building the Binary

The binary is the Rust bin crate [`crates/dcc-mcp-server/`](https://github.com/loonghao/dcc-mcp-core/tree/main/crates/dcc-mcp-server).
It is statically-linked apart from the platform libc and needs no Python
runtime.

```bash
# Release build (recommended for production)
cargo build --release --bin dcc-mcp-server

# Resulting binary
# Linux / macOS: target/release/dcc-mcp-server
# Windows:       target\release\dcc-mcp-server.exe
```

Cross-compiling for Linux from any host:

```bash
cargo install cross --locked
cross build --release --bin dcc-mcp-server --target x86_64-unknown-linux-gnu
```

### Install Location

Pick **one** location consistently per machine:

| Platform | Suggested path |
|----------|---------------|
| Linux (system-wide) | `/usr/local/bin/dcc-mcp-server` |
| Linux (per-user) | `~/.local/bin/dcc-mcp-server` |
| Container image | `/usr/local/bin/dcc-mcp-server` |
| Windows | `C:\Program Files\dcc-mcp\dcc-mcp-server.exe` |

### Environment Variables

The binary is configured entirely through CLI flags or environment variables
(flags win). Only the variables listed here are stable; see
`dcc-mcp-server --help` for the full set.

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_MCP_PORT` | `0` (OS-assigned) | Per-instance MCP HTTP port |
| `DCC_MCP_GATEWAY_PORT` | `9765` | Well-known port the gateway competes for; `0` disables the gateway |
| `DCC_MCP_REGISTRY_DIR` | platform default | Shared `FileRegistry` directory — **must** be identical across replicas that need to see each other |
| `DCC_MCP_STALE_TIMEOUT` | `30` | Seconds without a heartbeat before an instance is considered dead |
| `DCC_MCP_HEARTBEAT_INTERVAL` | `5` | Heartbeat period in seconds |
| `DCC_MCP_SERVER_NAME` | `dcc-mcp-server` | Name advertised to MCP clients |
| `DCC_MCP_DCC` | *(empty)* | DCC hint: `maya`, `blender`, `photoshop`, … |
| `DCC_MCP_DCC_VERSION` | — | Reported in registry entry |
| `DCC_MCP_SCENE` | — | Current scene file reported to the gateway |
| `DCC_MCP_SKILL_PATHS` | — | `:` / `;` separated extra skill directories |
| `DCC_MCP_LOG_FILE` | `false` | Enable rotating file logs in addition to stderr |
| `DCC_MCP_LOG_DIR` | platform log dir | Where rotated logs are written |

### Smoke Test

```bash
# Terminal 1
dcc-mcp-server --dcc generic --mcp-port 18812

# Terminal 2
curl -sf http://127.0.0.1:9765/health   # → {"ok":true}
curl -s http://127.0.0.1:9765/instances # → JSON list of registered instances
```

---

## 2. Docker

### Multi-Stage Image

See [`examples/compose/gateway-ha/Dockerfile`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha)
for the canonical image. Sketch:

```Dockerfile
# Stage 1 — build
FROM rust:1.85-slim AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --bin dcc-mcp-server

# Stage 2 — runtime
FROM debian:12-slim
RUN useradd --system --uid 10001 --home-dir /var/lib/dcc-mcp dcc
COPY --from=builder /src/target/release/dcc-mcp-server /usr/local/bin/
USER dcc
EXPOSE 9765
ENTRYPOINT ["/usr/local/bin/dcc-mcp-server"]
```

Build and run once:

```bash
docker build -t dcc-mcp-server:latest -f examples/compose/gateway-ha/Dockerfile .
docker run --rm -p 9765:9765 dcc-mcp-server:latest \
  --dcc generic --host 0.0.0.0
```

### docker-compose for HA

The [`examples/compose/gateway-ha/docker-compose.yml`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha)
file brings up **two gateway candidates** (both compete for `9765`; one
wins, the other becomes a plain instance that can take over on failure)
plus **two mock DCC servers**, all sharing a single registry volume.

```bash
cd examples/compose/gateway-ha
docker compose up -d
curl http://localhost:9765/health
curl http://localhost:9765/instances
docker compose down
```

---

## 3. systemd

Use systemd on bare-metal or long-lived VMs where you want the OS to keep
`dcc-mcp-server` alive. The canonical unit lives in
[`examples/systemd/dcc-mcp-gateway.service`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/systemd).

The unit enables these hardening options:

- `DynamicUser=true` — the service gets an auto-created, unprivileged user.
- `ProtectSystem=strict` + `ProtectHome=true` — read-only `/`, no `/home`.
- `NoNewPrivileges=true` — the process cannot gain capabilities via `exec`.
- `PrivateTmp=true`, `PrivateDevices=true`, `ProtectKernelTunables=true`.
- `StateDirectory=dcc-mcp` — writable `/var/lib/dcc-mcp` for the registry.
- `CapabilityBoundingSet=` — no capabilities at all.

Install and enable:

```bash
sudo install -m0644 examples/systemd/dcc-mcp-gateway.service \
  /etc/systemd/system/dcc-mcp-gateway.service
sudo systemctl daemon-reload
sudo systemctl enable --now dcc-mcp-gateway.service
systemctl status dcc-mcp-gateway.service
journalctl -u dcc-mcp-gateway.service -f
```

Override per host (port, skill paths, etc.) with a drop-in:

```bash
sudo systemctl edit dcc-mcp-gateway.service
# [Service]
# Environment=DCC_MCP_GATEWAY_PORT=9765
# Environment=DCC_MCP_REGISTRY_DIR=/var/lib/dcc-mcp/registry
# Environment=DCC_MCP_SKILL_PATHS=/opt/skills:/etc/skills
```

---

## 4. Load Balancer

### MCP Session Stickiness

The MCP Streamable HTTP transport (spec
[2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26))
carries a session identifier in the `Mcp-Session-Id` HTTP header. The
initialization response sets it; every subsequent `POST /mcp` and the
long-lived `GET /mcp` SSE stream carry the same header back. All requests
with the same `Mcp-Session-Id` **must** reach the same gateway replica, or
SSE events will not be delivered.

Hash on `Mcp-Session-Id` rather than client IP — a single office behind a
NAT gateway may present as one source IP yet host many independent
sessions.

### nginx

```nginx
upstream dcc_mcp_gateways {
    hash $http_mcp_session_id consistent;
    server 10.0.0.11:9765 max_fails=2 fail_timeout=5s;
    server 10.0.0.12:9765 max_fails=2 fail_timeout=5s;
    keepalive 16;
}

server {
    listen 443 ssl http2;
    server_name mcp.example.com;

    ssl_certificate     /etc/ssl/mcp.example.com.crt;
    ssl_certificate_key /etc/ssl/mcp.example.com.key;

    # Passive health — drop a backend after 2 failures for 5s.
    # Active health — Nginx OSS users can use a cron curl loop or
    # the upstream's /health endpoint from an external probe.

    location /mcp {
        proxy_pass http://dcc_mcp_gateways;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header Mcp-Session-Id $http_mcp_session_id;

        # SSE: disable buffering and keep the connection long.
        proxy_buffering  off;
        proxy_cache      off;
        proxy_read_timeout 1h;
        proxy_send_timeout 1h;
        chunked_transfer_encoding on;
    }

    location = /health {
        proxy_pass http://dcc_mcp_gateways;
    }
}
```

### AWS Application Load Balancer

ALB has no built-in hash-on-header mode, so use **application cookie
stickiness** anchored to the session header:

1. Target group → Attributes → **Stickiness: enabled**, type
   `app_cookie`, cookie name `Mcp-Session-Id`, duration `3600s`.
2. Health check path `/health`, response code `200`, interval `10s`,
   threshold `2`.
3. Listener on `:443` terminates TLS; forwards HTTP to targets on `:9765`.
4. Idle timeout ≥ `3600s` so SSE streams are not severed.

Optional: put a CloudFront distribution in front for DDoS protection and
cache `/health` for `10s`.

---

## 5. Gateway HA Topology (#327)

This is the topology originally planned for issue #327 and merged into
issue #330. It is the only way to scale gateway throughput past one process
while keeping a single public endpoint.

```
                ┌──────────────────┐
   clients ───▶ │  LB (nginx/ALB)  │
                └──────────────────┘
                   │          │
                   ▼          ▼
         ┌───────────┐  ┌───────────┐
         │ gateway-a │  │ gateway-b │    dcc-mcp-server replicas
         └───────────┘  └───────────┘    (stateless, read-mostly)
                 \            /
                  \          /
                ┌──────────────────┐
                │ shared registry  │    NFS / EFS / S3-mountpoint
                │  (FileRegistry)  │    Mount path = DCC_MCP_REGISTRY_DIR
                └──────────────────┘
                   ▲          ▲
                   │          │
         ┌───────────┐  ┌───────────┐
         │ dcc-maya-1│  │ dcc-blender-1 │ DCC instances register themselves
         └───────────┘  └───────────┘
```

### Sharing the FileRegistry

Every replica **must** point `DCC_MCP_REGISTRY_DIR` at the same POSIX
directory, exposed through a filesystem with working `fsync`:

- Kubernetes: `ReadWriteMany` PVC (CephFS, EFS CSI, Longhorn RWX).
- Bare-metal: NFSv4 or GlusterFS.
- AWS: EFS mounted via NFSv4.1.
- On-prem AWS-alike: S3 Mountpoint is acceptable but slow; prefer EFS.

Do **not** use S3 object stores without a POSIX layer — the registry relies
on atomic rename and directory listing.

### Election & Duplicate-Tool Suppression

All replicas run the same election code described in
[`gateway-election.md`](gateway-election.md):

1. Each replica tries to bind `DCC_MCP_GATEWAY_PORT` on its own
   pod/container IP. On a single-host compose file only one succeeds.
2. In the LB topology each replica is on a distinct IP, so they all bind
   successfully and each pretends to be "the gateway for this pod". The
   LB hides this from clients.
3. Every replica reads the same shared `FileRegistry`, so every replica
   sees **the same set of DCC instances and the same set of tools**.
4. Tools are namespaced `{instance_short_id}__{tool}` — the short id is
   derived deterministically from the instance registration, so two
   replicas publishing the same DCC instance produce **identical** tool
   names. MCP clients deduplicate by name; there are no duplicates in
   `tools/list`.
5. `tools/list_changed` notifications fire on whichever replica the SSE
   stream is pinned to, driven by registry file-watch events.

### Failover SLA

Target: **< 5 seconds from pod death to LB drop**.

- LB health check interval `2s`, unhealthy threshold `2` → ≤ `4s` to
  stop routing to the dead replica.
- Existing SSE streams pinned to the dead replica are severed; clients
  see a disconnect and reconnect — the LB routes them to a healthy
  replica using the `Mcp-Session-Id` cookie/hash.
- Registry entries of a truly-dead DCC are pruned after
  `DCC_MCP_STALE_TIMEOUT` (default `30s`) by any live gateway replica;
  this is independent of LB failover.

Tune `DCC_MCP_HEARTBEAT_INTERVAL=2` and `DCC_MCP_STALE_TIMEOUT=10` in
aggressive-failover deployments. Do not go below `1s` heartbeats — you
will saturate the shared registry directory.

---

## 6. Security

- **Bind privately**. The binary defaults to `127.0.0.1`. In containers
  set `--host 0.0.0.0` only when the container network is private.
- **TLS at the edge only**. Terminate TLS at the LB (nginx / ALB).
  `dcc-mcp-server` speaks plaintext HTTP on the loopback / pod network
  for simplicity and performance.
- **Firewall**. Expose only the LB's public port (443). Block `9765`
  and each replica's `--mcp-port` from the public internet.
- **Authentication**. The MCP spec defines OAuth 2.1 bearer tokens —
  enforce them at the LB if you expose the endpoint beyond your VPC.
- **systemd hardening** (section 3) plus `DynamicUser=true` keeps a
  compromised process off the rest of the host.
- **No secrets in env**. Prefer file-based config mounted via systemd
  `LoadCredential=` or Kubernetes Secret mounts.

---

## 7. Monitoring

### Logs

- **systemd** — `journalctl -u dcc-mcp-gateway.service -f`.
- **Docker / compose** — `docker compose logs -f gateway-a`.
- **Kubernetes** — `kubectl logs -f deploy/dcc-mcp-gateway`.
- **Rotating file logs** — set `DCC_MCP_LOG_FILE=true` and
  `DCC_MCP_LOG_DIR=/var/log/dcc-mcp`. Ship with promtail / fluentbit.

### Health and Readiness Probes

The gateway exposes `GET /health` returning `{"ok":true}` with status
`200`. Use it for both liveness and readiness.

```yaml
readinessProbe:
  httpGet: { path: /health, port: 9765 }
  initialDelaySeconds: 2
  periodSeconds: 5
  failureThreshold: 2
livenessProbe:
  httpGet: { path: /health, port: 9765 }
  initialDelaySeconds: 10
  periodSeconds: 10
  failureThreshold: 3
```

> **Note**: There is no `/mcp/healthz` endpoint today — the LB-friendly
> path is `/health`. A dedicated `/readyz` that also checks registry
> reachability is tracked as a follow-up.

### Metrics

Prometheus scrape support is tracked separately. Until then, use the
telemetry facility (`DCC_MCP_LOG_FILE=true` + log-based metrics) or
derive request counts from the LB's access log.

---

## 8. Upgrade & Rollback

Zero-downtime via LB draining. With two replicas behind one LB:

```bash
# 1. Drain replica A
#    nginx: remove from upstream, reload
#    ALB:   deregister target, wait for "draining" to finish
#
# 2. Upgrade & restart replica A with the new binary
#
# 3. Probe
curl -sf http://<replica-a-ip>:9765/health

# 4. Put replica A back in rotation
# 5. Repeat for replica B
```

Rollback is the same procedure with the previous binary. Because the
registry on disk is versioned defensively (unknown fields are ignored),
rolling between adjacent minor versions is safe. For major-version jumps
consult the release notes.

---

## See Also

- [Gateway Election](gateway-election.md) — how the well-known port is claimed.
- [Transport Layer](transport.md) — IPC between DCC processes.
- [MCP 2025-03-26 spec](https://modelcontextprotocol.io/specification/2025-03-26) — Streamable HTTP, `Mcp-Session-Id`.
- Example artifacts: [`examples/compose/gateway-ha/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha), [`examples/k8s/gateway-ha/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/k8s/gateway-ha), [`examples/systemd/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/systemd).
