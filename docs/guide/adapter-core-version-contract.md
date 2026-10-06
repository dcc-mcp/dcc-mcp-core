# Adapter Core Version Contract

`dcc-mcp-core` ships `0.MINOR.PATCH` releases. Under `0.x` semantics a **minor
bump may break an adapter**; only patch releases are guaranteed compatible.
Every dependency declaration an adapter publishes for core therefore has to
carry a usable upper bound — including the request that a package environment
(DCC runtime assembly) sends to its resolver.

The contract is machine-readable at
[`compatibility/core-bounds.json`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/compatibility/core-bounds.json)
and enforced by two implementations that stay in sync:

| Surface | Entry point |
|---------|-------------|
| Python (adapters, packaging tests, CI) | `dcc_mcp_core.deployment.core_bounds` |
| Rust (catalog validation, CLI) | `dcc-mcp-catalog::core_bounds` (`evaluate`, `derive_requirement`, `check_runtime`, `compare_declarations`) |
| Gate | `pytest tests/test_core_bounds.py` + `cargo test -p dcc-mcp-catalog` |

## The rules

1. **Lower bound required.** A declaration must name a floor.
2. **Upper bound required.** A declaration must name a ceiling.
3. **One minor line.** The ceiling must not admit more than
   `max_minor_lines` (default `1`) core minor lines. `>=0.19.3,<0.19.5` and
   `>=0.19.3,<0.20.0` pass; `>=0.19.3,<1.0.0` is reported as
   `upper_bound_too_wide`.
4. **Unknown specifiers fail closed.** A specifier the checker cannot interpret
   makes the whole declaration unusable rather than silently ignored.

Violation codes: `missing_lower_bound`, `missing_upper_bound`,
`upper_bound_too_wide`, `unsupported_specifier`, `unparsable_declaration`,
`inverted_range`.

## Declaration forms

Both shapes an adapter publishes are parsed:

```text
Packaging requirement       dcc-mcp-core>=0.19.3,<0.19.5
                            dcc-mcp-core~=0.20.14        → >=0.20.14,<0.21.0
                            >=0.19.3,<0.19.5; python_version >= "3.8"

Package environment request dcc_mcp_core-0.20            → >=0.20.0,<0.21.0
                            dcc_mcp_core-0.19.3..0.20.0  → >=0.19.3,<0.20.0
                            dcc_mcp_core-0               → >=0.0.0,<1.0.0  ✗
                            dcc_mcp_core                 → unbounded        ✗
```

The last two rows are the failure this contract exists for: "any 0.x" admits
every breaking minor line core has ever shipped.

## Deriving a bound when you only know a floor

```python
from dcc_mcp_core.deployment import core_bounds

core_bounds.derive_requirement("0.19.45")   # '>=0.19.45,<0.20.0'
core_bounds.evaluate("dcc_mcp_core-0")["codes"]
# ['upper_bound_too_wide']
```

Rust equivalent:

```rust
use dcc_mcp_catalog::{derive_requirement, evaluate, CoreBoundPolicy};

assert_eq!(derive_requirement("0.19.45").as_deref(), Some(">=0.19.45,<0.20.0"));
let report = evaluate("dcc_mcp_core-0", &CoreBoundPolicy::default());
assert!(!report.is_ok());
```

## Catching drift between the two declarations

An adapter's packaging metadata and its package-environment request must agree.
`compare_declarations` reports the side that admits versions the other excludes:

```python
core_bounds.compare_declarations(">=0.19.3,<0.19.5", "dcc_mcp_core-0")
# {'packaging': '>=0.19.3,<0.19.5', 'environment': 'dcc_mcp_core-0',
#  'drift': 'environment_wider', 'ok': False}
```

Drift is the actionable finding, so it is reported even when the wider side also
violates the bound policy. Only a range that cannot be read at all yields
`declaration_unusable`.

## Catching a bad combination at runtime

A resolver can still produce an unsupported combination, and when it does the
failure normally surfaces several imports away from the declaration that allowed
it. Compare the declaration against the core that is actually executing:

```python
report = core_bounds.check_runtime(">=0.19.3,<0.19.5", "0.20.28")
report["verdict"]  # 'core_newer_than_declared'
```

Read the requirement straight from the installed distribution when the adapter
does not carry its own copy:

```python
requirement = core_bounds.installed_core_requirement("dcc-mcp-maya")
core_bounds.check_runtime(requirement, core_version)["verdict"]
```

Call this during adapter start-up and log a warning; never raise — an artist
must not be blocked by a version mismatch.

## Catalog entries

`dcc-mcp-catalog.yml` entries may declare `core_requirement` next to
`min_core_version`:

```yaml
- name: "dcc-mcp-houdini"
  version: "0.38.0"
  min_core_version: "0.20.14"          # floor only — never a range
  core_requirement: ">=0.20.14,<0.21.0"  # bounded, validated on load
```

`validate_entry` rejects a `core_requirement` that violates the contract and
suggests the bounded form. An entry that declares only `min_core_version` stays
valid: a floor is not a range, and the adapter's real ceiling is not core's to
invent. The admin API exposes the field as `core_requirement` in
`/admin/api/marketplace` rows.

## Legacy declarations

Rows in [Adapter Compatibility Matrix](adapter-compatibility-matrix.md) and
older adapter releases still show the wide form `>=0.<minor>.0,<1.0.0`. Those
rows record what a release actually declared; they are reported as
`upper_bound_too_wide` by this contract. Narrow the declaration the next time
you touch the requirement — do not widen a new one to match them.
