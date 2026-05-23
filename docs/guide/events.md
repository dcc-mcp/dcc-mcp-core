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

The core dispatcher and skill catalog emit these events when subscribers are
present:

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

## Gateway Traffic Capture

RFC 0003 P0 adds an opt-in `traffic.frame` stream for local debugging. It is
off by default. Enable the quick JSONL sink when starting a gateway:

```bash
DCC_MCP_TRAFFIC_CAPTURE=jsonl:./capture.jsonl dcc-mcp-server ...
```

Each JSONL row is the structured EventBus envelope. The frame payload lives in
`attributes` and includes `capture_id`, `direction`, `leg`, `transport`, safe
HTTP metadata, MCP method/id metadata, and a JSON body with `size_bytes`.
Current P0 frames cover `tools/call` traffic at these gateway boundaries:

| Leg | Meaning |
|-----|---------|
| `client_to_gateway` | MCP `/mcp` or REST `/v1/call` request entering the gateway |
| `gateway_to_client` | Gateway response leaving through MCP or REST |
| `gateway_to_adapter` | Gateway forwarding a backend `POST /v1/call` |
| `adapter_to_gateway` | Backend response or transport error observed by the gateway |

Traffic capture may include scene paths, user prompts, and tool arguments. If
`DCC_MCP_PROD_PROFILE=1`, the gateway refuses to enable capture unless
`DCC_MCP_FORCE_TRAFFIC_CAPTURE=1` is also set.

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
unsubscribe without deadlocking the bus.
