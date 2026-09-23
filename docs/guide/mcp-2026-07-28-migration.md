# Migrating to MCP 2026-07-28

MCP `2026-07-28` is the largest protocol revision since MCP shipped. It removes
the session handshake, moves client context into a per-request `_meta` object,
and replaces the session-based lifecycle with a stateless one. dcc-mcp-core
serves both protocols side by side and routes each request by its declared
protocol version, so upgrading is a per-client and per-deployment decision
rather than a flag day.

This guide is for client authors who call a dcc-mcp-core endpoint and for
adapter maintainers who embed or ship it. It lists the breaking changes, the
steps for each side, an adapter checklist, and — because the two protocols
coexist for a defined window — how routing and the staged timeline work today.

**Read this together with:**

- [ADR-010 — MCP 2026-07-28 dual-protocol migration strategy](../adr/010-mcp-2026-07-28-dual-protocol-migration.md) — phases, sunset date, rationale
- [ADR-033 — explicit MCP HTTP protocol routing](../adr/033-mcp-http-protocol-router.md) — the `MCP-Protocol-Version` routing rule
- [ADR-034 — final-revision modern response projection](../adr/034-modern-response-contract.md) — response envelope and discovery shape
- [ADR-035 — schema-driven parameter headers](../adr/035-schema-driven-parameter-headers.md) — `Mcp-Name` / `Mcp-Param-*` mirroring
- [Modern request boundary](modern-request-boundary.md) — envelope and header validation, error codes
- [Stateless provider parity](stateless-provider-parity.md) — resources and prompts on the modern route
- [Migration: embedded → daemon](migration/from-embedded-to-daemon.md) — unrelated runtime-topology migration; read it if you are also retiring an embedded gateway

## Status at a glance

