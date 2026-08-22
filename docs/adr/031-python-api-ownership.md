# ADR 031: Python API Ownership and Compatibility Boundaries

- Status: Accepted
- Date: 2026-08-22
- Issue: #2193

## Context

The Python package accumulated one top-level module per capability, exposed
adapter contracts from the private `_server` package, and kept standard
registration behavior in both phase classes and `DccServerBase` methods. The
Rust `qtserver://` build also embedded a top-level Python file, making a build
path look like the public owner of host transport behavior.

Removing historical imports in one release would break existing adapters, so
the ownership correction needs an explicit compatibility boundary.

## Decision

New Python APIs must live in an ownership-oriented namespace:

| Namespace | Owner |
|-----------|-------|
| `dcc_mcp_core.server` | Server construction, dispatch, options, and adapter contracts |
| `dcc_mcp_core.runtime` | Native/lite runtime selection and fallback contracts |
| `dcc_mcp_core.deployment` | Import-light Rez deployment and sidecar lifecycle |
| `dcc_mcp_core.host` | Host transports, including the canonical Qt dispatcher source |
| `dcc_mcp_core.skill_index` | Skill indexing and retrieval |
| `dcc_mcp_core.experimental` | Explicitly non-stable compatibility APIs |

The package root remains a compatibility facade. Stable and experimental lazy
maps are separate; experimental entries are absent from root `__all__` but
remain resolvable for one major-version window. Public exports cannot source a
private Python package. A package-architecture test freezes the legacy flat
module set so new capability modules must choose an owner.

`DccServerBase` exposes four component accessors: `skill_discovery`,
`execution`, `lifecycle`, and `observability`. Existing flat methods remain
compatible during the major-version migration, while new functionality belongs
on a component rather than on the facade.

Standard registration behavior belongs only to `RegistrationPhase`
implementations. The ten duplicate base-class phase methods are removed. A
legacy adapter override is still detected for one compatibility window; new
host-specific behavior must be supplied as a custom phase.

The canonical Qt dispatcher source is `dcc_mcp_core.host.qt_dispatcher`, and
the Rust crate embeds that file directly. `dcc_mcp_core.qt_dispatcher` is an
identity-preserving compatibility import.

## Consequences

- New top-level Python modules fail CI.
- Stable root exports cannot point into `_server`, `_runtime`, or another
  private Python implementation package.
- Existing adapter imports and callable identities remain valid.
- Removing the compatibility facade and legacy phase overrides requires a
  future major-version decision.
- Creator Skills use the public namespaces; skill-authoring workflow and tool
  schemas are otherwise unchanged.
