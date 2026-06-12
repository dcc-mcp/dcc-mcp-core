# Event System

DCC-MCP-Core provides a thread-safe `EventBus` for downstream extensions that
need to observe gateway, skill, or tool lifecycle without forking the server.
The bus supports exact subscriptions, `prefix.*` wildcard subscriptions, and a
catch-all `*` subscription.

## Legacy Keyword Events

`publish()` preserves the original lightweight API: subscribers are called with
the keyword arguments supplied by the publisher.

```python
from dcc_mcp_core import EventBus

bus = EventBus()

def on_scene_saved(**payload):
    print(payload["file_path"])

sub_id = bus.subscribe("scene.saved", on_scene_saved)
bus.publish("scene.saved", file_path="/tmp/scene.usda", size_kb=1024)
bus.unsubscribe("scene.saved", sub_id)
```

## Structured Lifecycle Events

`emit()` publishes an RFC-0002 event envelope and passes the same dict to every
matching subscriber.

```python
bus = EventBus()

def record_metric(event: dict) -> None:
    attrs = event["attributes"]
    print(event["name"], attrs["tool_slug"], attrs.get("duration_ms"))

bus.subscribe("tool.*", record_metric)
bus.emit(
    "tool.completed",
    source={"dcc_type": "maya"},
    correlation={"request_id": "req-123"},
    attributes={"tool_slug": "maya_scene__open", "result_success": True},
)
```

Every structured event uses this envelope shape:

```json
{
  "schema_version": 1,
  "name": "tool.completed",
  "id": "ev_...",
  "timestamp_ns": 1779478215123456789,
  "source": {},
  "correlation": {},
  "attributes": {}
}
```

## Built-In Emit Points

The core dispatcher and skill catalog emit these events when subscribers or
policy hooks are present:

| Event | Emitted when |
|-------|--------------|
| `tool.dispatched` | A tool call passed lookup, policy, and schema validation and is about to run |
| `tool.completed` | A tool handler returned successfully |
| `tool.failed` | Tool lookup, policy, validation, or handler execution failed |
| `skill.loading` | A skill load is beginning |
| `skill.loaded` | A skill loaded and registered its tools |
| `skill.unloaded` | A loaded skill was unloaded and its tools were removed |
| `skill.validation_failed` | A skill could not load because it was missing, had dependency issues, or failed setup validation |
| `traffic.frame` | Opt-in gateway traffic capture frame for MCP/REST debugging |

Tool lifecycle attributes include `tool_slug`, `tool_name`, `duration_ms`,
`result_success` on terminal events, and metadata such as `dcc_type`,
`skill_name`, `group`, and `annotations` when known. `tool.completed` derives
`result_success` from an output object's `success` boolean when present;
otherwise handler success defaults to `true`.

Skill lifecycle attributes include `skill_name`, `dcc_type`, `version`,
`skill_path`, declared/registered tool counts, registered tool names, and
failure details such as `error_kind` and `error_message`.

## Before Hooks And Vetoes

`before()` registers a synchronous policy hook for lifecycle points where an
operation can still be rejected. Unlike normal subscribers, before hooks are
blocking: return `None` or `False` to allow the operation, or return
`EventBus.veto(reason, code="...")`, a `{"reason": "...", "code": "..."}`
dict, or a string reason to veto it.

```python
from dcc_mcp_core import EventBus, ToolDispatcher, ToolRegistry

registry = ToolRegistry()
registry.register("delete_scene", dcc="maya")
dispatcher = ToolDispatcher(registry)

def block_destructive(event: dict):
    if event["attributes"]["tool_slug"] == "delete_scene":
        return EventBus.veto("destructive tools are disabled", "policy_denied")
    return None

dispatcher.event_bus().before("tool.dispatched", block_destructive)
```

Only these event names are vetoable:

| Event | Veto result |
|-------|-------------|
| `skill.loading` | Rejects the skill load before tools are registered |
| `tool.dispatched` | Rejects the tool call before the handler runs |
| `resource.subscribed` | Reserved for rejecting resource subscriptions |
| `client.initialize` | Reserved for rejecting client initialization |

When a veto rejects a tool call, the dispatcher returns a structured
`EVENT_VETOED` error and emits `tool.failed` with
`error_kind="event_vetoed"`, `veto_code`, and `veto_reason`. When
`skill.loading` is vetoed, the catalog rejects the load and emits
`skill.validation_failed` with the same veto fields.

## Gateway Traffic Capture

RFC 0003 adds an opt-in `traffic.frame` stream for local debugging. It is off
by default. Enable the quick JSONL sink when starting a gateway:

```bash
DCC_MCP_TRAFFIC_CAPTURE=jsonl:./capture.jsonl dcc-mcp-server ...
```

Each JSONL row is the structured EventBus envelope. The frame payload lives in
`attributes` and includes `capture_id`, `direction`, `leg`, `transport`, safe
HTTP metadata, MCP method/id metadata, and a JSON body with `size_bytes` and
`redacted_paths`. Current frames cover `tools/call` traffic at these gateway
boundaries:

| Leg | Meaning |
|-----|---------|
| `client_to_gateway` | MCP `/mcp` or REST `/v1/call` request entering the gateway |
| `gateway_to_client` | Gateway response leaving through MCP or REST |
| `gateway_to_adapter` | Gateway forwarding a backend `POST /v1/call` |
| `adapter_to_gateway` | Backend response or transport error observed by the gateway |

