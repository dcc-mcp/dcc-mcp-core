# py37-lite Architecture: Rust/Python Decoupling + Fallback Pattern

> **Target audience**: DCC-MCP adapter repository developers.
> **Status**: Living document — update as the fallback surface evolves.

## Table of Contents

1. [Background: Why Python 3.7](#1-background-why-python-37)
2. [Technical Constraint: PyO3 and Native Extensions](#2-technical-constraint-pyo3-and-native-extensions)
3. [Architecture Decision: Rust Extension / Python API Decoupling](#3-architecture-decision-rust-extension--python-api-decoupling)
4. [Implemented Fallback Modules](#4-implemented-fallback-modules)
5. [Maintenance Guide](#5-maintenance-guide)
6. [Adapter Compatibility Requirements](#6-adapter-compatibility-requirements)
7. [CI Gatekeeping](#7-ci-gatekeeping)
8. [Troubleshooting & FAQ](#8-troubleshooting--faq)

---

## 1. Background: Why Python 3.7

Several major DCC hosts embed Python 3.7 as their scripting runtime:

| DCC | Minimum Version | Embedded Python | EOL Concern |
|-----|----------------|-----------------|-------------|
| Maya | 2022 | 3.7 | Cannot upgrade without vendor support |
| Blender | 2.83 LTS | 3.7 | Upgraded in 2.90+ but LTS users remain |
| MotionBuilder | 2022 | 3.7 | Same Maya-generation toolchain |

**The red line**: `dcc-mcp-core` guarantees Python 3.7 compatibility through **2026-12-31**. After this date, projects may drop Python 3.7 in a minor version bump with an announcement.

### Why not drop 3.7 now?

- Maya 2022 is still widely deployed in production pipelines.
- Maya vendors typically upgrade every 2-3 major releases; 2022-era shops cannot switch to Maya 2024+ overnight.
- Breaking 3.7 compat would strand those users without any DCC-MCP access.

---

## 2. Technical Constraint: PyO3 and Native Extensions

### The problem

The Rust native extension (`_core` / `_core.pyd` / `_core.so`) is built with **PyO3 0.29**, which requires **Python 3.8+**.

Downgrading PyO3 to a 3.7-compatible version is not viable:

| Approach | Problem |
|----------|---------|
| PyO3 0.21 (last 3.7-compatible) | Loses years of safety fixes, async improvements, and ecosystem support |
| `abi3-py37` feature | Does not exist — PyO3's `abi3` support starts at py38 |
| C FFI from Rust (no PyO3) | Manual `PyObject` handling is error-prone and discards PyO3's safety guarantees |
| cPython C API directly | Maintenance nightmare — every contributor must be an FFI expert |

### The consequence

The `_core` compiled extension **cannot** be loaded on Python 3.7. Any code path that imports `dcc_mcp_core._core` will raise `ImportError` on Maya 2022, Blender 2.83, etc.

### Workflow diagram

```
                        ┌──────────────────┐
                        │  dcc-mcp-core     │
                        │  Python package   │
                        └───────┬──────────┘
                                │
                    ┌───────────┴───────────┐
                    │                       │
                    ▼                       ▼
        ┌──────────────────┐     ┌──────────────────┐
        │  py37-lite        │     │  Standard wheel   │
        │  (py3-none-any)   │     │  (cp38-abi3 /     │
        │                   │     │   platform tag)   │
        │  No _core.so      │     │  Has _core.so     │
        │  Pure Python only │     │  Rust compiled    │
        └────────┬─────────┘     └────────┬─────────┘
                 │                        │
                 ▼                        ▼
        ┌──────────────────┐     ┌──────────────────┐
        │  Python 3.7      │     │  Python 3.8+     │
        │  Maya 2022, etc. │     │  All other hosts │
        │  try/except      │     │  Direct import   │
        │  → fallback      │     │  → full speed    │
        └──────────────────┘     └──────────────────┘
```

---

## 3. Architecture Decision: Rust Extension / Python API Decoupling

### Principle

**Pure-Python modules must never hard-depend on `dcc_mcp_core._core`.**

Every public Python API that optionally uses a Rust-backed implementation must provide a pure-Python fallback, selected at import time via `try/except ImportError`.

### The canonical pattern

```python
"""Example: host/_standalone.py"""

from __future__ import annotations

from dcc_mcp_core.host._protocols import TickableDispatcher

try:
    from dcc_mcp_core._core import BlockingDispatcher
    from dcc_mcp_core._core import QueueDispatcher
except ImportError:
    from dcc_mcp_core.host._fallback import BlockingDispatcher
    from dcc_mcp_core.host._fallback import QueueDispatcher
```

### Pattern variants

There are two acceptable variants, depending on whether the module wants to surface the fallback types to callers or keep them internal:

**A. Re-export fallback (for `__init__.py`-style aggregators)**:

```python
# host/__init__.py
try:
    from dcc_mcp_core._core import BlockingDispatcher
    from dcc_mcp_core._core import DispatchError
    from dcc_mcp_core._core import QueueDispatcher
except ImportError:
    BlockingDispatcher = None
    DispatchError = None
    QueueDispatcher = None
```

Callers check `is None` to detect absence.

**B. Internal fallback (for implementation modules)**:

```python
# _server/config.py
try:
    from dcc_mcp_core._core import McpHttpConfig
except ImportError:
    @dataclass
    class McpHttpConfig:
        """Pure-Python fallback for the core HTTP config."""
        ...
```

### What NOT to do

- ❌ `import dcc_mcp_core._core` at module top level without `try/except`.
- ❌ Using `TYPE_CHECKING`-only imports that hide the runtime dependency but still require `_core` at call time.
- ❌ Importing `_core` inside frequently-called functions (performance cost + hidden dependency).
- ❌ PEP 604 syntax (`str | None`, `dict[str, Any]`) **even with** `from __future__ import annotations` — it is a syntax error on Python 3.7. Use `typing.Optional`, `typing.Dict`, `typing.List`, etc.

---

## 4. Implemented Fallback Modules

### 4.1 Pure-Python wheel (PIP-2531)

**Artifact**: `dcc_mcp_core-<version>-py3-none-any.whl`

**Build**: `scripts/build_py37_pure_wheel.py` — assembles a wheel from `python/dcc_mcp_core/` sources **only**, without compiling any Rust code.

**Cargo feature**: `py37-lite` — intentionally empty; it exists so `cargo build -p dcc-mcp-core --no-default-features -F py37-lite` signals "don't compile Rust extensions."

**Invocation**: `just build-py37` (aliased to `python scripts/build_py37_pure_wheel.py`).

**Validation**: CI verifies the wheel tag is `py3-none-any` and confirms `dcc_mcp_core._core` raises `ImportError`.

### 4.2 Import fallback (PIP-2532)

**Scope**: Every module that optionally imports `_core` types.

**Files using the pattern** (source of truth — update when adding new fallbacks):

| File | Imports from `_core` | Fallback source |
|------|---------------------|-----------------|
| `host/__init__.py` | `BlockingDispatcher`, `DispatchError`, `PostHandle`, `QueueDispatcher`, `TickOutcome` | Sets to `None` |
| `host/_standalone.py` | `BlockingDispatcher`, `QueueDispatcher` | `host/_fallback` |
| `host/_protocols.py` | `TickOutcome` | `host/_fallback` |
| `host/_wire.py` | `normalize_tool_arguments`, `normalize_tool_meta` | Inline pure-Python functions |
| `_server/config.py` | `McpHttpConfig` | Pure-Python `@dataclass` |
| `_server/config.py` | `resolve_mcp_http_config_class` | `_runtime/config_bridge` then fallback to `_core` |
| `_server/skill_discovery.py` | Various | (check per import) |
| `_server/execution_bridge.py` | Various | (check per import) |
| `_runtime/server_factory.py` | Various | (check per import) |
| `_runtime/skill_paths.py` | Various | (check per import) |
| `_runtime/config_bridge.py` | Various | (check per import) |
| `dcc_server.py` | `SandboxContext`, `SandboxPolicy`, `ToolRecorder`, `TransportAddress` | Graceful `ImportError` — feature degrades |
| `skill.py` | Various | (check per import) |

**Fallback implementations**:

- `host/_fallback.py` — `QueueDispatcher`, `BlockingDispatcher`, `PostHandle`, `TickOutcome`, `DispatchError`, utility functions. A fully functional pure-Python replacement used when `_core` is absent.
- `_typing_compat.py` — `Protocol`, `runtime_checkable`, `Literal` backports for Python 3.7 (where `typing.Protocol` is unavailable).
- Inline `@dataclass` definitions — simple value types like `McpHttpConfig` are redefined as pure-Python `@dataclass` in the fallback path.

### 4.3 HTTP server decoupled to sidecar (PIP-2533)

**The HTTP server** (Streamable HTTP MCP transport) was previously inlined in `dcc-mcp-core` but is now a **separate binary**: `dcc-mcp-server`.

- `dcc-mcp-server` is built as a **native binary wheel** (`cp38-abi3-*`) and installs **only on Python 3.8+**.
- On Python 3.7 (Maya 2022), the pure-Python wheel does **not** ship `dcc-mcp-server` at all.
- Maya 2022 users install `dcc-mcp-server` separately (via the sidecar/gateway pattern) when their platform provides a compatible binary.

**Dependency split in `pyproject.toml`**:

```toml
# Core (always installed)
dependencies = [
    # ...
]
# Server (only on 3.8+)
[project.optional-dependencies]
server = [
    "dcc-mcp-server>=0.18.17,<1.0.0; python_version >= '3.8'",
]
```

**Result**: The `py37-lite` wheel has zero Rust code and can be installed on Python 3.7. Users who need the MCP HTTP server on 3.7 deploy it through the sidecar/gateway mechanism.

---

## 5. Maintenance Guide

### 5.1 Adding a new Rust extension feature

When you add a new type/function to the Rust `_core` module that should be callable from Python:

1. **Implement the pure-Python fallback first** (or at least in the same PR).
2. In the Python module that consumes it, use the `try/except ImportError` pattern.
3. If the type has no sensible pure-Python implementation, make the feature degrade gracefully:
   - Set the import target to `None`.
   - Guard call sites with `if Thing is not None:`.
   - Log a debug message when falling back.

### 5.2 Priorities

| Priority | Principle |
|----------|-----------|
| P0 | Core dispatcher, host adapter, and skill APIs must never crash on 3.7 |
| P1 | New features should degrade gracefully on 3.7, not hard-fail |
| P2 | Pure-Python fallback performance matters for hot paths (dispatcher ticks) |
| P3 | Nice-to-have — CLI sugar, admin UI, analytics |

### 5.3 Syntax Rules

All Python files that are packaged into the `py37-lite` wheel **must**:

- Use `from __future__ import annotations` (enables postponed evaluation of annotations).
- Avoid PEP 604 union syntax (`X | Y`) — use `typing.Union[X, Y]` or `Optional[X]`.
- Avoid PEP 585 builtin generics (`list[X]`, `dict[K, V]`) — use `typing.List[X]`, `typing.Dict[K, V]`.
- Avoid walrus operator (`:=`), match/case, and other Python 3.8+ syntax.
- Avoid `f-strings` in `scripts/check_py37_syntax.py` specifically — the script itself runs on 3.7.

Exception: files in `_server/`, `_runtime/`, and other directories that are **never** packaged into the py37-lite wheel may use modern syntax. See `pyproject.toml` → `[tool.ruff.lint.per-file-ignores]` for the current exemption list.

### 5.4 Testing

- **`scripts/check_py37_syntax.py`** — runs on CI with a real Python 3.7 interpreter to catch syntax errors. All pure-Python sources are scanned.
- **`test-py37` CI job** — installs the py37-lite wheel on Python 3.7 and runs import + smoke tests.
- **Local testing**: `scripts/run_with_py37.py <script.py>` locates a 3.7 interpreter and invokes it.

---

## 6. Adapter Compatibility Requirements

### 6.1 Python 3.7 adapters

The following adapters **must** install correctly on Python 3.7 (use `py37-lite` wheel):

| Adapter | Python Dependency | Notes |
|---------|------------------|-------|
| `dcc-mcp-maya` | Core only (pure-Python) | Maya 2022 = Python 3.7 |
| `dcc-mcp-blender` | Core only (pure-Python) | Blender 2.83 LTS = Python 3.7 |
| `dcc-mcp-motionbuilder` | Core only (pure-Python) | MB 2022 = Python 3.7 |
| Any adapter using pure-Python dispatcher | Core only | No sidecar binary needed |

### 6.2 Python 3.8+ adapters

These adapters always use the **standard wheel** (with `_core` compiled extension):

| Adapter | Python Dependency | Notes |
|---------|------------------|-------|
| `dcc-mcp-3dsmax` | Core + optional server | 3ds Max 2025+ embeds Python 3.10+ |
| `dcc-mcp-houdini` | Core + optional server | Houdini 20.5 embeds Python 3.10+ |
| `dcc-mcp-photoshop` | Core + optional server | UXP Python 3.8+ |
| `dcc-mcp-unreal` | Core + optional server | UE 5.x Python 3.8+ |
| `dcc-mcp-fpt` | Core + server | REST bridge, Python 3.8+ |
| CLI / headless use | Core + server | Always Python 3.8+ |

### 6.3 What adapter authors need to do

1. **Declare dependency correctly** in `pyproject.toml`:
   ```toml
   dependencies = [
       "dcc-mcp-core>=0.18.0,<1.0.0",
   ]
   ```
   (Do NOT constrain to `py37-lite` — pip selects the right wheel automatically.)

2. **Test on Python 3.7** if your target DCC embeds it. Use a local 3.7 interpreter or CI job.

3. **Never import `_core` directly** in adapter code that should work on 3.7.

4. **Use the same `try/except ImportError` pattern** if your adapter has optional native extensions.

---

## 7. CI Gatekeeping

### CI jobs protecting py37-lite

| Job | What it does | Failure impact |
|-----|-------------|----------------|
| `build-py37-wheel` | Builds the pure-Python wheel and verifies it's `py3-none-any` | Blocks merge |
| `test-py37` | Installs py37-lite wheel on real Python 3.7, runs import + smoke tests | Blocks merge |
| `py37-syntax-check` | Runs `check_py37_syntax.py` on a real Python 3.7 interpreter | Blocks merge |

### Running locally

```bash
# Build the py37-lite wheel
just build-py37

# Check syntax (requires Python 3.7 installed)
python3.7 scripts/check_py37_syntax.py

# Or use the helper
python scripts/run_with_py37.py scripts/check_py37_syntax.py
```

---

## 8. Troubleshooting & FAQ

### Q: Why does `import dcc_mcp_core` fail on Python 3.7?

Check that you installed the py37-lite wheel. The standard wheel contains `_core.pyd`/`_core.so` which cannot load on Python 3.7:

```bash
# Wrong — installs the latest wheel (may be cp38-abi3 which fails on 3.7)
pip install dcc-mcp-core

# Right — force the pure-Python wheel
pip install "dcc-mcp-core; python_version >= '3.7'"
```

If using a local build:
```bash
just build-py37
pip install dist/dcc_mcp_core-*-py3-none-any.whl
```

### Q: I'm adding a new feature with a Rust component. What's my checklist?

1. [ ] Implement pure-Python fallback for the Python-facing API.
2. [ ] Use `try/except ImportError` to select implementation.
3. [ ] Add the fallback file in the appropriate location (e.g., `host/_fallback.py` for dispatcher types).
4. [ ] Update the table in section 4.2 of this document.
5. [ ] Verify `scripts/check_py37_syntax.py` passes.
6. [ ] Verify `just build-py37 && pip install dist/*py3-none-any.whl && python3.7 -c "from dcc_mcp_core import ..."` works.
7. [ ] Update CI test job if the new fallback needs smoke testing.

### Q: Can I use `TYPE_CHECKING` to avoid importing `_core` at runtime?

Yes, but only when the import is purely for type annotations:

```python
from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from dcc_mcp_core._core import SomeRustType  # never loaded at runtime

def f(x: SomeRustType) -> None:  # evaluated as string, safe on 3.7
    ...
```

However, any callable that _needs_ the Rust type at runtime must still use the `try/except` pattern. `TYPE_CHECKING` is not a fallback mechanism.

### Q: Why does CI use `ubuntu-22.04` instead of `ubuntu-latest` for py37 jobs?

Python 3.7 is not available in the `actions/setup-python` tool cache on `ubuntu-24.04+` and must be installed manually — but it **is** available on `ubuntu-22.04` via `apt`. The `setup-windows-python37` action handles Windows via direct download from python.org.

---

## References

- [CLAUDE.md](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/CLAUDE.md) — Red line: Python 3.7 support through 2026-12-31.
- [Cargo.toml](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/Cargo.toml#L266-L269) — `py37-lite` feature definition.
- [scripts/build_py37_pure_wheel.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/build_py37_pure_wheel.py) — Wheel builder.
- [scripts/check_py37_syntax.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/check_py37_syntax.py) — Syntax gate.
- [scripts/run_with_py37.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/scripts/run_with_py37.py) — Local 3.7 runner helper.
- [host/_fallback.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/_fallback.py) — Pure-Python dispatcher fallback.
- [host/__init__.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/__init__.py) — Re-export with fallback to `None`.
- [host/_standalone.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/_standalone.py) — Import fallback at use site.
- [host/_protocols.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/_protocols.py) — `TickOutcome` fallback.
- [_typing_compat.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/_typing_compat.py) — Protocol/Literal backport.
- [_server/config.py](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/_server/config.py) — `McpHttpConfig` fallback `@dataclass`.
- [ADR 002: DCC Main-Thread Affinity](../adr/002-dcc-main-thread-affinity.md) — Why dispatchers exist.
- [Adapter Compatibility Matrix](./adapter-compatibility-matrix.md) — Per-adapter version tracking.
- [agents-reference.md](./agents-reference.md) — Full agent integration guide.
