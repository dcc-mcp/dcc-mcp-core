# Maya 2022 Support Policy

> **Effective**: 2026-07-04  
> **Expires**: 2026-12-31 (may be extended per studio demand)

## Commitment

dcc-mcp-core **sustains Maya 2022 compatibility through the end of 2026**. Maya 2022 ships with an embedded Python 3.7 interpreter, which imposes the following constraints on our codebase.

## What works on Maya 2022 / Python 3.7

### Pure-Python modules

All `dcc_mcp_core` pure-Python modules are **syntax-compatible with Python 3.7**. This includes:

- Skill scripts (`python/dcc_mcp_core/skills/`)
- Host adapter protocols (`python/dcc_mcp_core/host/`)
- Tool registration and dispatch (`python/dcc_mcp_core/_server/`)
- Schema derivation (`python/dcc_mcp_core/schema.py`)
- Installation lifecycle helpers

Type annotations use `typing.Optional`, `typing.Dict`, `typing.List`, etc. (not PEP 604 `|` syntax or built-in generics) so the modules import cleanly on Python 3.7.

### Binary-only server wheel

The `dcc-mcp-server` companion package ships a **pure Rust binary** (no Python ABI) that runs on any host. Maya 2022 can `pip install dcc-mcp-server` and launch the gateway daemon via `DccServerBase`.

## What does NOT work on Python 3.7

### Rust extension (`_core`)

The `dcc_mcp_core._core` native extension is built with **PyO3 0.29** targeting **abi3-py38**. It requires **Python 3.8+** and cannot be loaded by Python 3.7.

This means:
- `from dcc_mcp_core import ToolRegistry` works on 3.7 (pure Python)
- `from dcc_mcp_core._core import ...` fails on 3.7 (native extension)

### Semantic embeddings

The `dcc-mcp-core-semantic` companion wheel requires **Python 3.8+** (PyO3 0.29 + ONNX Runtime).

## Architecture for Maya 2022

```
┌─────────────────────────────────────────────────┐
│ Maya 2022 (Python 3.7)                          │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-core (pure Python)                │  │
│  │  - HostAdapter, SkillCatalog, schema.py   │  │
│  │  - Import works, Rust extension NOT used  │  │
│  └───────────────────────────────────────────┘  │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-server (binary wheel)             │  │
│  │  - dcc-mcp-server.exe / dcc-mcp-cli       │  │
│  │  - Pure Rust, no Python ABI               │  │
│  └───────────────────────────────────────────┘  │
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │ dcc-mcp-maya adapter                      │  │
│  │  - Maya-specific commands + UI            │  │
│  │  - Communicates via HTTP to gateway       │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

The gateway daemon (binary, no Python dependency) connects to Maya's embedded Python via the Maya adapter, which uses `dcc-mcp-core` pure-Python modules for protocol definitions and tool dispatch.

## CI enforcement

| Check | Frequency | What it verifies |
|---|---|---|
| `python-matrix-full` | Weekly | Full Python 3.8–3.14 matrix |
| `python-test` | Every PR | Python 3.8, 3.10, 3.13, 3.14 |
| `check_py37_syntax` | Every PR | Pure-Python syntax compatibility with Python 3.7 |

The `check_py37_syntax.py` script (`scripts/check_py37_syntax.py`) runs `python -m py_compile` on all `python/dcc_mcp_core/` modules using Python 3.7 to ensure no Python 3.8+ syntax leaks into the pure-Python layer.

## Writing Python 3.7-compatible code

When modifying pure-Python modules:

- **DO** use `typing.Optional[X]` instead of `X | None`
- **DO** use `typing.Dict[K, V]` instead of `dict[K, V]`
- **DO** use `typing.List[X]` instead of `list[X]`
- **DO** use `typing.Tuple[X, ...]` instead of `tuple[X, ...]`
- **DO** use `from __future__ import annotations` for forward references
- **DO NOT** use walrus operator (`:=`)
- **DO NOT** use `f"{expr=}"` debug format strings
- **DO NOT** use positional-only parameters (`/`)

## End-of-life plan

- **2026-12-31**: Maya 2022 support baseline expires
- **2026-10**: Community survey — extend or deprecate?
- **2027-01**: If deprecated, remove Python 3.7 syntax checks and compat code

## References

- [PyO3 0.29 changelog](https://github.com/PyO3/pyo3/releases/tag/v0.29.0) — dropped Python 3.7 support
- [Maya 2022 Python API](https://help.autodesk.com/view/MAYAUL/2022/ENU/?guid=Maya_SDK_py_ref_html)
- [ABI3 wheel specification](https://pyo3.rs/v0.23.0/building-and-distribution#py_limited_apiabi3)
