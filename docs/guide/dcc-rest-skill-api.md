# Per-DCC REST Skill API

> Issue refs: [#658](https://github.com/dcc-mcp/dcc-mcp-core/issues/658) ·
> [#660](https://github.com/dcc-mcp/dcc-mcp-core/issues/660) ·
> [#818](https://github.com/dcc-mcp/dcc-mcp-core/issues/818) ·
> umbrella [#657](https://github.com/dcc-mcp/dcc-mcp-core/issues/657)

The MCP gateway is **not** the only way to make a DCC's skills callable from
outside. Each `McpHttpServer` embedded in a DCC process can also expose its
discovered skills as a small, versioned REST surface mounted at `/v1/*`. The
gateway then **indexes and routes** to those per-DCC services rather than
republishing every backend action as a separate MCP tool.

## Why

| Problem with gateway-only exposure                      | How the per-DCC REST surface fixes it                         |
|---------------------------------------------------------|---------------------------------------------------------------|
| `tools/list` grows linearly with `instances × actions`  | REST stays bounded: discovery, describe/call, resources, prompts, jobs |
| MCP tool names must match `^[A-Za-z0-9_-]{1,64}$`       | REST slugs use `<dcc>.<skill>.<action>` on per-DCC servers     |
| Non-MCP agents need a separate adapter to call DCC code | They `POST /v1/call` with a JSON body                          |
| MCP resources/prompts need JSON-RPC clients             | They can use `GET /v1/resources*` and `GET /v1/prompts*`       |
| No structured error class for "skill not loaded"        | `ServiceErrorKind::SkillNotLoaded` (kebab-case)                |

## Routes

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/v1/healthz` | Liveness |
| GET | `/v1/readyz` | Runtime readiness bits (process/dcc/skill_catalog/dispatcher/host_execution_bridge/main_thread_executor) |
| GET | `/v1/openapi.json` | utoipa-generated OpenAPI 3.x contract |
| GET | `/docs` | Optional Scalar UI for the OpenAPI contract (`DCC_MCP_DOCS_UI=0` disables it) |
| GET | `/v1/skills` | Loaded skills/actions |
| POST | `/v1/search` | Compact keyword/tag/dcc/scope search |
| POST | `/v1/describe` | Schema + annotations for one slug |
| GET | `/v1/tools/{slug}` | Alias for describe |
| POST | `/v1/call` | Invoke one tool by slug; accepts `params` and `arguments` |
| GET | `/v1/context` | Current DCC scene/document snapshot |
| GET | `/v1/resources` | MCP-style resource list |
| GET | `/v1/resources/{uri}` | Read one percent-encoded resource URI |
| GET | `/v1/resources/{uri}/events` | SSE stream for one resource's updates |
| GET | `/v1/prompts` | MCP-style prompt template list |
| GET | `/v1/prompts/{name}` | Render one prompt; pass JSON object args as `?args=...` |
| GET | `/v1/jobs/{id}/events` | SSE stream for one async job |
| DELETE | `/v1/jobs/{id}` | Cancel one async job |

## SOLID layering

```text
SkillRestRouter   ← axum thin adapter
       │
SkillRestService  ← pure logic, no axum
   │  │  │  │  │  │  │
   ▼  ▼  ▼  ▼  ▼  ▼  ▼
SkillCatalogSource  ToolInvoker  ResourceProvider  PromptProvider  JobProvider  AuthGate  AuditSink
   (trait)          (trait)      (trait)           (trait)         (trait)      (trait)  (trait)
```

Each collaborator is a **trait** so adapters (Maya/Blender/Houdini) can swap
in their own implementation without touching the router. Defaults wire to:

- `SkillCatalog` (`dcc-mcp-skills`) for the catalog source,
- `ToolDispatcher` (`dcc-mcp-actions`) for invocation,
- empty `ResourceProvider` / `PromptProvider` defaults that adapters can replace,
- `JobProvider` for job event streams and cancellation,
- `AllowLocalhostGate` for auth (loopback-only),
- `NoopAuditSink` for audit.

## Token efficiency

`/v1/search` hits intentionally **omit** `input_schema`. A regression test
asserts each serialised `SkillListEntry` stays under
`SEARCH_HIT_BUDGET_BYTES` (currently 512 bytes), so an agent can page
hundreds of capabilities per turn without blowing its context budget. Schema
is fetched on demand by `POST /v1/describe` with `include_schema: true`
(default).

## Enterprise controls (#660)

- **Versioned paths** — `/v1/*` is the stable contract.
- **Structured errors** — single envelope `{kind, message, hint, request_id, candidates?}`,
  `kind` is kebab-case (`unknown-slug`, `ambiguous`, `skill-not-loaded`,
  `invalid-params`, `unauthorized`, `bad-request`, `affinity-violation`,
  `not-ready`, `host-busy`, `backend-error`, `internal`).
- **Auth gate** — pluggable `AuthGate`. Default `AllowLocalhostGate`
  rejects non-loopback peers. Enable remote calls by installing
  `BearerTokenGate::new(vec![token])` and binding the listener to a
  non-loopback interface.
- **Audit sink** — every call emits one `AuditEvent`
  (`{request_id, at, slug, route, subject, outcome, duration_ms}`).
- **Readiness bits** — `process / dcc / skill_catalog / dispatcher` gate
  normal routing; `host_execution_bridge / main_thread_executor` are exposed
  separately so main-thread smoke tests can require them before calling DCC
  tools. `/v1/call` returns `503 not-ready` until the base routing bits are
  green.
- **OpenAPI** — generated by `utoipa` from the `ToSchema` derives on the
  request/response types — no hand-maintained JSON.

## Wiring example

```rust
use std::sync::Arc;
use axum::Router;
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_skill_rest::{
    AllowLocalhostGate, BearerTokenGate, NoopAuditSink, SkillRestConfig,
    SkillRestService, StaticReadiness, build_skill_rest_router,
};
use dcc_mcp_skills::SkillCatalog;

fn build_dcc_app(
    registry: Arc<ToolRegistry>,
    dispatcher: Arc<ToolDispatcher>,
) -> Router {
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));
    let service = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher);
    let cfg = SkillRestConfig::new(service)
        .with_auth(Arc::new(AllowLocalhostGate::new()))
        .with_audit(Arc::new(NoopAuditSink))
        .with_readiness(Arc::new(StaticReadiness::fully_ready()));
    build_skill_rest_router(cfg)
}
```

## Calling pattern

```bash
# 1. Search compact hits.
curl -s localhost:8765/v1/search -d '{"query":"sphere"}' -H 'content-type: application/json'

# 2. Fetch the schema for one slug.
curl -s localhost:8765/v1/describe \
  -d '{"tool_slug":"maya.spheres.create_sphere","include_schema":true}' \
  -H 'content-type: application/json'

# 3. Invoke. `arguments` is accepted as an alias for `params`.
curl -s localhost:8765/v1/call \
  -d '{"tool_slug":"maya.spheres.create_sphere","arguments":{"radius":1.5}}' \
  -H 'content-type: application/json'

# 4. Read MCP resources / render prompts without JSON-RPC.
curl -s localhost:8765/v1/resources
curl -s 'localhost:8765/v1/resources/scene%3A%2F%2Fcurrent'
curl -s 'localhost:8765/v1/prompts/create_plan?args=%7B%22task%22%3A%22model%20a%20prop%22%7D'
```
