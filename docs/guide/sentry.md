# Sentry Error Monitoring

The gateway and server binaries ship with built-in Sentry SDK integration for
real-time error monitoring. The SDK captures Rust panics, explicit error events,
and selected span breadcrumbs automatically.

## Activation

Set the `DCC_MCP_SENTRY_DSN` environment variable to your Sentry project DSN.
The SDK initialises at server startup and captures panics automatically.

```bash
DCC_MCP_SENTRY_DSN="https://<key>@o<org>.ingest.sentry.io/<project>" \
  dcc-mcp-server --app maya
```

## Configuration

| Variable                      | Default              | Description                                |
|-------------------------------|----------------------|--------------------------------------------|
| `DCC_MCP_SENTRY_DSN`          | (disabled)           | Sentry project DSN                         |
| `DCC_MCP_SENTRY_ENVIRONMENT`  | `production`         | Environment tag for source-map filtering   |
| `DCC_MCP_SENTRY_RELEASE`      | crate version        | Release identifier (commit correlation)    |
| `DCC_MCP_SENTRY_SAMPLE_RATE`  | `1.0`                | Error event sample rate (`0.0`–`1.0`)      |

When `DCC_MCP_SENTRY_DSN` is absent the SDK skips initialisation entirely,
so zero-config deployments pay no overhead.

## What Gets Captured

| Event type                 | Automatic? | Notes                            |
|----------------------------|------------|----------------------------------|
| Rust panics                | ✅ Yes     | Caught by the Sentry panic hook  |
| Explicit `sentry::capture_error` | Manual | Use in catch blocks              |
| Explicit `sentry::capture_message` | Manual | Use for business-logic alerts    |
| Span breadcrumbs           | ✅ Yes     | When OTLP tracing is also active |
| Webhook delivery failures  | ✅ Yes     | When webhooks are configured     |

## Python-Side Error Reporting

Python adapters can forward exceptions to Sentry through the Rust bridge.
Use the `dcc_mcp_core.telemetry` helpers:

```python
from dcc_mcp_core import capture_exception

try:
    # … skill execution …
    pass
except Exception as exc:
    capture_exception(exc)
```

## Disabling Sentry

The Sentry crate is compiled in by default. To exclude it from the binary:

```bash
# Build without default features, then opt in only what you need
cargo build --no-default-features -p dcc-mcp-server
```

The SDK skips initialisation when `DCC_MCP_SENTRY_DSN` is absent, so the
compiled-in crate adds negligible size unless a DSN is configured.

## E2E Tests

Real-ingest Sentry E2E tests are gated behind the `sentry_e2e` feature and
require a valid `DCC_MCP_SENTRY_DSN` environment variable:

```bash
cargo test --features sentry_e2e --test sentry_e2e
```

These tests send events to your configured Sentry project and verify the
ingest pipeline end-to-end. They are excluded from CI unless the
`sentry_e2e` flag is explicitly set.

## See Also

- [gateway.md](gateway.md) — gateway configuration including webhooks and
  observability
- [observability.md](observability.md) — metrics, OTLP tracing, Prometheus
- [production-deployment.md](production-deployment.md) — production checklist
