# Gateway Middleware Chain

The gateway supports a pluggable `BeforeCall` / `AfterCall` middleware chain applied to every `tools/call` dispatch (issue #770).

## Quick Start

```rust
use dcc_mcp_gateway::gateway::middleware::{
    AuditMiddleware, MiddlewareChain, QuotaMiddleware, RedactionMiddleware,
};
use std::sync::Arc;

let config = GatewayConfig {
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default()))
        .with_before(Arc::new(QuotaMiddleware::new(100)))  // 100 calls/min
        .with_before(Arc::new(RedactionMiddleware::new(["api_key", "token"]))),
    ..GatewayConfig::default()
};
```

## Built-in Middleware

| Middleware | Purpose | On failure |
|-----------|---------|-----------|
| `AuditMiddleware` | Emits a `tracing::info!` log per `tools/call` with method, tool, DCC type, session ID, duration, and result | Never blocks |
| `QuotaMiddleware::new(N)` | Limits to N calls/minute globally; returns `MiddlewareError::QuotaExceeded` when over | Aborts with 429-equivalent error |
| `RedactionMiddleware::new(fields)` | Replaces matching arg field values with `"[REDACTED]"` before logging or forwarding | Never fails |

## Custom Middleware

Implement `BeforeCallMiddleware` or `AfterCallMiddleware`:

```rust
use dcc_mcp_gateway::gateway::middleware::{
    AfterCallMiddleware, BeforeCallMiddleware, CallContext, CallResult, MiddlewareError,
};

pub struct MyMiddleware;

#[async_trait::async_trait]
impl BeforeCallMiddleware for MyMiddleware {
    async fn before_call(&self, ctx: &mut CallContext) -> Result<(), MiddlewareError> {
        // Inspect or mutate ctx before the call is dispatched
        tracing::info!(tool = ?ctx.tool_slug, dcc = ?ctx.dcc_type, "custom before-call");
        Ok(())
    }
}

#[async_trait::async_trait]
impl AfterCallMiddleware for MyMiddleware {
    async fn after_call(
        &self,
        ctx: &mut CallContext,
        result: &mut CallResult,
    ) -> Result<(), MiddlewareError> {
        tracing::info!(success = result.success, "custom after-call");
        Ok(())
    }
}
```

Register it:

```rust
MiddlewareChain::new()
    .with_before(Arc::new(MyMiddleware))
    .with_after(Arc::new(MyMiddleware))
```

## `CallContext` Fields

| Field | Type | Description |
|-------|------|-------------|
| `method` | `String` | MCP method (`tools/call`, `tools/list`, …) |
| `tool_slug` | `Option<String>` | Tool name |
| `dcc_type` | `Option<String>` | DCC type (`maya`, `blender`, …) |
| `session_id` | `Option<String>` | MCP session ID |
| `request_id` | `String` | Unique per-request ID (matches audit log `request_id`) |
| `args` | `serde_json::Value` | Tool arguments (mutable; `RedactionMiddleware` modifies this) |
| `metadata` | `HashMap<String, String>` | Pass-through bag for inter-middleware communication |

## Execution Order

```
Request → BeforeCall[0] → BeforeCall[1] → ... → dispatch → AfterCall[0] → AfterCall[1] → Response
```

If any `before_call` returns `Err`, the call is aborted and subsequent middlewares are skipped.

## Integration with Admin UI

The gateway's Admin UI uses `AuditMiddleware` to populate `/admin/api/calls` and to promote completed calls into `/admin/api/traces`. The shipped `dcc-mcp-server` path wires an `AdminAuditSink` automatically when admin is enabled. If you construct `dcc-mcp-gateway` directly, add an audit middleware/sink to your `GatewayConfig` before starting the router:

```rust
let audit = Arc::new(AuditMiddleware::default());

GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(audit.clone())
        .with_after(audit),
    ..GatewayConfig::default()
}
```

Set `DCC_MCP_GATEWAY_AUDIT_DIR` when operators need bounded `audit.jsonl` and `traces.jsonl` persistence across gateway restarts.

## See also

- [admin-ui.md](admin-ui.md) — dashboard that consumes the audit feed
- [gateway.md](gateway.md) — full gateway configuration reference
- [observability.md](observability.md) — OTLP tracing (middleware can enrich span attrs via `CallContext`)