Traffic capture may include scene paths, user prompts, and tool arguments. If
`DCC_MCP_PROD_PROFILE=1`, the gateway refuses to enable capture unless
`DCC_MCP_FORCE_TRAFFIC_CAPTURE=1` is also set.

For replay/diff-oriented debugging, use the YAML config path. Redactions run
before any sink writes:

```yaml
enabled: true
sinks:
  - kind: sqlite
    path: ./captures/run-${TIMESTAMP}.db
  - kind: jsonl
    path: ./captures/run-${TIMESTAMP}.jsonl
  - kind: admin_live
    ring_buffer: 500
filters:
  include:
    - mcp.method: tools/call
  exclude:
    - http.url: "*/v1/readyz"
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
  - body.data.params.arguments.scene_path: "[SCRUBBED:path]"
```

Start the gateway with `DCC_MCP_TRAFFIC_CONFIG=./traffic_capture.yaml`. Relative
sink paths resolve from the config file's directory, and `${TIMESTAMP}` expands
once when the sink opens.
The optional `admin_live` sink keeps a bounded in-memory ring for
`/admin/api/traffic`, `/v1/debug/traffic`, and their JSONL export routes.

## Event Webhooks

RFC 0002 P3 adds optional webhook delivery in the standalone
`dcc-mcp-server`. Set `DCC_MCP_WEBHOOKS_CONFIG` to a YAML file, or edit the
Admin UI Integrations panel and let it write `~/dcc-mcp/etc/webhooks.yaml`
(`DCC_MCP_ETC_DIR` overrides the directory). The explicit environment variable
wins when both are present. The server subscribes to the shared
dispatcher/catalog `EventBus` before skill discovery, so startup `skill.*` and
runtime `tool.*` events can be forwarded without wrapping handlers.

:::: v-pre

```yaml
queue_capacity: 1024
webhooks:
  - name: studio-metrics
    url: https://example.invalid/dcc-mcp/events
    events: ["tool.completed", "tool.failed"]
    headers:
      Authorization: "Bearer ${DCC_MCP_WEBHOOK_TOKEN}"
    delivery:
      attempts: 3
      timeout_ms: 2000
      backoff_ms: [200, 1000, 5000]
    filters:
      - attributes.skill_name: "maya-*"
    payload_template: |
      {"text":"{{source.dcc_type}} called {{attributes.tool_slug}}"}
```

::::

By default, each request body is the structured event envelope. When
`payload_template` is present, `&#123;&#123;path.to.field&#125;&#125;` placeholders are resolved
against the envelope and sent as the JSON request body. Filters are ORed across
rules and ANDed within a rule; string values support `*` wildcards. The
delivery worker uses a bounded queue and drops new events when full rather than
blocking tool execution. If all retry attempts fail, the server emits
`webhook.delivery_failed` with the original event id/name and final error.

Enterprise WeChat group robots can be configured as a webhook kind. The runtime
wraps the rendered content in the robot markdown payload:

```yaml
webhooks:
  - name: wecom-alerts
    kind: wecom
    url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=${WECOM_ROBOT_KEY}
    events: ["tool.failed", "gateway.instance.*"]
    message_template: |
      DCC-MCP $event
      DCC: $dcc-type
      Tool: $tool-slug
      URL: $url
```

`message_template` supports both <code v-pre>`{{source.dcc_type}}`</code> style envelope paths and
operator-friendly dollar variables such as `$event`, `$dcc-type`,
`$instance-id`, `$tool-slug`, `$skill-name`, and `$url`. Set
`DCC_MCP_WECOM_WEBHOOK_URL` to enable the same integration without a YAML file;
optionally set `DCC_MCP_WECOM_EVENTS` as a comma or newline separated list and
`DCC_MCP_WECOM_TEMPLATE` for the message body. When configured from the Admin UI,
WeCom is saved as a `kind: wecom` webhook named `wecom-message-push` in the same
local `webhooks.yaml` file. Saving the WeCom shortcut preserves existing
non-WeCom webhook entries and replaces only an existing `kind: wecom` or
`name: wecom-message-push` entry.

## Wildcard Subscriptions

Use dotted event names and subscribe to either an exact name, a prefix wildcard,
or all events:

```python
bus.subscribe("tool.completed", on_completed)
bus.subscribe("skill.*", on_any_skill_event)
bus.subscribe("*", on_any_event)
```

## Dispatcher And Catalog Buses

`ToolDispatcher.event_bus()` returns the dispatcher's bus. A `SkillCatalog`
created with `SkillCatalog.new_with_dispatcher(...)` shares the dispatcher's bus
so subscribers can observe both `skill.*` and `tool.*` events from one place.

```python
from dcc_mcp_core import ToolDispatcher, ToolRegistry

registry = ToolRegistry()
dispatcher = ToolDispatcher(registry)
dispatcher.event_bus().subscribe("tool.*", record_metric)
```

## Failure Isolation

Subscriber exceptions are logged and do not stop later subscribers. Callbacks
are collected before invocation so a callback can safely subscribe or
unsubscribe without deadlocking the bus. Before hooks are intentionally stricter:
an exception or unsupported truthy return value becomes a veto so policy hooks
fail closed.
