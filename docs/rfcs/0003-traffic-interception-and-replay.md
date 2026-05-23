# RFC 0003 - Traffic Interception & Agent Debugging

**Status**: Draft
**Target repo**: `dcc-mcp-core` (with admin UI surface in `dcc-mcp-server`)
**Authors**: dcc-mcp-core contributors
**Date**: 2026-05-23
**Depends on**: RFC 0002 (event bus). This RFC is the highest-fidelity sink that consumes the bus.

---

## Summary

When an agent calls our skills and the result is wrong (skill returned
poor data, agent re-prompted in a way we didn't predict, latency
spiked, a notification we sent was misread), the **only** way to debug
today is to add `print` to skill scripts and tail Maya's stdout. There
is no first-class way to see **what the agent actually sent, what we
sent back, in what order, with what timing**.

This RFC proposes an **opt-in, dev-grade traffic interceptor** that
captures every byte the gateway exchanges with MCP clients and every
tool dispatch it forwards to a DCC adapter, into a structured store
that supports:

- **Live inspection** in the existing admin UI (`/admin`)
- **Recording** to JSONL / SQLite for later analysis
- **Replay** against a mock client to verify skill fixes deterministically
- **Diff** between two captures to verify a prompt or skill change does what we think

It is **explicitly a development & debugging tool**, not a production
audit log - different file from RFC 0002's webhook-style event log.

## Motivation - what studio agent authors actually need

From real downstream Maya skill iteration:

- "The agent called `bootstrap_project` with `enable_debugpy=true` but
  my report says `debugpy.status=error`. Did the agent actually send
  `enable_debugpy=true`, or did it default? Did my schema description
  mislead it?" - answer requires the **raw JSON-RPC payload**.
- "I changed my skill's description from X to Y. Did the agent's
  retrieval / selection actually change?" - answer requires
  **diffing two captures** of the same prompt before and after.
- "Cursor is showing 'Server not ready' but `/health` says OK." -
  answer requires the **SSE notification stream** that the gateway
  sent, in order, with timing.
- "My skill returns `{success: true}` but the agent's user-facing
  message says 'I couldn't do that'. What does the agent see vs what
  I sent?" - answer requires the **server-side response payload**,
  including any framing/transformation done by the gateway.
- "I want to write a regression test: when the agent says
  'screenshot the shelves view', it should end up calling
  `maya_pipeline__create_shelves` then `maya_dev__capture_ui`."
  - answer requires **deterministic replay** of a recorded session.

The unifying observation: prompt-engineering and skill design are
**empirical** disciplines today. Without a tape recorder we are
designing in the dark.

## Constraints

Same as RFC 0001 / RFC 0002, plus these additional constraints:

1. **Dev-grade, not prod-grade.** It is acceptable for the interceptor
   to add 5-20% latency, double memory for the capture buffer, and ship
   a SQLite file that grows without bound during a recording session.
   The interceptor must be **off by default** and refuse to start if
   `DCC_MCP_PROD_PROFILE=1` unless `DCC_MCP_FORCE_TRAFFIC_CAPTURE=1`
   is *also* set.

2. **PII / scene-content safety.** Recordings may contain artist names,
   project paths, scene-graph snippets, and screenshots. The capture
   subsystem must support **redact rules** (regex / key-path) applied
   at write time, *not* read time - once written, the file is the
   ground truth.

## Non-goals

- Production audit log (use RFC 0002 webhooks -> durable sink).
- Security tracing or SIEM integration.
- Cross-host distributed tracing (use OpenTelemetry).

---

## Design

### 1. Capture surface

The interceptor taps three points in the gateway request/response
pipeline. All three are **already** instrumented in the trace logs
(see `dcc_mcp_http::trace::on_request/on_response` in the live logs)
- this RFC formalises and structures them.

```
+---- MCP client (Cursor) ----+
| HTTP POST /mcp              |     -- (1) capture inbound MCP envelope
| GET  /mcp  (SSE)            |     -- (2) capture outbound SSE frames
+------------+----------------+
             v
       +------------+
       |  gateway   |
       +-----+------+
             v
+---- DCC adapter ------------+
| HTTP POST /v1/call          |     -- (3) capture forwarded tool call
|         /v1/load_skill etc. |         and its response
+-----------------------------+
```

