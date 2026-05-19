#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

docker run --rm \
  -v "$PWD:/io" \
  -e DCC_MCP_ADMIN_UI_PREBUILT=1 \
  -e CARGO_TARGET_DIR=/io/target-manylinux2014 \
  -w /io/pkg/dcc-mcp-server-bin \
  ghcr.io/pyo3/maturin:v1.13.3 \
  build --release --manylinux 2014 --out wheels
