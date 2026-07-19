# Observability Troubleshooting

Operational runbook for diagnosing and resolving observability issues.

---

## T1. Metric is missing or shows zero

### T1.1 Prometheus endpoint returns 404

**Check**: Is the `prometheus` Cargo feature enabled?

```bash
# Verify the binary was built with prometheus support
strings /path/to/dcc-mcp-server | grep prometheus
# No output → feature not compiled in

# Check the log for prometheus initialization
grep -i prometheus /var/log/dcc-mcp/gateway.log
```

**Fix**: Rebuild with `--features prometheus`, or use a wheel that includes it:

```bash
maturin develop --features python-bindings,ext-module,workflow,prometheus
```

**Also check**: Did you set `enable_prometheus=True` in `McpHttpConfig`?

```python
cfg = McpHttpConfig(
    port=8765,
    server_name="maya-mcp",
    enable_prometheus=True,  # required
)
```

### T1.2 `dcc_mcp_build_info` is the only metric visible

The exporter publishes `dcc_mcp_build_info` on startup as a heartbeat to
confirm the endpoint is live. If only this metric appears:

- No tool calls have been dispatched yet — metrics are created lazily on
  first observation.
- Call a tool and scrape again.

### T1.3 `ObservabilityQuery` returns all zeroes

**Check**: Does the SQLite database exist?

```bash
ls -la /tmp/dcc-mcp-registry/gateway_admin.sqlite
# or
echo $DCC_MCP_GATEWAY_ADMIN_DB
```

**Check**: Has the gateway ever run? The SQLite file is created on the first
gateway startup. If it does not exist, no data has been collected.

```python
from dcc_mcp_core.admin_sqlite_lane import resolve_admin_db_path
db_path = resolve_admin_db_path()
print("DB exists:", db_path.exists())
```

**Fix**: Start the gateway process and make some tool calls, then retry the
query.

### T1.4 Query returns zeroes but database has rows

Verify the `ObservabilityQuery` is pointed at the correct database path:

```python
from dcc_mcp_core import ObservabilityQuery
q = ObservabilityQuery(db_path="/actual/path/gateway_admin.sqlite")
```

The default constructor with `db_path=None` and no `read_json_fn` returns
empty results. Either provide `db_path` (for direct SQLite access) or a
`read_json_fn` callback.

---

## T2. OTLP traces not appearing

### T2.1 Spans not reaching the collector

**Check**: Is `OTEL_EXPORTER_OTLP_ENDPOINT` set?

```bash
echo $OTEL_EXPORTER_OTLP_ENDPOINT
# Must be set — OTLP is opt-in by environment variable
```

**Check**: Is the collector reachable?

```bash
# Test gRPC connectivity (requires grpcurl)
grpcurl -plaintext localhost:4317 grpc.health.v1.Health/Check

# Test HTTP collector
curl -s http://localhost:4318/health
```

**Check**: Gateway logs for OTLP errors:

```bash
grep -i otlp /var/log/dcc-mcp/gateway.log | tail -20
```

### T2.2 Spans reaching collector but not visible in Jaeger/Tempo

**Check**: Does the collector's service pipeline include an exporter?

```yaml
# otel-collector.yaml
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [otlphttp/jaeger]   # ← must be present
```

**Check**: Is the backend's query port accessible?

| Tool | UI port |
|------|---------|
| Jaeger | 16686 |
| Tempo | 3200 (also 4317 for OTLP) |
| Phoenix | 6006 |

### T2.3 Only some DCC instances produce traces

**Check**: Per-DCC environment:

```bash
# Each DCC process needs its own OTEL_* vars
# Gateway passes through OTEL environment to child processes
# But some DCCs may strip environment. Verify:
cat /proc/<dcc-pid>/environ | tr '\0' '\n' | grep OTEL
```

---

## T3. Admin UI issues

### T3.1 `GET /admin` returns 404

**Check**: Is the gateway process running?

```bash
ps aux | grep dcc-mcp-server
```

**Check**: What port is the gateway on?

```bash
# Default port is 0 (OS-assigned). Check the gateway log:
grep "listening on" /var/log/dcc-mcp/gateway.log
```

**Fix**: The admin UI is served by the elected gateway, not by per-DCC
servers. Connect to the gateway's port, not a DCC server's port.

### T3.2 Admin tables are empty ("No data")

**Check**: Audit ring buffer capacity:

```bash
# Default is 5000 rows. Check if the ring has ever been populated:
curl -s http://localhost:8765/admin/api/calls | python -c "import sys,json; print(len(json.load(sys.stdin)))"
```

**Check**: Has `DCC_MCP_GATEWAY_AUDIT_DIR` been set? If not, data is
in-memory only and resets on gateway restart.

**Check**: Has traffic actually passed through the gateway?

```bash
# Verify the registry has backends registered
curl -s http://localhost:8765/admin/api/workers
```

### T3.3 Stats show 0 calls despite activity

The stats endpoint computes from the trace ring buffer (default 200 traces).
If your traffic exceeds 200 requests, older entries are evicted. Increase the
trace ring capacity:

