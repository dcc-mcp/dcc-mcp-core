# REST cheatsheet — DCC-MCP gateway

Base URL: `$DCC_MCP_GATEWAY_URL` (default `http://127.0.0.1:9765`).

Aligned with [rest-api-surface.md](https://github.com/loonghao/dcc-mcp-core/blob/main/docs/guide/rest-api-surface.md).

## Discovery and health


| Method | Path               | Purpose                                         |
| ------ | ------------------ | ----------------------------------------------- |
| GET    | `/v1/healthz`      | Liveness                                        |
| GET    | `/v1/readyz`       | Readiness (503 while booting)                   |
| GET    | `/v1/instances`    | **Instance inventory** (`total`, `instances[]`) |
| GET    | `/v1/context`      | Aggregated context + `instances` + counts       |
| GET    | `/v1/openapi.json` | OpenAPI document                                |


## Capability workflow


| Method | Path               | Purpose                                           |
| ------ | ------------------ | ------------------------------------------------- |
| POST   | `/v1/search`       | Find tools; returns `hits[].tool_slug`            |
| POST   | `/v1/describe`     | Full schema for one slug                          |
| GET    | `/v1/tools/{slug}` | Describe alias (URL-encoded slug)                 |
| POST   | `/v1/call`         | Invoke one tool                                   |
| POST   | `/v1/call_batch`   | Up to 25 ordered calls (`stop_on_error` optional) |


## Path-style call (optional)


| Method | Path                                                     | Body                               |
| ------ | -------------------------------------------------------- | ---------------------------------- |
| POST   | `/v1/dcc/{dcc}/instances/{id}/call`                      | `{"backend_tool","arguments",...}` |
| GET    | `/v1/dcc/{dcc}/instances/{id}/describe?backend_tool=...` | —                                  |


## Example: inventory

```bash
GATEWAY="${DCC_MCP_GATEWAY_URL:-http://127.0.0.1:9765}"
curl -s "$GATEWAY/v1/instances"
```

## Example: search

```bash
curl -s -X POST "$GATEWAY/v1/search" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{"query":"sphere","dcc_type":"maya","limit":10}'
```

## Example: describe

```bash
curl -s -X POST "$GATEWAY/v1/describe" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{"tool_slug":"maya.a1b2c3d4.maya_primitives__create_sphere","include_schema":true}'
```

## Example: call

```bash
curl -s -X POST "$GATEWAY/v1/call" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{
    "tool_slug": "maya.a1b2c3d4.maya_primitives__create_sphere",
    "arguments": { "radius": 2.0 }
  }'
```

## Example: batch

```bash
curl -s -X POST "$GATEWAY/v1/call_batch" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{
    "calls": [
      { "tool_slug": "maya.a1b2c3d4.tool_a", "arguments": {} },
      { "tool_slug": "maya.a1b2c3d4.tool_b", "arguments": {} }
    ],
    "stop_on_error": true
  }'
```

## Slug rules

- Gateway: `<dcc_type>.<id8>.<backend_tool>` from **search hits only**.
- Do not put `code`, `radius`, etc. at the top level of `/v1/call` — only inside `arguments`.

## Common errors


| kind               | HTTP | Action                                     |
| ------------------ | ---- | ------------------------------------------ |
| `unknown-slug`     | 404  | Re-search; instance may have restarted     |
| `instance-offline` | 503  | Re-run `/v1/instances`                     |
| `skill-not-loaded` | 409  | Load skill on backend or pick another tool |
| `invalid-params`   | 400  | Fix `arguments` per describe schema        |
