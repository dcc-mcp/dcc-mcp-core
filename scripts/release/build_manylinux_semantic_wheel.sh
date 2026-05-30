#!/usr/bin/env bash
# Build the `dcc-mcp-core-semantic` companion wheel inside the pyo3/maturin
# docker image. Two build modes:
#
#   abi3  → single wheel for CPython 3.8-3.13 (features without `abi3-py38`
#           omitted; this script forces it back on so the abi3 ABI tag is
#           emitted regardless of the host interpreter).
#   py37  → cp37-cp37m wheel for embedded DCC hosts (Maya 2022, 3ds Max 2022)
#           that still ship Python 3.7. Built with `-i python3.7`.
#
# NOTE on manylinux tag: fastembed-rs pulls `ort` (ONNX Runtime), which
# requires glibc >= 2.27. That rules out manylinux2014 (CentOS 7, glibc 2.17)
# used by `pkg/dcc-mcp-server-bin/`; semantic wheels MUST target
# manylinux_2_28 (CentOS 8 / RHEL 8). The pyke.io CDN ships matching
# prebuilt ORT binaries for this baseline.
#
# The wheel build runs under an isolated CARGO_TARGET_DIR so it never
# collides with the host's `target/` directory (host build scripts may
# have been compiled against a newer glibc and would fail to execute
# inside the container).
set -euo pipefail

mode="${1:-abi3}"

case "$mode" in
  abi3)
    maturin_args=(--features python-bindings,ext-module,abi3-py38)
    ;;
  py37)
    maturin_args=(--features python-bindings,ext-module -i python3.7)
    ;;
  *)
    echo "::error::unknown mode '$mode' (expected abi3|py37)" >&2
    exit 1
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

maturin_args_str="${maturin_args[*]}"
# The `ghcr.io/pyo3/maturin` image declares `maturin` as its ENTRYPOINT, so
# any trailing argv is forwarded to `maturin` (running `... sh -c "..."`
# therefore fails with `unrecognized subcommand 'sh'`). Override the
# entrypoint to `/bin/sh` so the shell receives `-c "..."` directly.
#
# The maturin v1.13.3 image is built FROM `quay.io/pypa/manylinux2014_x86_64`
# (CentOS 7 / glibc 2.17, OpenSSL 1.0.2). `dcc-mcp-semantic`'s fastembed
# dep is pinned to **rustls** (`hf-hub-rustls-tls`,
# `ort-download-binaries-rustls-tls`) so the wheel build is OpenSSL-free
# and there is no need to install openssl-devel inside the container.
# The `--manylinux 2_28` flag forward-tags the produced wheel; glibc 2.17
# is a strict subset of 2.28 so the wheel still runs on every
# manylinux_2_28 host.
docker run --rm \
  --entrypoint /bin/sh \
  -v "$PWD:/io" \
  -e CARGO_TARGET_DIR=/io/target-manylinux-semantic \
  -w /io/pkg/dcc-mcp-core-semantic \
  ghcr.io/pyo3/maturin:v1.13.3 \
  -c "maturin build --release --manylinux 2_28 --out wheels ${maturin_args_str}"

if command -v sudo >/dev/null 2>&1; then
  sudo chown -R "$(id -u):$(id -g)" pkg/dcc-mcp-core-semantic/wheels
fi
