# Python 3.7 Compatibility Architecture

> **Audience**: core and adapter maintainers.
> **Decision**: [ADR 011](../adr/011-python-37-lts-compatibility-contract.md).
> **Contract**: [`compatibility/python.json`](../../compatibility/python.json).

## Why this profile exists

DCC applications embed Python and usually cannot adopt a newer interpreter
without upgrading the entire host. Python 3.7 therefore remains an LTS target
for Maya 2022, Blender 2.83, MotionBuilder 2022, and studio-owned hosts.

The architecture separates three concerns that must not be conflated:

1. source-level Python 3.7 compatibility;
2. native CPython 3.7 wheel compatibility; and
3. a pure-Python fallback for platforms without a native wheel.

## Build profiles

```text
                         dcc-mcp-core source
                                  |
              +-------------------+-------------------+
              |                   |                   |
              v                   v                   v
       native_py37           lite_py37              abi3
       cp37-cp37m            py3-none-any           cp38-abi3
       includes _core        excludes _core         includes _core
       Python 3.7            Python 3.7             Python 3.8+
```

### `native_py37`

The native profile uses the PyO3 series pinned in the compatibility contract
and does not enable `abi3-py38`. Linux and Windows wheels are built separately
for CPython 3.7. This is the full package and the authoritative LTS proof.

### `lite_py37`

The lite profile packages the Python tree without a compiled extension. Public
entry points select pure-Python or sidecar-backed implementations when `_core`
is absent. It is a portability fallback, not a replacement for the native
profile.

### `abi3`

The modern profile enables `abi3-py38`. One wheel per platform serves all
maintained Python versions from 3.8 upward.

## Import boundary

Modules that support the lite profile must treat `_core` as optional:

```python
try:
    from dcc_mcp_core._core import BlockingDispatcher
except ImportError:
    from dcc_mcp_core.host._fallback import BlockingDispatcher
```

Use this pattern only at an ownership boundary. Do not scatter broad
`except ImportError` blocks through business logic; they can hide unrelated
missing imports. Keep fallback selection in the facade or factory responsible
for that capability.

For type-only imports, use `TYPE_CHECKING`:

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from dcc_mcp_core._core import ToolRegistry
```

## Python 3.7 typing behavior

`from __future__ import annotations` postpones annotation evaluation. That lets
many modern annotation expressions compile on Python 3.7, but it does not make
their runtime evaluation safe. Code that calls `typing.get_type_hints` or
stores evaluated aliases must use a compatibility layer.

Python 3.7's `typing` module lacks several runtime APIs used by newer code,
including `Literal`, `Protocol`, and `runtime_checkable`. Shared behavior
belongs in `dcc_mcp_core._typing_compat`; do not duplicate private fallbacks in
individual modules.

## Source rules

Shipped modules and Python 3.7 tests must avoid grammar introduced after 3.7:

- assignment expressions (`:=`);
- positional-only parameter markers (`/`);
- debug f-strings (`f"{value=}"`);
- structural pattern matching; and
- other syntax rejected by a real CPython 3.7 compiler.

PEP 585 and PEP 604 annotation expressions require extra care: postponed
annotations can make them importable, while later evaluation can still fail.
Runtime smoke and tests provide the proof; a compile-only check does not.

## CI layers

```text
compatibility/python.json
          |
          +-- projection check (metadata, PyO3, workflows, docs)
          +-- real CPython 3.7 syntax check
          +-- native Linux wheel -> content check -> runtime smoke -> full suite
          +-- native Windows wheel -> content check -> runtime smoke
          +-- lite wheel -> content check -> fallback runtime smoke
          `-- stable aggregate status
```

The aggregate `Python 3.7 compatibility` status must be required in the
repository ruleset. Its `if: always()` behavior ensures that skipped and
cancelled dependencies cannot look green.

## Adding a Python-facing feature

1. Decide whether the feature is available in native 3.7, lite 3.7, or both.
2. Keep Rust extension imports behind the owning facade when lite is supported.
3. Add the critical import to `compatibility/python.json` when it belongs to
   the guaranteed runtime surface.
4. Add native behavior tests. Add lite behavior tests when a fallback exists.
5. Run `vx just check-python-support` and `vx just check-py37-syntax`.
6. Let CI prove native Linux/Windows wheel construction.

## Local commands

Use vx for the parser gate. Wheel building/runtime validation requires a full
CPython 3.7 installation with `pip` and development/import libraries; set
`PYTHON37` to that executable. See
[Maya 2022 Support](./maya2022-support.md#local-validation) for PowerShell.

```bash
# Static contract and real-parser checks
vx just check-python-support
vx just check-py37-syntax

# Native wheel
PYTHON37="${PYTHON37:-python3.7}"
vx just build-py37 -i "$PYTHON37"
"$PYTHON37" scripts/ci/check_python_wheel.py --profile native_py37 \
  'dist/dcc_mcp_core-*-cp37-cp37m-*.whl'

# Lite wheel
"$PYTHON37" scripts/build_py37_pure_wheel.py
"$PYTHON37" scripts/ci/check_python_wheel.py --profile lite_py37 \
  'dist/dcc_mcp_core-*-py3-none-any.whl'
```

## Operational guidance

- Prefer the native wheel when one exists for the DCC platform.
- Use lite only when the platform has no supported native artifact or when the
  adapter intentionally delegates execution to `dcc-mcp-server`.
- Never work around an incompatible native build by publishing only lite; that
  turns a release failure into a silent capability loss.
- A PyO3 upgrade must prove native Python 3.7 in the same change.

## Deprecation

There is no date-based removal. A future change must supersede ADR 011, ship in
a major release, provide at least 180 days of notice, and document adapter
migrations.