Capture frame schema:

```jsonc
{
  "schema_version": 1,
  "capture_id":  "cap_01HQX...",          // ULID
  "session_id":  "cap_session_01HQX...",  // groups frames from one recording run
  "timestamp_ns": 1779478215123456789,
  "direction":   "inbound" | "outbound" | "internal",
  "leg":         "client_to_gateway" | "gateway_to_client_sse"
               | "gateway_to_adapter" | "adapter_to_gateway",
  "transport":   "http" | "sse",
  "http": {
    "method":  "POST",
    "url":     "http://127.0.0.1:9765/v1/call",
    "headers": { "content-type": "application/json", "mcp-session-id": "..." },
    "status":  null  // request side
  },
  "mcp": {
    "kind":    "request" | "response" | "notification",
    "method":  "tools/call",
    "id":      42,
    "session_id": "fc4c2da2-..."
  },
  "body": {
    "encoding": "json" | "base64" | "text",
    "data":     "...",            // see section 3 PII rules
    "size_bytes": 1234,
    "redacted_paths": ["params.arguments.api_key"]  // recorded redactions
  },
  "correlation": {
    "request_id": "cd4aacff-...",
    "trace_id":   "abc123..."
  }
}
```

Same `EventBus` envelope conventions as RFC 0002 - capture frames are
emitted on `traffic.frame` and any subscriber (notably the writers in
section 2) consumes them.

### 2. Sinks

The interceptor is a thin emitter on `traffic.frame`. Any number of
writers can subscribe. We ship four:

| Writer        | Format                   | When to use                                        |
| ------------- | ------------------------ | -------------------------------------------------- |
| `file_jsonl`  | newline-delimited JSON   | Quick local capture, `tail -f` friendly, git-able  |
| `sqlite`      | one row per frame        | Replay / diff (indexed by session, method, leg)    |
| `admin_live`  | in-memory ring buffer    | Real-time admin UI inspector                       |
| `ot_exporter` | OT span events           | Cross-correlate with existing OpenTelemetry tools  |

Configured via env var (single-sink quick mode) or YAML
(multi-sink studio mode):

```bash
# Quick mode
DCC_MCP_TRAFFIC_CAPTURE=jsonl:./capture.jsonl
DCC_MCP_TRAFFIC_FILTER=skill=maya-pipeline-dev

# Studio mode: declarative config
DCC_MCP_TRAFFIC_CONFIG=./traffic_capture.yaml
```

```yaml
# traffic_capture.yaml
enabled: true
sinks:
  - kind: sqlite
    path: ./captures/run-${TIMESTAMP}.db
  - kind: admin_live
    ring_buffer: 5000
filters:
  include:
    - mcp.method: tools/call
    - mcp.method: notifications/*
  exclude:
    - http.url: "*/v1/readyz"
    - http.url: "*/v1/resources"   # noisy poll
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
  - http.headers.authorization: "[REDACTED]"
  - body.data.params.arguments.scene_path: "[SCRUBBED:path]"
```

### 3. PII & redaction

Redactions are applied **before** writing to any sink. Three rule kinds:

- **Exact-key replacement**: `body.data.params.arguments.api_key ->
  "[REDACTED]"`
- **Regex on string values**: any `\S+@\S+\.\S+` -> `[email]`
- **Type tags**: `[SCRUBBED:path]` rewrites filesystem paths to anchored
  workspace-relative form (`/Users/.../my_project/` -> `<workspace>/`).
  Useful for shareable bug reports.

Captures that pass through redactions carry `body.redacted_paths` so a
replay tool knows which fields were modified and can warn if a
downstream test depends on them.

### 4. Replay

A CLI (`dcc-mcp capture replay <capture.db>`) drives a recorded
session against a live gateway:

