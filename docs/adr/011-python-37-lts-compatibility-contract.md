# ADR 011: Treat Python 3.7 as an explicit LTS compatibility profile

## Status

Accepted

## Context

Maya 2022, Blender 2.83, MotionBuilder 2022, 3ds Max releases, and studio-owned
DCC hosts still embed CPython 3.7. Those interpreters cannot be upgraded
independently from the host application.

The repository previously described Python 3.7 support in several conflicting
ways:

- package metadata advertised Python 3.7;
- release automation built native `cp37-cp37m` wheels;
- pull-request CI only proved the pure-Python `py37-lite` fallback;
- documentation still described the Rust extension as Python 3.8-only; and
- the support promise had a calendar expiry unrelated to actual studio usage.

This made a green pull request insufficient evidence that a native Python 3.7
wheel would still build, import, and pass the supported test surface.

## Decision

Python 3.7 is a long-term-support compatibility profile with no automatic
calendar expiry. Removing it requires all of the following:

1. an accepted superseding ADR;
2. a major release;
3. at least 180 days of public notice; and
4. an explicit migration path for affected DCC adapters.

The canonical machine-readable policy lives in
`compatibility/python.json`. Package metadata, PyO3 constraints, wheel tags,
CI platforms, test-tool versions, and runtime smoke imports are projections of
that contract. `scripts/ci/check_python_support.py` fails when a projection
drifts.

Three build profiles are maintained:

| Profile | Purpose | Required evidence |
| --- | --- | --- |
| `native_py37` | Full Rust/Python package for CPython 3.7 | Linux and Windows `cp37-cp37m` wheel build, metadata/content validation, runtime smoke; full test suite on Linux |
| `lite_py37` | Pure-Python sidecar fallback when no native wheel is available | `py3-none-any` build, no `_core` binary, runtime fallback smoke |
| `abi3` | Stable-ABI wheel for Python 3.8+ | `cp38-abi3` content/metadata validation and the maintained-version matrix |

`py37-lite` is useful but is not evidence that native Python 3.7 support is
healthy. Native wheel failures block the aggregate `Python 3.7 compatibility`
job. Repository rulesets should require that stable job name.

PyO3 remains on the series declared in the contract. An upgrade is allowed
only when the same pull request proves that both native Python 3.7 wheel jobs
and their runtime checks still pass; otherwise the upgrade is rejected.

## Non-functional requirements

- **Reliability:** incompatible Python, wheel-tag, or native-extension changes
  fail before merge and again before publish.
- **Maintainability:** one JSON contract owns compatibility facts; scripts and
  workflows consume or validate projections instead of duplicating ad-hoc
  shell assertions.
- **Portability:** native validation covers Linux and Windows, the dominant DCC
  deployment families, while the lite profile remains platform-independent.
- **Security:** maintained Python versions keep current test tooling. Python
  3.7 uses the last compatible pinned test toolchain and does not weaken
  runtime package dependency constraints.
- **Cost:** the full 3.7 suite runs once on Linux; Windows runs native build and
  runtime smoke. Release automation still builds both native platforms.

## Failure modes and mitigations

| Failure mode | Mitigation |
| --- | --- |
| `requires-python` or classifiers drift | Static contract projection check |
| PyO3 silently drops CPython 3.7 | Native Linux and Windows PR builds |
| A wheel has the wrong tag or omits `_core` | Central wheel contract validator |
| Source compiles but fails during import | Real Python 3.7 runtime smoke |
| Lite fallback masks a broken native build | Stable aggregate gate requires both profiles |
| A constituent job is skipped | Aggregate job treats skipped/cancelled as failure |
| Documentation reintroduces a calendar expiry | Contract check rejects expiry text in policy documents |

## Consequences

### Positive

- Studio DCC deployments have an enforceable compatibility promise.
- Reviewers can rely on one stable status instead of interpreting many jobs.
- Release and pull-request validation use the same wheel semantics.
- Compatibility changes become explicit architectural decisions.

### Negative

- Every relevant pull request pays for two additional native builds.
- PyO3 upgrades may be delayed until they retain CPython 3.7 support.
- The Python 3.7 test toolchain must remain pinned because upstream tools have
  moved on.

### Neutral

- The modern package continues using `abi3-py38`; native CPython 3.7 wheels are
  separate artifacts.
- The lite profile remains supported for sidecar deployments but is not the
  primary compatibility proof.

## Alternatives considered

### Keep a fixed end date

Rejected. DCC interpreter lifecycles are controlled by vendors and studios,
not by a calendar date in this repository.

### Support only `py37-lite`

Rejected. It removes the Rust extension and therefore does not preserve the
full package contract used by existing adapters.

### Freeze all dependencies indefinitely

Rejected. Only compatibility-sensitive constraints are pinned. Other
dependencies can continue to move behind the existing CI and security gates.

### Run the full suite on every OS

Rejected for now. Linux provides the full behavioral gate; Windows adds the
native ABI and import proof. This gives high signal without tripling the
already large test cost.

## References

- [`compatibility/python.json`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/compatibility/python.json)
- [`scripts/ci/check_python_support.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/ci/check_python_support.py)
- [`scripts/ci/check_python_wheel.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/ci/check_python_wheel.py)
- [`scripts/ci/smoke_python37_runtime.py`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/ci/smoke_python37_runtime.py)
