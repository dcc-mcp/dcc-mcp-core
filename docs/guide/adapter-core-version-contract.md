# Adapter to Core Version Contract

Adapters declare which `dcc-mcp-core` release they support **twice**, and only
one of the two declarations is enforced by a resolver. This document defines
the contract between them, the tooling that derives the second from the first,
and the gate that fails when they disagree.

## Why two declarations drift

| Declaration | Read by | Written | Enforced |
|-------------|---------|---------|----------|
| `Requires-Dist: dcc-mcp-core>=0.19.3,<0.19.5` in the wheel | `pip` | generated from `pyproject.toml` | yes |
| `requires = ["dcc_mcp_core-0"]` in `package.py` | the studio package-environment resolver | by hand | only if it says something |

The package-environment requirement is hand-maintained and drifts. Because it
drifts *wider*, a resolver is free to select a core release the adapter
explicitly excluded on PyPI — `dcc_mcp_core-0` admits anything in the `0.x`
line. The resulting environment imports cleanly, and `import dcc_mcp_core`,
`import dcc_mcp_maya`, and `import dcc_mcp_maya.server` all succeed. The
failure only surfaces at the next layer, as an `ImportError` from a partially
initialised module that never mentions the version.

That gap is not a bug in the resolver. It is a missing contract: nothing ever
stated that the two declarations must agree, so nothing checked.

## The contract

The machine-readable half lives in
[`compatibility/adapter-core-requirement.json`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/compatibility/adapter-core-requirement.json)
and is enforced by `scripts/ci/check_adapter_core_requirement.py`. Four rules:

1. **The Python distribution metadata is the source of truth.** The
   package-environment requirement is derived from it, never the reverse.
2. **An upper bound is mandatory.** A declaration without a ceiling is a
   violation even when the installed core happens to satisfy it.
3. **The upper bound is exclusive.** `<0.19.5` renders as the `..0.19.5` range
   token. `<=0.19.5` has no equivalent token and is rejected rather than
   silently widened.
4. **The package-environment requirement must never be wider than the declared
   range.** Narrower is safe; wider is how a resolver escapes the range.

## Deriving the package-environment requirement

Do not type the token. Generate it from the declaration you already maintain:

```python
# In the adapter's release tooling, or a one-off:
python -c "import dcc_mcp_core.version_compat.core_requirement as m; print(m.package_environment_requirement_for('dcc-mcp-maya'))"
# dcc_mcp_core-0.19.3..0.19.5
```

The same conversion is available as a library call:

```python
from dcc_mcp_core.version_compat.core_requirement import requirement_from_pep440

requirement = requirement_from_pep440("dcc-mcp-core>=0.19.3,<0.19.5")
requirement.to_package_environment()   # 'dcc_mcp_core-0.19.3..0.19.5'
requirement.to_pep440()                # 'dcc-mcp-core>=0.19.3,<0.19.5'
```

| Package-environment token | Meaning |
|---------------------------|---------|
| `dcc_mcp_core` | any version — always a violation |
| `dcc_mcp_core-0` | any `0.x` release |
| `dcc_mcp_core-0.19.3+` | `0.19.3` or newer, no ceiling |
| `dcc_mcp_core-0.19.3..0.19.5` | `0.19.3` up to but excluding `0.19.5` |

The `..` range operator is exclusive at the upper end, so it maps directly onto
PEP 440 `<`. A truncated version is read as a prefix floor: `dcc_mcp_core-0`
means `>=0.0.0,<1.0.0`, which is why it looks bounded while excluding nothing
an adapter actually cares about.

## Checking at release time

Add the gate to the adapter's release workflow. It exits non-zero when the
package-environment requirement is wider than the declaration, when a
requirement cannot be parsed, or when `package.py` does not require core at
all:

```yaml
- name: Check the core requirement contract
  run: python scripts/check_core_requirement.py --adapter-root . --github
```

