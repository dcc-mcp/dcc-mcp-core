# ADR 030: Preserve Published Crate Identities Across Boundary Cleanup

## Status

Accepted

## Context

Several workspace names describe the layer that first introduced them rather
than every responsibility they now contain. In particular,
`dcc-mcp-http-server` owns transport-independent HTTP server runtime state,
while `dcc-mcp-gateway-search` owns the reusable search query, ranking, and
pagination contract used by the gateway, catalog, skill catalog, and REST
service.

Renaming either published crate would require a coordinated dependency and
import migration for no wire or behavioral benefit. The current package
descriptions and architecture guide already state their narrower ownership.

The tunnel crates were also reported as disconnected. That observation became
obsolete when the tunnel CLI binaries and relay-backed gateway discovery were
integrated. Their protocol, agent, and relay separation now follows the
dependency direction recorded in ADR 027.

The standalone `marketplace-ui` directory had a different problem: it was not
served, packaged, built in CI, or included in any release process. Its gateway
WebSocket protocol remains a supported backend integration point, but the
unshipped source tree was not a product boundary.

## Decision

- Keep the published `dcc-mcp-http-server` and
  `dcc-mcp-gateway-search` crate identities stable.
- Continue describing the former as runtime support and the latter as the
  canonical search/query/ranking engine.
- Keep the integrated tunnel protocol, agent, relay, and CLI boundaries.
- Remove the unbuilt `marketplace-ui` source tree. A future standalone
  marketplace client must live in a repository or package with an explicit
  owner, build, test, deployment, and release contract.
- Treat source layout cleanup as an internal refactor. Existing Python import
  paths remain compatibility aliases when implementations move.

## Consequences

Downstream Rust consumers avoid a breaking rename and existing telemetry target
names stay stable. The repository no longer carries a private web application
that cannot be produced from CI. Future UI work must define delivery ownership
before source is added.

Python modules can gain clearer canonical ownership without forcing adapters to
change imports in the same release.

## Alternatives Considered

Renaming both Rust crates was rejected because it creates ecosystem churn
without changing their contracts. Moving the tunnel crates to another
repository was rejected because they are now part of the server and gateway
runtime. Adding CI only for `marketplace-ui` was rejected because a green
build would still leave serving, deployment, and release ownership undefined.
