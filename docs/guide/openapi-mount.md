# OpenAPI → MCP Mount Helper

Expose any existing REST API as MCP tools in the gateway with a single config block (issue #773).

## Quick Start

```rust
gateway_builder.mount_openapi(
    OpenApiMount::from_url("https://api.example.com/openapi.json")
        .base_url("https://api.example.com")
        .auth(AuthConfig::bearer("$MY_API_TOKEN"))
        .tool_prefix("example"),
)
```

This generates one MCP tool per OpenAPI operation. Tool names follow `{prefix}__{operationId}` or `{prefix}__{method}_{path_sanitized}` when `operationId` is absent.

## How It Works

1. **Spec parsing** — `OpenApiMount` fetches and parses the OpenAPI 3.x JSON spec
2. **Tool generation** — one MCP tool per operation; `inputSchema` is built from `requestBody` + `parameters`
3. **HTTP forwarding** — on `tools/call`, path/query/body params are mapped and the request is forwarded to the backend with auth headers injected

## `OpenApiMount` Builder

```rust
OpenApiMount::from_url("https://api.example.com/openapi.json")
    // Required: backend base URL
    .base_url("https://api.example.com")
    // Optional: auth forwarding
    .auth(AuthConfig::bearer("$MY_API_TOKEN"))
    // Optional: prefix for all generated tool names (avoids collisions)
    .tool_prefix("example")
```

## Auth Configuration

| Method | Description |
|--------|-------------|
| `AuthConfig::bearer("$TOKEN")` | `Authorization: Bearer <token>` — `$ENV_VAR` references resolved at call time |
| `AuthConfig::api_key("X-Api-Key", "$KEY")` | Custom header injection |
| `AuthConfig::basic("$USER", "$PASS")` | HTTP Basic auth (base64-encoded) |

Environment variable references (`$VAR_NAME`) are resolved at call time, not at mount time. If the variable is unset, the header is omitted.

## Generated Tool Schema

For a `POST /pets` operation with a JSON body:

```json
{
  "name": "example__createPet",
  "description": "Create a new pet",
  "inputSchema": {
    "type": "object",
    "properties": {
      "name": { "type": "string" },
      "species": { "type": "string" }
    },
    "required": ["name"]
  }
}
```

## Parameter Mapping

| OpenAPI location | Maps to |
|-----------------|---------|
| `path` params (`/pets/{petId}`) | Substituted in URL path |
| `query` params | Appended as `?key=value` |
| `requestBody` (JSON) | Serialized as JSON body |

## Error Handling

HTTP 4xx/5xx responses from the backend are mapped to `CallError::BackendError { status, body }`.

## Gateway Registration

```rust
impl GatewayBuilder {
    pub fn mount_openapi(mut self, mount: OpenApiMount) -> Self { ... }
}

// Multiple mounts
gateway_builder
    .mount_openapi(OpenApiMount::from_url("...").tool_prefix("api1"))
    .mount_openapi(OpenApiMount::from_url("...").tool_prefix("api2"))
```

## Limitations

- Only OpenAPI 3.x JSON specs are supported (YAML support via string conversion)
- `$ref` resolution is limited to single-level inline references
- File upload (`multipart/form-data`) is not yet supported
- WebSocket / streaming operations are not exposed as MCP tools

## See also

- [gateway.md](gateway.md) — full gateway configuration reference
- [middleware.md](middleware.md) — add auth forwarding policies via `BeforeCallMiddleware`
- [rest-api-surface.md](rest-api-surface.md) — per-DCC REST skill API