Vendor
[`scripts/ci/check_adapter_core_requirement.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/ci/check_adapter_core_requirement.py)
into the adapter the same way the release-workflow digest kit is shared across
the organisation, so a change on core's `main` cannot alter what an adapter's
release gate enforces. The script is stdlib-only; it reads the parsing rules
from the installed `dcc_mcp_core.version_compat.core_requirement` so both sides always agree
on what a bound means.

Adapters that would rather not vendor a file can assert the same invariant in
their own test suite:

```python
from dcc_mcp_core.version_compat.core_requirement import compare_requirements
from dcc_mcp_core.version_compat.core_requirement import requirement_from_package_environment
from dcc_mcp_core.version_compat.core_requirement import requirement_from_pep440


def test_package_environment_requirement_is_not_wider_than_declared() -> None:
    declared = requirement_from_pep440("dcc-mcp-core>=0.19.3,<0.19.5")
    observed = requirement_from_package_environment("dcc_mcp_core-0.19.3..0.19.5")
    assert compare_requirements(declared, observed) == []
```

The gate reads `[project].dependencies` from `pyproject.toml` and the literal
`requires` list from `package.py`. It parses `package.py` with `ast` and never
executes it, so a checkout cannot run code just by being scanned. A `requires`
list that is not a literal list of strings is reported as an error, never
treated as an exemption.

## Failing early at runtime

An adapter that wants the hard failure calls the guard while importing:

```python
# dcc_mcp_maya/__init__.py
from dcc_mcp_core.version_compat.core_requirement import enforce_core_compatibility

enforce_core_compatibility(__name__)   # reads dcc-mcp-maya's own Requires-Dist
```

`enforce_core_compatibility` raises `CoreRequirementError` naming the adapter,
its declared range, the running core version, and the remedy. Set
`DCC_MCP_CORE_REQUIREMENT_ENFORCE=0` to downgrade it to a warning.

`DccServerBase` performs the same comparison as a startup warning on every
server it builds, so even an adapter that has not adopted the guard produces a
log line naming the version. It is best-effort — when the adapter's metadata
cannot be read there is nothing to compare — and shares the
`DCC_MCP_CORE_VERSION_CHECK=0` opt-out with the version provenance check.

## Ceiling policy: `<1.0.0` is not a ceiling

Core is still `0.x`, so `<1.0.0` excludes nothing that exists. It is a
placeholder adapters copy from the compatibility matrix, not a release anybody
verified against, and the matrix's historical advice to pin
`>=X.Y.0,<1.0.0` is exactly what let a resolver pair an adapter with a core
five minors ahead of its tested range.

The target is a **tested ceiling**: bound core at the next minor above the
highest release the adapter was verified against.

```python
# pyproject.toml — verified against 0.20.14, so the ceiling is 0.21
dependencies = ["dcc-mcp-core>=0.20.14,<0.21.0"]
```

```python
# package.py — derived from the line above, never written by hand
requires = ["dcc_mcp_core-0.20.14..0.21.0"]
```

The gate reports an open `<1.0.0` ceiling as a **warning**, not an error: every
adapter row in the compatibility matrix currently uses it, and flipping the
severity would fail all of them in one commit. New adapters should use a tested
ceiling from the start; existing ones move when they next touch their pin.

## Known violations

`compatibility/adapter-core-requirement.json` records the adapters observed
carrying a package-environment requirement wider than their declaration. Each
row stores the declared range, the observed token, and the token the contract
requires, and the gate recomputes the drift codes from those two inputs — a row
whose codes no longer match is a build failure.

Rows are **warnings** in core's CI, because the fix belongs to the adapter
repository. They become errors in the adapter's own CI the moment that adapter
runs the gate against itself.

## See also

- [adapter-compatibility-matrix.md](adapter-compatibility-matrix.md) — core pins, and the matrix rows the catalog mirrors
- [adapter-release-checklist.md](adapter-release-checklist.md) — where the contract check sits in the release train
- [rez-skill-packages.md](rez-skill-packages.md) — package layout and the environment variables a Rez package contributes
- [`dcc_mcp_core.version_compat.core_requirement`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/version_compat/core_requirement.py) — the parsing, rendering, and enforcement API
