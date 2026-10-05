# dcc-mcp-server-bin — PyPI distribution of the `dcc-mcp-server` binary

<!-- mcp-name: io.github.dcc-mcp/dcc-mcp-core -->

> **Status:** shipped — the wheel is built by the `build-binaries` job and
> published by `publish-server-pypi` in `.github/workflows/release.yml`.

The HTML comment above is the MCP Registry's PyPI ownership marker: it proves
this package belongs to the `io.github.dcc-mcp/dcc-mcp-core` entry in
`server.json` at the repository root. The registry reads the README as
published on PyPI, so removing or renaming the marker breaks registry
publishing on the next release.

This directory packages the `crates/dcc-mcp-server` Rust binary as a
platform-specific **binary-only** PyPI wheel, following the same pattern as
`ruff`, `uv`, `cmake`, and `pyright`. The result is a single
`pip install dcc-mcp-server` that drops the gateway / sidecar / translate
CLI onto `PATH` for Python 3.7+ regardless of which DCC the user runs.

## Why a separate PyPI package?

`dcc-mcp-core` is a PyO3 wheel — its `_core.so` is loaded into the host
Python interpreter (mayapy / blender-python / hython). The sidecar binary,
by contrast, is meant to run as its **own** OS process; bundling it into
`dcc-mcp-core` would couple two artefacts with very different release
cadences and ABI matrices. Splitting them is the standard pattern.

| Package | Distributes | Audience |
|---|---|---|
| `dcc-mcp-core` (existing) | PyO3 wheel (`_core.so` + Python facade) | Skill authors, plugin/addon code running *inside* a DCC interpreter |
| `dcc-mcp-server` (this dir) | platform-specific Python 3.7+ binary wheels | Operators, sidecar spawners, anyone who wants a standalone gateway |
| `dcc-mcp-<dcc>` (each repo) | pure-Python plugin/addon glue | DCC plugin loaders (`userSetup.py`, addon `register()`, …) |

## Layout

```
pkg/dcc-mcp-server-bin/
├── pyproject.toml              ← maturin config, bindings = "bin"
├── python/
│   └── dcc_mcp_server/
│       └── __init__.py         ← binary_path() helper for subprocess spawn
└── README.md                   ← this file
```

The Rust source is **not duplicated** here — `pyproject.toml` sets
`manifest-path = "../../crates/dcc-mcp-server/Cargo.toml"` so maturin
builds the existing workspace crate.

## Local build

```bash
# Build a wheel for the current platform / Python
cd pkg/dcc-mcp-server-bin/
vx pip install maturin
vx maturin build --release

# Resulting wheel lands in ../../target/wheels/dcc_mcp_server-*.whl
vx pip install ../../target/wheels/dcc_mcp_server-*.whl
dcc-mcp-server --help
```

The wheel uses maturin `bindings = "bin"`, so it does not load a Python
extension module and has no CPython ABI dependency. Its metadata deliberately
declares `Requires-Python: >=3.7` so embedded Python 3.7 hosts such as Maya
2022 can install it directly.

## Cross-platform CI release

Wheels are built by the `build-binaries` job in `.github/workflows/release.yml`
(there is no separate server-binary workflow) and published to PyPI by the
`publish-server-pypi` job.

| OS | Arch | Wheel platform tag |
|---|---|---|
| manylinux2014 | x86_64 | `manylinux_2_17_x86_64` |
| Windows | x86_64 | `win_amd64` |
| macOS | universal2 | `macosx_10_12_universal2` |

The wheels are retagged to `py3-none-<platform>` by
`scripts/release/server_wheel_tags.py`, so a single wheel serves every
supported Python version. Linux wheels are deliberately built for
manylinux2014 rather than a newer baseline so older DCC-hosted Python
environments such as Maya 2022 can install them.

Both jobs are gated on `release-please` creating a release. The repository is
released as a single release-please package, so the server package ships from
the same `v<version>` tags as the core release — there is no separate
dcc-mcp-server tag stream.

## Usage from a DCC plugin

```python
# In a Maya plugin / Blender addon, after `pip install dcc-mcp-server`:
import os, subprocess
from dcc_mcp_server import binary_path

_proc = subprocess.Popen([
    str(binary_path()),
    "sidecar",
    "--dcc", "maya",
    "--host-rpc", "commandport://127.0.0.1:6000",
    "--watch-pid", str(os.getpid()),
])
```

That's the entire plugin → sidecar wiring. Per-DCC `HostRpcClient`
implementations land in their respective adapter repos.
