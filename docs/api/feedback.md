# Feedback API

Agent feedback and rationale utilities for DCC-MCP servers (issues #433, #434).

Three complementary features: **Rationale capture** — agents include `_meta.dcc.rationale` in `tools/call` requests to explain why they are invoking a tool. **Gateway feedback** — `POST /v1/feedback` and `dcc-mcp-cli feedback` remain available with zero live DCC instances. **Compatibility forwarder** — `dcc_feedback__report` remains available on live adapters and forwards through the same gateway contract.

**Exported symbols:** `clear_feedback`, `extract_rationale`, `get_feedback_entries`, `make_rationale_meta`, `register_feedback_tool`

## register_feedback_tool

```python
register_feedback_tool(
    server,
    *,
    dcc_name="dcc",
    gateway_endpoint=None,
    gateway_host=None,
    gateway_port=None,
    instance_id_provider=None,
) -> None
```

Register the `dcc_feedback__report` MCP tool on `server`. Call **before** `server.start()`.

The tool accepts `tool_name`, `intent`, `blocker`, `severity` (`"blocked"` | `"workaround_found"` | `"suggestion"`), optional `attempt`, and optional failed-call `request_id` / `job_id`. The shared Core handler attaches the adapter `dcc_type` and current `instance_id`, posts to gateway `/v1/feedback`, validates the exact `X-Request-ID` response correlation, and returns the gateway receipt. It fails closed when the gateway is disabled, unavailable, rejects the report, or returns a stale/malformed receipt. There is no instance-local success fallback.

## Agent failure-reporting workflow

Shell-capable agents use the published `dcc-mcp` Skill and existing CLI
surfaces:

```bash
dcc-mcp-cli doctor
dcc-mcp-cli stats --range 24h --status failure --session-id <session-id>
dcc-mcp-cli feedback \
  --tool-name tool_that_failed \
  --intent "goal" \
  --attempt "sanitized attempt" \
  --blocker "observed failure" \
  --severity blocked \
  --dcc-type <dcc> \
  --instance-id <live-or-dead-instance-id> \
  --request-id <request-id> \
  --job-id <job-id>
```

The gateway records a bounded `feedback_reported` entry in
`resources://gateway/events` and returns its `feedback_id`; it does not require
a live DCC and does not create an external issue. Use sanitized values and never
include credentials, reusable tokens, or raw sensitive payloads. For a
gateway-routed failure, preserve the CLI-returned
`request_id` and retrieve public-safe
`/v1/debug/issue-reports/<request_id>`. Review `?mode=raw` locally and never
upload it automatically. Route Skill defects to the owning Skill, adapter or
host-runtime defects to the adapter repository, and shared CLI/gateway/protocol
defects to `dcc-mcp-core`; create an external issue only with user authorization.

Adapters also mirror accepted reports to bounded, rotated JSONL files below the
shared registry directory. Query those durable records through the gateway:

```bash
dcc-mcp-cli feedback list --range 7d --dcc maya --severity blocked --json
dcc-mcp-cli feedback export --range all --dcc maya --json
```

Both commands call `GET /admin/api/feedback`. `list` defaults to 100 rows and
`export` to the endpoint maximum of 1,000. The response is newest first,
deduplicated by feedback id, and reports `skipped_invalid`, `deduplicated`, and
`files_scanned` counters. Malformed or oversized individual records never enter
the result; directory/file I/O errors or exceeded scan bounds fail explicitly
instead of returning a silently incomplete export.

The compatibility `dcc_feedback__report` entry point disappears with its live
adapter, but while live it is only a thin forwarder to the gateway authority.
Prefer the gateway CLI/REST path for crash-class feedback and reference the dead
instance plus its last request/job ids directly.

## extract_rationale

```python
extract_rationale(params: dict | str) -> str | None
```

Extract `_meta.dcc.rationale` from a `tools/call` params dict.

```python
params = {
    "name": "create_sphere",
    "arguments": {"radius": 1.0},
    "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
}
rationale = extract_rationale(params)  # "User wants a reference sphere."
```

## make_rationale_meta

```python
make_rationale_meta(rationale: str) -> dict
```

Build the `_meta` fragment for a `tools/call` request with a rationale. Returns `{"_meta": {"dcc": {"rationale": "..."}}}`.

## get_feedback_entries

```python
get_feedback_entries(*, tool_name=None, severity=None, limit=50) -> list[dict]
```

Return recent feedback entries, newest first. Each entry has keys: `id`, `timestamp`, `tool_name`, `intent`, `attempt`, `blocker`, `severity`.

## clear_feedback

```python
clear_feedback() -> int
```

Clear all in-memory feedback entries. Returns the count cleared.
