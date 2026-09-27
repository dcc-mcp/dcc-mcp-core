# dcc-mcp-cli-bin — PyPI wrapper for the `dcc-mcp-cli` binary

This directory builds the `dcc-mcp-cli` PyPI project: one platform-specific
wheel per release that puts the Rust CLI on `PATH`.

```bash
# Run it without installing anything
uvx dcc-mcp-cli --version

# Or install it into a tool environment / venv
uv tool install dcc-mcp-cli
pip install dcc-mcp-cli
```

## Why a wrapper instead of a raw binary wheel?

`pkg/dcc-mcp-server-bin` ships its raw executable through maturin's
`bindings = "bin"`. `dcc-mcp-cli` deliberately does not:

| | Raw binary wheel | This wrapper |
|---|---|---|
| Per-release PyPI storage | ~165 MB (three platforms) | ~66 MB (three platforms) |
| PyPI 10 GB budget | ~60 releases | ~152 releases |
| Payload | the executable | the release zip, unpacked on first use |

The wheel embeds the same `dcc-mcp-cli-<version>-<platform>.zip` the GitHub
Release publishes, verifies its SHA-256 against `payload.json`, and unpacks it
into the environment's scripts directory on the first invocation.

## Layout

```
pkg/dcc-mcp-cli-bin/
├── pyproject.toml                 # hatchling; console script dcc-mcp-cli
├── python/dcc_mcp_cli/
│   ├── __init__.py                # public API: binary_path(), main()
│   ├── __main__.py                # python -m dcc_mcp_cli
│   ├── _bootstrap.py              # unpack + exec
│   └── _payload/                  # staged at build time, git-ignored
│       ├── payload.json
│       └── dcc-mcp-cli-<version>-<platform>.zip
└── tests/test_cli_bootstrap.py
```

## Where the binary lands, and why it matters

The bootstrap unpacks the executable into the scripts directory of the
interpreter that installed the wheel — `<venv>/bin` on POSIX,
`<venv>\Scripts` on Windows — and then executes it. Consequences:

- **Component contract.** `current_exe.parent()` resolves to that directory, so
  `dcc-mcp-cli components ensure dcc-cua` installs `dcc-cua` as a sibling and
  detects it on the next run. A package manager must therefore keep the CLI in
  a *single* directory; unpacking into `site-packages` would break it.
- **POSIX vs Windows naming.** On POSIX the unpacked binary replaces the
  console script, so `dcc-mcp-cli` becomes the native executable directly.
  On Windows the pip-generated `dcc-mcp-cli.exe` launcher is the running
  process image and cannot be replaced while mapped, so the binary is unpacked
  as `dcc-mcp-cli-bin.exe` and the launcher execs it.
- **Read-only prefixes.** If the scripts directory is not writable, the
  bootstrap falls back to a per-user directory
  (`~/.local/share/dcc-mcp-cli/bin`, or `%LOCALAPPDATA%\dcc-mcp-cli\bin`).
  Set `DCC_MCP_CLI_BIN_DIR` to pin the location explicitly.

`dcc-cua` is intentionally **not** distributed here. It is an independently
released companion executable that `dcc-mcp-cli components ensure` reconciles
by version next to the CLI; shipping it as a second distribution would create
two copies and version drift.

## Self-update

`dcc-mcp-cli update` works for every install, this one included — updating
through the CLI is the supported flow everywhere. The bootstrap writes
`dcc-mcp-cli.package-manager.json` next to the unpacked binary; the CLI reads
it as *provenance*, not as a lock, and uses it to report which manager
installed the copy and which version that manager still has on record.

Because the update replaces the binary on the next launch, the manager's own
metadata falls behind the binary on disk. `update apply` has not applied
anything yet — it stages the update — so the advisory says so and names the
command that re-syncs the metadata afterwards:

```json
"package_manager": {
  "manager": "pypi",
  "recorded_version": "0.20.34",
  "advisory": "the update will replace this binary in place on the next launch; the pypi package manager will still record version 0.20.34. ..."
}
```

The marker records the size of the binary it unpacked, and the CLI updates
that record when it applies a staged self-update, so the replacement is not
mistaken for a damaged install and re-unpacked from the wheel.

Re-sync the manager's metadata at any time — it also restores the packaged
binary if you prefer the manager to own the version:

```bash
uv tool upgrade dcc-mcp-cli
pipx upgrade dcc-mcp-cli
pip install --upgrade dcc-mcp-cli
```

## Building locally

```bash
# 1. Build the release archive (example: current platform)
cargo build --release -p dcc-mcp-cli
python scripts/release/build_standalone_bundle.py \
  --version 0.20.34 \
  --platform windows-x86_64 \
  --binary-name dcc-mcp-cli \
  --binary-path target/release/dcc-mcp-cli.exe \
  --out-dir dist

# 2. Stage the archive and build the retagged wheel
python -m pip install "build>=1.2" "hatchling>=1.25" "wheel>=0.46"
python scripts/release/build_cli_wrapper_wheel.py \
  --version 0.20.34 \
  --platform windows-x86_64 \
  --zip dist/dcc-mcp-cli-0.20.34-windows-x86_64.zip \
  --out-dir wheels
python scripts/release/cli_wheel_tags.py validate --wheel-dir wheels --version 0.20.34
```

CI runs the same three steps in the `build-cli-wheels` job of
`.github/workflows/release.yml`, once per platform, from the archives the
`build-binaries` job already produced.

## Python compatibility

The bootstrap is pure Python with no third-party imports and runs unchanged on
Python 3.7 (Maya 2022), so the project's LTS floor applies: the wheel declares
`Requires-Python: >=3.7` and carries the full 3.7–3.14 classifier set. The
binary it unpacks has no CPython ABI dependency — the interpreter only ever
starts it as a child process.
