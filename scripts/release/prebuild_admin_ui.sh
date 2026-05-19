#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

vx npm --prefix admin-ui ci
vx npm --prefix admin-ui run build
test -f crates/dcc-mcp-gateway/src/gateway/admin/generated/index.html
