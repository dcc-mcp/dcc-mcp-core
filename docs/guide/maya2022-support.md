# Maya 2022 / Python 3.7 Long-Term Support

> **Policy**: Python 3.7 is an LTS compatibility profile. It has no automatic
> calendar expiry. See [ADR 011](../adr/011-python-37-lts-compatibility-contract.md).

## Commitment

Maya 2022 embeds CPython 3.7, as do other DCC releases that studios still use
in production. `dcc-mcp-core` therefore keeps an installable, tested Python
3.7 path until a major release completes the formal deprecation process in
ADR 011.

The machine-readable source of truth is
[`compatibility/python.json`](../../compatibility/python.json).

## Supported wheel profiles

### Native Python 3.7

Linux and Windows releases include native `cp37-cp37m` wheels built with the
contracted PyO3 0.28 series. These wheels contain `dcc_mcp_core._core` and
provide the normal Rust-backed package surface.

This is the authoritative Python 3.7 compatibility profile.

### py37-lite fallback

The release also includes a `py3-none-any` wheel without `_core`. It provides
pure-Python host, skill, configuration, and sidecar fallbacks for platforms
where a native cp37 wheel is unavailable.

`py37-lite` is supported, but it is not evidence that native compatibility is
healthy. Merge and release gates require both profiles.

### Python 3.8+

Maintained Python versions use the `cp38-abi3` wheel. One stable-ABI build per
platform serves Python 3.8 through the maximum version declared in the
compatibility contract.

## Runtime architecture

```text
Maya 2022 / CPython 3.7
        |
        +-- native cp37 wheel available
        |      `-- Python facade + Rust _core extension
        |
        `-- no native wheel for the platform
               `-- py37-lite facade + external dcc-mcp-server sidecar
```

The companion `dcc-mcp-server` wheel contains platform binaries rather than a
Python extension ABI. It remains installable from Python 3.7 and lets lite
adapters delegate gateway execution to the sidecar.

## CI enforcement

| Check | Frequency | Contract |
| --- | --- | --- |
| `Python 3.7 contract` | Every PR | Package metadata, PyO3 series, CI/release jobs, docs, and test pins match `compatibility/python.json` |
| `Python 3.7 native (linux-x86_64)` | Every PR | Build/install native cp37 wheel, validate contents, run runtime smoke and full test suite |
| `Python 3.7 native (windows-x86_64)` | Every PR | Build/install native cp37 wheel and run runtime smoke |
| `Test (py37-lite)` | Every PR | Install lite wheel and prove fallback behavior without `_core` |
| `py37 syntax check` | Every PR | Compile shipped Python and test sources with a real CPython 3.7 parser |
| `Python 3.7 compatibility` | Every PR | Stable aggregate; fails on failed, skipped, or cancelled constituents |

Repository rulesets should require the exact aggregate job name
`Python 3.7 compatibility`.

## Authoring rules

- Keep modules importable on Python 3.7. Use postponed annotations where
  modern annotation expressions are present.
- Do not use Python 3.8+ grammar such as assignment expressions,
  positional-only parameters, debug f-strings, or `match` statements in
  shipped Python modules.
- Do not evaluate modern annotations on Python 3.7 without a compatibility
  adapter. `compile()` alone is not a runtime proof.
- Use `_typing_compat` or a documented fallback for runtime `Protocol`,
  `Literal`, and related APIs unavailable from Python 3.7's `typing` module.
- Do not import `_core` unconditionally on code paths that must support the
  lite profile.
- New public Python APIs need tests in both the native and lite profiles when
  they cross the Rust/Python boundary.

## Local validation

The vx-managed Python 3.7 is sufficient for the syntax gate but may not include
`pip` or the native import library. Building and installing a native wheel
requires a full CPython 3.7 installation with `pip` and development/import
libraries. On Windows, the standard python.org installation provides these.

PowerShell:

```powershell
$py37 = py -3.7 -c "import sys; print(sys.executable)"
vx just check-python-support
vx just check-py37-syntax
vx just build-py37 -i $py37
$wheel = Get-ChildItem dist/dcc_mcp_core-*-cp37-cp37m-*.whl | Select-Object -First 1
& $py37 scripts/ci/check_python_wheel.py --profile native_py37 --platform windows-x86_64 $wheel.FullName
& $py37 -m pip install --force-reinstall --no-deps $wheel.FullName
& $py37 scripts/ci/smoke_python37_runtime.py --profile native_py37
```

Bash:

```bash
PYTHON37="${PYTHON37:-python3.7}"
vx just check-python-support
vx just check-py37-syntax
vx just build-py37 -i "$PYTHON37"
"$PYTHON37" scripts/ci/check_python_wheel.py --profile native_py37 --platform linux-x86_64 \
  'dist/dcc_mcp_core-*-cp37-cp37m-*.whl'
wheel=$(ls dist/dcc_mcp_core-*-cp37-cp37m-*.whl | head -1)
"$PYTHON37" -m pip install --force-reinstall --no-deps "$wheel"
"$PYTHON37" scripts/ci/smoke_python37_runtime.py --profile native_py37
```

For the lite profile, use the same full interpreter. PowerShell:

```powershell
& $py37 scripts/build_py37_pure_wheel.py
$wheel = Get-ChildItem dist/dcc_mcp_core-*-py3-none-any.whl | Select-Object -First 1
& $py37 scripts/ci/check_python_wheel.py --profile lite_py37 --platform any $wheel.FullName
& $py37 -m pip install --force-reinstall --no-deps $wheel.FullName
& $py37 scripts/ci/smoke_python37_runtime.py --profile lite_py37
```

Bash:

```bash
"$PYTHON37" scripts/build_py37_pure_wheel.py
"$PYTHON37" scripts/ci/check_python_wheel.py --profile lite_py37 --platform any \
  'dist/dcc_mcp_core-*-py3-none-any.whl'
wheel=$(ls dist/dcc_mcp_core-*-py3-none-any.whl | head -1)
"$PYTHON37" -m pip install --force-reinstall --no-deps "$wheel"
"$PYTHON37" scripts/ci/smoke_python37_runtime.py --profile lite_py37
```

## Deprecation process

Python 3.7 support can be removed only after an accepted superseding ADR, a
major release, at least 180 days of notice, and a documented adapter migration
path. A dependency upgrade or hosted-runner change is not, by itself, a reason
to silently weaken the contract.

## References

- [ADR 011: Python 3.7 LTS Compatibility Contract](../adr/011-python-37-lts-compatibility-contract.md)
- [py37-lite Architecture](./py37-lite-architecture.md)
- [Adapter Compatibility Matrix](./adapter-compatibility-matrix.md)