```bash
DCC_MCP_TRACE_LOG_CAPACITY=2000 dcc-mcp-server
```

Or use the SQLite-backed path (`admin-persist-sqlite` feature) for access to
all historical data.

---

## T4. ToolRecorder (Python) issues

### T4.1 `metrics()` returns `None`

```python
metrics = recorder.metrics("create_sphere")
if metrics is None:
    print("This tool has never been recorded.")
```

**Fix**: Ensure `recorder.start("create_sphere", "maya")` was called and
`guard.finish()` was invoked. The metric is only registered after a
completed recording cycle.

### T4.2 Percentiles are always 0.0

Percentiles require a minimum sample count:

- p95 stabilises after ~20 samples.
- p99 stabilises after ~100 samples.

Below these thresholds, percentiles may default to 0.0 or match the maximum
observed value, depending on the internal histogram state.

### T4.3 `p95_duration_ms` equals `avg_duration_ms`

This happens when the sample count is too low for meaningful percentile
computation (< 20 samples). Increase the sample size or use
`avg_duration_ms` instead.

---

## T5. Schema / database issues

### T5.1 Cannot open `gateway_admin.sqlite`

**Check**: File permissions:

```bash
ls -la /tmp/dcc-mcp-registry/gateway_admin.sqlite
# Must be readable by the user running the agent/CLI
```

**Check**: WAL journal mode — the file may have `-wal` and `-shm`
companions:

```bash
ls -la /tmp/dcc-mcp-registry/gateway_admin.sqlite*
```

All three files must be present and accessible for read operations.

**Check**: Is another process holding a write lock?

```bash
# On Linux/Mac:
lsof /tmp/dcc-mcp-registry/gateway_admin.sqlite
```

### T5.2 `no such table: tool_calls`

The database was created by an older gateway version (schema v1). The DDL
is `CREATE TABLE IF NOT EXISTS`, so restarting the gateway creates missing
tables automatically.

```bash
# Verify current schema version
sqlite3 /tmp/dcc-mcp-registry/gateway_admin.sqlite ".tables"
# Should show: tool_calls, sessions, session_events, ...
```

**Fix**: Restart the gateway to trigger the DDL bootstrap.

### T5.3 Database is growing too large

The SQLite lane is append-only. If the database grows beyond acceptable
bounds, consider:

1. Adding a cleanup job:
   ```sql
   DELETE FROM tool_calls WHERE started_at_ms < strftime('%s', 'now', '-30 days') * 1000;
   DELETE FROM sessions WHERE started_at_ms < strftime('%s', 'now', '-30 days') * 1000;
   DELETE FROM session_events WHERE created_at_ms < strftime('%s', 'now', '-30 days') * 1000;
   ```

2. Running VACUUM after large deletes:
   ```sql
   VACUUM;
   ```

3. Building the gateway without `admin-persist-sqlite` to use only the
   in-memory ring buffer (data resets on restart).

---

## T6. Common configuration mistakes

| Symptom | Likely cause |
|---------|-------------|
| Prometheus endpoint 404 | `prometheus` feature not compiled or `enable_prometheus=False` |
| OTLP traces not appearing | `OTEL_EXPORTER_OTLP_ENDPOINT` not set |
| `ObservabilityQuery` returns zeros | Wrong `db_path` or gateway has never run |
| Admin UI "No data" | `DCC_MCP_GATEWAY_AUDIT_DIR` not set (in-memory only) |
| Stats inaccurate | Trace ring capacity too small (default 200) |
| SQLite not persisting | `admin-persist-sqlite` feature not compiled in |
| ToolRecorder percentiles zero | Fewer than 20 samples |
| `skill_paths_custom` reads fail | File exists but table missing; first gateway startup hasn't finished DDL |

---

## Quick diagnostic script

```python
"""Run this on a machine to verify observability is working."""
import os, sys
from pathlib import Path

# 1. Check Prometheus
port = os.environ.get("DCC_MCP_HTTP_PORT", "8765")
import urllib.request
try:
    resp = urllib.request.urlopen(f"http://localhost:{port}/metrics")
    print(f"[OK] Prometheus endpoint reachable on port {port}")
except Exception as e:
    print(f"[WARN] Prometheus endpoint: {e}")

# 2. Check OTLP
if "OTEL_EXPORTER_OTLP_ENDPOINT" in os.environ:
    print(f"[OK] OTLP configured: {os.environ['OTEL_EXPORTER_OTLP_ENDPOINT']}")
else:
    print("[INFO] OTLP not configured (optional)")

# 3. Check SQLite
from dcc_mcp_core.admin_sqlite_lane import resolve_admin_db_path
db = resolve_admin_db_path()
if db.exists():
    print(f"[OK] Admin SQLite database exists: {db}")
else:
    print(f"[WARN] Admin SQLite database not found at {db}")

# 4. Check gateway admin UI
try:
    resp = urllib.request.urlopen(f"http://localhost:{port}/admin/api/calls")
    print(f"[OK] Gateway admin API reachable")
except Exception as e:
    print(f"[WARN] Gateway admin API: {e}")
```