| Area | State in current releases |
|------|---------------------------|
| Stateless request path | Shipped, **opt-in** (compiled only with the `mcp-2026-07-28` Cargo feature) |
| Legacy session path | Shipped, unchanged, **still the default** for requests without an explicit modern claim |
| `server/discover`, `tools/list`, `tools/call` | Shipped on the modern route |
| Resources / prompts providers | Shipped on the modern route, gated by feature flags and registered providers |
| Skills extension (`skills/list`, `skills/get`, `resources/directory/read`) | Shipped on the modern route |
| Schema-driven `Mcp-Param-*` headers | Shipped on the modern route |
| Multi-round-trip input (replaces SSE push) | **Not implemented** — see [SSE streaming is replaced by multi-round-trip input](#sse-streaming-is-replaced-by-multi-round-trip-input) |
| Tasks wire surface | **Not implemented and not advertised** — see [Tasks are unavailable on the modern route](#tasks-are-unavailable-on-the-modern-route) |
| OAuth 2.1 / OIDC authorization | **Not implemented** — see [Authorization hardening is deferred to Phase 2](#authorization-hardening-is-deferred-to-phase-2) |
| Session / SSE removal | Planned for Phase 3; no session or SSE code has been marked `#[deprecated]` yet |

The default protocol version is still `2025-06-18` and `ProtocolMode::default()`
is still `Session`. Nothing changes for a client that does not opt in.

## Breaking changes

### `initialize` / `initialized` handshake is removed

The modern route has no lifecycle handshake. `server/discover` replaces
`initialize` and answers discovery in a single stateless round trip.

- Send `server/discover` instead of `initialize`; there is no `initialized`
  notification to send afterwards.
- On the modern route, `initialize` is not a supported method: a request that
  carries a valid modern envelope claim reaches the modern method registry and
  gets `METHOD_NOT_FOUND` (HTTP 404 / JSON-RPC `-32601`). A legacy `initialize`
  without a modern claim keeps working on the legacy route.
- Discovery results carry `supportedVersions` (a list), not `protocolVersion`.
  Read the list rather than comparing a single version string.
- Negotiation through the `initialize` body cannot select a lifecycle. A client
  that asks for `2026-07-28` inside `initialize` gets `2025-06-18` back; only
  the modern envelope and the matching headers reach the stateless handler.

### `Mcp-Session-Id` and protocol-level sessions are removed

Modern requests are fully self-contained: there is no session to create,
resume, or expire, and no server-side session state to pin a client to.

- Stop persisting or replaying `Mcp-Session-Id` on modern requests. Its
  presence never upgrades a request to the stateless path.
- Do not rely on session affinity or sticky load balancing for modern traffic;
  any instance can serve any request.
- Server-pushed state (progress, notifications, subscriptions) is not part of
  the shipped modern surface. Poll or re-list instead.

### Every request carries its own context in `_meta`

Client identity, capabilities, and the protocol version travel in
`params._meta` on every request. Required keys:

| Key | Required | Notes |
|-----|----------|-------|
| `io.modelcontextprotocol/protocolVersion` | Yes | Must be a string; must equal the `MCP-Protocol-Version` header |
| `io.modelcontextprotocol/clientCapabilities` | Yes | Must be an object; an empty object is valid |
| `io.modelcontextprotocol/clientInfo` | No | `{ "name": …, "version": … }`; optional, but a present malformed value is rejected |
| `io.modelcontextprotocol/logLevel` | No | Validated when present |
| `progressToken` | No | Preserved through dispatch |

Standard headers on modern `POST` requests:

| Header | Required | Rule |
|--------|----------|------|
| `MCP-Protocol-Version` | Yes | Must match the `_meta` version claim |
| `Mcp-Method` | Yes | Must match the JSON-RPC `method` |
| `Mcp-Name` | Conditional | Required for `tools/call` and `prompts/get` (mirrors `params.name`) and `resources/read` (mirrors `params.uri`); must match the body field |
| `Mcp-Param-*` | Conditional | Schema-driven parameter mirroring and validation, per [ADR-035](../adr/035-schema-driven-parameter-headers.md) |

A header-only opt-in is not enough: the ingress classifier is body-primary and
validates the envelope claim and the headers together, so a malformed claim
cannot silently become legacy traffic.

### SSE streaming is replaced by multi-round-trip input

The revision replaces server-push streaming with an `InputRequiredResult`
multi-round-trip exchange. **dcc-mcp-core has not implemented that exchange
yet.** Practically:

- The legacy SSE subscribers remain, and they remain legacy-only. Do not expect
  server push on a modern request.
- Long-running DCC operations must be modelled as an ordinary request/response
  call, or driven through the existing job tooling, until multi-round-trip
  execution ships.
- Capability advertising is honest about this: no subscription, notification,
  or list-change stream is declared.

### Tasks are unavailable on the modern route

`tasks/list` does not exist in the final revision. Beyond that, the delivered
stateless service implements no task methods at all:

- Discovery does not advertise a `tasks` capability.
- `tasks/get` and `tasks/cancel` return `METHOD_NOT_FOUND` (HTTP 404 /
  `-32601`).
- No `tasks/create` handler exists.

[ADR-010](../adr/010-mcp-2026-07-28-dual-protocol-migration.md) describes the
Tasks extension as a first-class feature; **the shipped code does not match
that description.** Treat Tasks as unavailable and follow the reconciliation in
a later release before designing against it. A `TasksCapability` value that
appears in Rust unit-test fixtures is test data, not server behaviour.

### `roots`, `sampling`, and `logging` are deprecated

These capabilities enter a 12-month deprecation window per the MCP deprecation
policy.

- Nothing in the shipped modern surface requires sampling, elicitation, or
  roots, and no method declares `requiredCapabilities`.
- `logging/setLevel` remains a legacy session-lifecycle method.
- New integrations should not be built on these capabilities; existing ones
  keep working on the legacy route until Phase 3.

### Authorization hardening is deferred to Phase 2

The revision tightens authorization to OAuth 2.1 / OIDC. dcc-mcp-core defers
this to Phase 2; today:

- Authorization is request-scoped on the modern route, evaluated per request
  rather than per session.
- Existing gateway token verification is unchanged.
- Deployments in front of a public endpoint should keep an external
  authenticating proxy until native OAuth 2.1 / OIDC support lands.

### Standard headers, cache hints, and trace context

- Cache fields (`ttlMs`, `cacheScope`) are emitted on discovery, list methods,
  and resource reads with conservative defaults (`ttlMs: 0`,
  `cacheScope: private`). Valid handler-authored hints win; invalid hints fall
  back conservatively. No positive TTL and no shared-authorization cache is
  enabled by default.
- Responses carry `resultType: complete` and the server identity under
  `_meta["io.modelcontextprotocol/serverInfo"]`.
- Trace and vendor metadata placed in `_meta` (for example a W3C `traceparent`
  value) is preserved through business dispatch.

## Dual-protocol coexistence

### How a request is routed

Routing is decided by `dcc-mcp-jsonrpc::classify_protocol_request`, shared so
the embedded HTTP server, gateway, and CLI cannot drift.

| Incoming request | Route |
|------------------|-------|
| `MCP-Protocol-Version: 2026-07-28` **and** a matching `_meta` version claim | Stateless (modern) |
| `MCP-Protocol-Version: 2025-06-18` or `2025-03-26` | Session (legacy) |
| Header absent, no modern `_meta` claim | Session (legacy) — Phase 1 default |
| `Mcp-Session-Id` present, version header absent | Session (legacy); the session header never upgrades a request |
| Unknown version | Session (legacy) in Phase 1–2; an explicit error in Phase 3 |
| All-legacy batch body | Session (legacy), even with a modern version header |

Boundary failures on the modern route:

| Failure | HTTP | JSON-RPC |
|---------|------|----------|
| Malformed JSON or invalid modern JSON-RPC shape | 400 | `-32700` / `-32600` |
| Missing or malformed request envelope | 400 | `-32602` |
| Required standard header absent, malformed, or disagreeing with the body | 400 | `-32020` |
| Unsupported modern version | 400 | `-32022` (with `data.supported` and `data.requested`) |
| Unregistered modern method | 404 | `-32601` |
| Body above `queue.max_request_body_bytes` (default 4 MiB) | 413 | — |

Ordinary handler failures stay HTTP 200 with a JSON-RPC error, and tool
execution failures stay HTTP 200 `tools/call` results with `isError`.

### Feature flag state

The `mcp-2026-07-28` Cargo feature is **off by default** in
`crates/dcc-mcp-jsonrpc/Cargo.toml`. Without it, `SUPPORTED_PROTOCOL_VERSIONS`
contains only the legacy versions and every request routes to the session
handler.

Published Python wheels (abi3 and native CPython 3.7) enable the feature, so
`pip install dcc-mcp-core` gives you an endpoint that answers modern requests.
The py37-lite wheel is pure Python and delegates HTTP serving to the separately
distributed `dcc-mcp-server` binary — validate it independently. The standalone
`dcc-mcp-server` binary is built with default Cargo features, which do **not**
forward `mcp-2026-07-28`, so a binary built with plain
`cargo build -p dcc-mcp-server` serves the legacy route only.

### Phase timeline

Targets from [ADR-010](../adr/010-mcp-2026-07-28-dual-protocol-migration.md);
dates are release targets, not guarantees.

| Phase | Release target | Date target | Default route | What changes |
|-------|----------------|-------------|---------------|--------------|
| 1 | 0.19.0 | 2026-07-15 | Session | Compat layer: both protocols served, modern path opt-in |
| 2 | 0.21.0 | 2026-09-30 | Stateless | Default version flips to `2026-07-28`; feature on by default; session and SSE code marked deprecated |
| 3 | 0.23.0 | 2026-12-15 | Stateless only | Session, SSE, and `initialize` removed; feature removed |

Sunset of the legacy protocol: **2026-12-15** (Phase 3), inside the
2026-12-31 deprecation-window deadline.

## Upgrading a client

1. **Detect, do not assume.** Send one `server/discover` with
   `MCP-Protocol-Version: 2026-07-28` and the matching `_meta` claim. If the
   server returns a successful discovery result, use the modern route. If it
   returns `-32601`, send legacy `initialize` and use the legacy route. If it
   returns `-32022`, read `data.supported` and retry with the highest mutually
   supported version.
2. **Send the required headers and `_meta` on every request.** There is no
   handshake to inherit them from.
3. **Replace `initialize` with `server/discover`** and drop the `initialized`
   notification.
4. **Drop session handling** — no `Mcp-Session-Id`, no session resume, no
   affinity requirement.
5. **Re-list to observe change.** `tools/list` is paginated (32 entries per
   page) and `listChanged` is `false`; follow `nextCursor` and re-list after
   loading or unloading skills.
6. **Handle the boundary errors above** instead of retrying blindly, and keep
   bodies under the configured limit.

A modern `tools/call`:

```bash
curl -s "http://127.0.0.1:<port>/mcp" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: tools/call' \
  -H 'Mcp-Name: create_sphere' \
  -d '{
    "jsonrpc": "2.0",
    "id": "call-1",
    "method": "tools/call",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { "name": "my-client", "version": "1.0.0" }
      },
      "name": "create_sphere",
      "arguments": { "radius": 1.0 }
    }
  }'
```

Modern `tools/call` arguments must be a JSON object when present; `null`,
arrays, and JSON-encoded strings are rejected before any handler runs.

## Upgrading a server or embedded deployment

1. **Ship an artifact built with the feature.** Wheels already include it; a
   custom Rust build needs `--features mcp-2026-07-28` on `dcc-mcp-core` (or
   `dcc-mcp-http` / `dcc-mcp-http-py`).
2. **Mount `/mcp` through the shared router** so modern and legacy requests are
   classified once and consistently; do not write a second route that decides
   by header alone.
3. **Keep business handlers unchanged.** Tool registration, dispatch, catalog,
   and thread-affinity behaviour are identical on both routes; provider wiring
   for resources and prompts is reused, not duplicated.
4. **Do not advertise capabilities you do not serve.** Discovery derives
   resources and prompts from enabled features plus registered providers, and
   it declares no tasks, subscriptions, or change notifications.
5. **Size your ingress.** The configured body limit now covers `/mcp` and
   returns 413 above it.
6. **Plan for request-scoped authorization** and keep an authenticating proxy in
   front of public endpoints until OAuth 2.1 / OIDC lands.

## Adapter checklist

Applies to the first-party adapters below. Tool handlers and Python APIs do not
change; the work is transport, packaging, and verification.

| Adapter | Repository | Dispatcher pattern to verify after the bump |
|---------|------------|---------------------------------------------|
| Maya | [dcc-mcp-maya](https://github.com/dcc-mcp/dcc-mcp-maya) | Qt sidecar + `HostUiDispatcherBase` — call dispatch unchanged |
| Blender | [dcc-mcp-blender](https://github.com/dcc-mcp/dcc-mcp-blender) | In-process MCP + optional diagnostics sidecar |
| Houdini | [dcc-mcp-houdini](https://github.com/dcc-mcp/dcc-mcp-houdini) | Event-loop callback — no per-session state to maintain |
| Nuke | [dcc-mcp-nuke](https://github.com/dcc-mcp/dcc-mcp-nuke) | Host main-thread dispatcher |
| Unreal | [dcc-mcp-unreal](https://github.com/dcc-mcp/dcc-mcp-unreal) | Unreal Python bridge |
| Photoshop | [dcc-mcp-photoshop](https://github.com/dcc-mcp/dcc-mcp-photoshop) | WebSocket bridge |

### Packaging and artifacts

- [ ] Pin `dcc-mcp-core` to a release whose wheels enable `mcp-2026-07-28`.
- [ ] Confirm the artifact that serves `/mcp` in your adapter was built with the
      feature enabled (wheels: yes; plain `cargo build -p dcc-mcp-server`: no).
- [ ] Validate py37-lite and any `dcc-mcp-server`-delegated path separately.
- [ ] Record the verified core pin in the
      [adapter compatibility matrix](adapter-compatibility-matrix.md).

### Transport and runtime

- [ ] Remove any dependence on `initialize`, `initialized`, and
      `Mcp-Session-Id` in adapter-side clients and smoke scripts.
- [ ] Send `MCP-Protocol-Version` and `Mcp-Method` on every request, plus
      `Mcp-Name` for `tools/call` / `prompts/get` / `resources/read`.
- [ ] Populate the required `_meta` keys on every request.
- [ ] Use `server/discover` and read `supportedVersions`.
- [ ] Do not call `tasks/*`, `subscriptions/listen`, or expect SSE push on the
      modern route.
- [ ] Re-list to observe catalog changes; do not cache cursors indefinitely.
- [ ] Handle 400 / `-32020` and `-32022` explicitly; treat 404 / `-32601` as
      "method not on this route".

### Verification

- [ ] Run the modern Rust suites (below) in the adapter's CI matrix.
- [ ] Run the official-SDK interop smoke at least once per release.
- [ ] Smoke one real `tools/call` against a live DCC host on both routes.
- [ ] Confirm no adapter code path requires session affinity or sticky routing.

## Verifying your migration

```bash
# Modern request/response, request boundary, parameter headers, providers
cargo test -p dcc-mcp-http --no-default-features --features mcp-2026-07-28 \
  --test stateless_response_contract
cargo test -p dcc-mcp-http --no-default-features --features mcp-2026-07-28 \
  --test stateless_request_boundary
cargo test -p dcc-mcp-http --no-default-features --features mcp-2026-07-28 \
  --test stateless_param_headers
cargo test -p dcc-mcp-http --no-default-features --features mcp-2026-07-28 \
  --test stateless_providers
```

The opt-in official-SDK interop smoke exercises discovery, list, and call
through the published `@modelcontextprotocol/client` 2.0.0 in both auto and
pinned negotiation; see
[`tests/interop/mcp-2026/README.md`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/tests/interop/mcp-2026/README.md).

```bash
npm ci --prefix tests/interop/mcp-2026 --ignore-scripts
DCC_MCP_SDK_SMOKE=1 cargo test -p dcc-mcp-http --no-default-features \
  --features mcp-2026-07-28 --test stateless_response_contract -- --nocapture
```

These are bounded checks over discovery, list, call, provider parity, and the
request boundary. They are **not** a full conformance claim: subscriptions,
multi-round-trip execution, and full transport and authorization conformance
are separate work.

## Known gaps

- **Tasks** — advertised by neither discovery nor code, despite ADR-010
  describing it as first-class. Pending reconciliation.
- **Multi-round-trip input** — not implemented; no replacement for SSE push yet.
- **Subscriptions and notifications** — not implemented; re-list instead.
- **OAuth 2.1 / OIDC** — deferred to Phase 2.
- **Deprecation markers** — session and SSE code is not yet annotated, because
  Phase 2 has not started.

## References

- [ADR-010 — MCP 2026-07-28 dual-protocol migration strategy](../adr/010-mcp-2026-07-28-dual-protocol-migration.md)
- [ADR-033 — explicit MCP HTTP protocol routing](../adr/033-mcp-http-protocol-router.md)
- [ADR-034 — final-revision modern response projection](../adr/034-modern-response-contract.md)
- [ADR-035 — schema-driven parameter headers](../adr/035-schema-driven-parameter-headers.md)
- [Modern request boundary](modern-request-boundary.md)
- [Stateless provider parity](stateless-provider-parity.md)
- [Capabilities](capabilities.md)
- [Protocols](protocols.md) and [transport layer](transport.md)
- [Adapter compatibility matrix](adapter-compatibility-matrix.md)
- [MCP 2026-07-28 release candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
