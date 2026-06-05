# dcc-mcp-core 0.18 Adapter Checklist

Use this checklist before publishing or certifying a DCC adapter against
`dcc-mcp-core` 0.18.x.

## Release classification

`0.18.0` is a minor release because adapter-visible gateway and runtime
behaviour changed in ways that are larger than a patch:

- the daemon-backed gateway guardian is now the default path for Python
  adapters and server modes that auto-start a gateway;
- gateway request metadata is forwarded only through the bounded allowlist,
  which changes how clients should pass `_meta` through the gateway;
- Sentry error monitoring can be enabled in `dcc-mcp-server` deployments and
  should be configured deliberately by operators;
- recent gateway REST and MCP surface changes remain the compatibility floor
  for adapters moving onto this release train.

Adapter repositories should set their minimum core dependency to
`dcc-mcp-core>=0.18.0` when they adopt this checklist.

Release notes should call this out as: minor release for daemon-backed gateway
startup, bounded gateway request metadata, Sentry server monitoring, and the
adapter dependency floor of `dcc-mcp-core>=0.18.0`.

## Required adapter changes

| Area | What changed | Adapter action |
|---|---|---|
| Gateway daemon guardian | `dcc-mcp-server` and Python `DccServerBase` startup paths use the daemon-backed guardian by default. | Remove local first-wins gateway election workarounds, stop spawning a competing gateway process, and let core own daemon launch/recovery unless the adapter has an explicit legacy-mode requirement. |
| Request metadata | Gateway search/describe/call only forwards bounded `_meta` keys. Unknown client metadata is dropped, and server-derived agent context cannot be spoofed by clients. | Put backend tool inputs in `arguments`, keep trace/correlation data under allowed `_meta` keys, and do not depend on arbitrary `_meta` echoing across the gateway boundary. |
| Sentry monitoring | `dcc-mcp-server` can initialize Sentry from environment configuration. | Decide whether the adapter distribution should document or set Sentry env vars. Never bake a DSN into adapter code or skill packages. |
| Gateway REST output | REST discovery/call endpoints default to compact TOON unless the request asks for JSON. | Update HTTP clients that parse JSON by sending `Accept: application/json` or `response_format: "json"`. |
| Gateway MCP surface | The gateway exposes bounded discovery/invocation tools instead of fanning out backend tools through `tools/list`. | Use `search` -> `describe` -> `call`, or REST `/v1/search` -> `/v1/describe` -> `/v1/call`. Do not expect backend tools to appear directly in gateway `tools/list`. |

## Migration snippets

### Pin the adapter dependency floor

```toml
[project]
dependencies = [
    "dcc-mcp-core>=0.18.0",
]
```

If the adapter ships a companion server wheel, keep it on the same release
train:

```toml
[project]
dependencies = [
    "dcc-mcp-core>=0.18.0",
    "dcc-mcp-server>=0.18.0",
]
```

### Use core-owned gateway startup

Prefer the standard `DccServerBase` startup path and let core manage the
gateway daemon:

```python
from pathlib import Path

from dcc_mcp_core import DccServerBase, DccServerOptions


opts = DccServerOptions.from_env(
    "maya",
    Path(__file__).parent / "skills",
    port=8765,
)
server = DccServerBase(options=opts)
server.register_builtin_actions()
handle = server.start()
```

Only keep legacy gateway election or custom process supervision when a host
integration cannot run the daemon-backed mode. Document that exception in the
adapter release notes.

### Pass bounded gateway metadata

Use wrapper fields only for gateway routing, and place backend tool inputs
inside `arguments`:

```json
{
  "tool_slug": "maya.ab12cd34.create_sphere",
  "arguments": {
    "radius": 2.0,
    "name": "preview_sphere"
  },
  "meta": {
    "progressToken": "job-42",
    "dcc": {
      "async": true
    }
  }
}
```

Do not pass backend arguments such as `radius`, `code`, or `file_path` at the
gateway wrapper top level.

### Keep REST clients JSON-compatible

```bash
curl -sS \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{"kind":"tool","query":"create sphere","dcc_type":"maya","limit":5}' \
  http://127.0.0.1:8765/v1/search
```

Clients that can consume TOON should keep the default compact response.

### Configure Sentry outside code

```bash
set DCC_MCP_SENTRY_DSN=https://public@example.ingest.sentry.io/project
set DCC_MCP_SENTRY_ENVIRONMENT=studio-smoke
set DCC_MCP_SENTRY_TRACES_SAMPLE_RATE=0.0
dcc-mcp-server serve --dcc maya --port 8765
```

Use environment or deployment configuration only. Adapter source, packaged
skills, and checked-in examples must not contain real DSNs.

## Release validation

Before marking an adapter compatible with `0.18.0`, run at least one of these
paths:

1. Launch the adapter with `dcc-mcp-core>=0.18.0` and confirm `/v1/readyz`
   reports the expected gateway/backend readiness.
2. Through the gateway, call `search`, `describe`, and `call` for one adapter
   tool. Use REST JSON mode if the adapter test harness parses JSON.
3. Verify one direct per-DCC MCP path still works: `search_tools`,
   `get_skill_info`, `load_skill`, then the selected backend tool.
4. If Sentry is enabled in the deployment, run a non-production DSN smoke and
   confirm adapter errors are reported without leaking secrets.

Record any host that cannot run a live DCC smoke in the adapter PR validation
notes, along with the fake-server or conformance test that covered the gap.