```bash
# Replay session "sess_01HQX..." against the local gateway,
# using the recorded MCP messages as inputs and asserting outputs match.
dcc-mcp capture replay run-2026-05-23.db \
    --session sess_01HQX... \
    --target http://127.0.0.1:9765/mcp \
    --assert outputs_equal \
    --rebind-instance-id current_live

# Modes:
#   --assert outputs_equal       fail if any response body differs
#   --assert outputs_compatible  loose match (status code + json shape)
#   --assert outputs_ignored     fire-and-forget regression smoke
```

`--rebind-instance-id` substitutes the recorded instance id with the
current live one (instance ids are necessarily different on replay).

### 5. Diff

```bash
dcc-mcp capture diff before.db after.db --session-pair s1 s2

# Outputs a structured diff:
#   matched 47 of 50 frames
#   frame 7:  tool.dispatched maya_pipeline__create_shelves
#     args:   force_path_inject changed false -> true
#   frame 9:  tool.completed
#     result: success true -> false
#     reason: ModuleNotFoundError("pymel")
```

This is the workflow for "I changed my skill description / I changed my
prompt / I bumped shelf tools - did anything observable change?"

### 6. Admin UI live inspector

Today `/admin` shows tasks. Add a `/admin/traffic` page powered by the
`admin_live` ring-buffer sink:

- Tab `Live` - websocket-streamed frames as they arrive
- Tab `Sessions` - group by `session_id`, expandable timeline
- Tab `Frame detail` - full JSON of any frame, with diff link to
  another frame
- Tab `Export` - download last N frames as JSONL / SQLite

A small **"Privacy"** banner at the top reminds the operator the
session may contain artist data and links to the redact rules in
effect.

---

## Phasing

- **P0** - `traffic.frame` event on the EventBus (depends on RFC 0002
  P0) + `file_jsonl` sink + env-var quick mode. ~300 lines. Brings
  the "tail -f my capture" use case in one PR.
- **P1** - `sqlite` sink + filters + redact rules. ~250 lines.
- **P2** - replay CLI. ~400 lines.
- **P3** - diff CLI. ~250 lines.
- **P4** - admin UI Live inspector + WebSocket fan-out. ~600 lines.
- **P5** - OT exporter. ~150 lines.

P0 alone unlocks the "what did the agent actually send" question.

## Backward compatibility

- The interceptor is **opt-in**; default OFF.
- When OFF, the capture taps are no-ops behind a single boolean check.
- Capture file format carries `schema_version=1`; breaking changes bump
  it; replay/diff tools support reading old versions for at least two
  bumps.

## Open questions

1. **What "outbound" really means for streaming responses**. A large
   tool result may stream out as multiple SSE frames; do we record one
   frame per SSE chunk (verbose, lossless) or one logical frame
   reconstructed by the gateway (denser, easier to diff but lossy on
   timing)? Suggest: capture both, distinguish via `frame.transport`.

2. **Replay against a different DCC**. Should `replay` allow targeting
   a different `dcc_type` than was recorded (e.g. recorded against
   Maya, replay against Blender) for adapter portability testing?
   Probably yes with `--allow-cross-dcc` flag plus a warning when tool
   slugs differ.

3. **Capture and the session resume from RFC 0001**. If a client
   resumes a session mid-recording, do we record the resume as a new
   capture session or stitch it back? Suggest: stitch, key by the
   pre-resume `session_id`.

4. **Capture file sharing**. The natural studio workflow is "I hit a
   weird agent behavior, I send my capture to the skill author."
   Should we ship a one-liner `dcc-mcp capture share <file>` that
   redacts default-sensitive fields and uploads to studio
   internal storage? Out of scope of this RFC but pencil it in.

5. **Capture of `before(...)` veto decisions**. RFC 0002 lets
   subscribers veto tool dispatch. If a tool is vetoed, do we record
   the request that *would* have been forwarded? Yes - emit
   `traffic.frame` with `direction=internal` and an `outcome=vetoed`
   field.

## Acknowledgements

Originated from prompt-engineering iteration on downstream Maya skill
descriptions where the only feedback channel was Cursor's user-facing
chat output - which is precisely the wrong signal to optimise against.
The agent's *raw protocol* is the ground truth and we need it.
