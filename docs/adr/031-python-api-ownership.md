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

- New top-level Python modules fail CI, unless the owner registers an exception
  under Addendum 1.
- Stable root exports cannot point into `_server`, `_runtime`, or another
  private Python implementation package.
- Existing adapter imports and callable identities remain valid.
- Removing the compatibility facade and legacy phase overrides requires a
  future major-version decision.
- Creator Skills use the public namespaces; skill-authoring workflow and tool
  schemas are otherwise unchanged.

## Addendum 1 (2026-09-22): Root-module exception register

Status: Accepted. Owner: product and delivery.

ADR 031 freezes the legacy flat module set so that every new capability must
choose an owning namespace. The mechanical gate is
`test_new_python_capabilities_do_not_grow_the_flat_namespace`, and the whitelist
it reads lives in the test file. PR #2523 (94a46ad7) was the first change to
grow that whitelist since the gate was introduced in commit c708cd5e, and it
added the module and the whitelist entry in a single commit — the gate approved
its own exception. A later change could copy that one-line comment and bypass
the boundary the same way, so the approval belongs in a document the gate
cannot edit.

### Decision

`python/dcc_mcp_core/skill_promotion.py` remains at the package root. No
relocation is required.

Rationale:

1. The issue that commissioned the work named that exact path.
2. None of the six ownership namespaces in the Decision table owns skill
   promotion. `dcc_mcp_core.skill_index` owns indexing and retrieval, and
   proposing a promotion is not retrieval.
3. The module is the payload type for `observability_query` responses rather
   than an independently owned capability: 222 lines holding four functions
   and four constants, all built around a single dataclass.

### Admission criteria for further root-level modules

A new top-level module is admitted only when all three hold:

1. The commissioning issue names the root path explicitly, or the module is the
   response or payload type of an existing root module rather than an
   independently owned capability.
2. No namespace in the Decision table owns the behaviour, and forcing it into
   one would break that namespace's boundary.
3. The module is registered in the exception register below — decision, date,
   and rationale — in the same pull request that adds it.

A comment inside `_LEGACY_TOP_LEVEL_MODULES` alone is not an admission. The
whitelist lives in the test, so an implementer who edits both in one commit
authors and approves the same exception.

### Exception register

| Module | Added by | Decision | Rationale |
|--------|----------|----------|-----------|
| `dcc_mcp_core.skill_promotion` | PR #2523 (94a46ad7) | Keep at the package root | Addendum 1, Decision |

### Consequences of this addendum

- `test_new_python_capabilities_do_not_grow_the_flat_namespace` stays the
  mechanical gate; this register is the human gate behind it.
- A root-level module without a register entry is a process defect even when CI
  is green. Reviewers reject on the missing entry, not on the test output.
- An entry is not a permanent home. Removing one means relocating the module,
  which is the migration ADR 031 already defers.
