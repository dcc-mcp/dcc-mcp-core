#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# npm occasionally crashes with "Exit handler never called!" on CI runners
# (see rust-coverage.yml). Retry a few times before failing the release job.
vx node --version

max_attempts=3
for attempt in $(seq 1 "$max_attempts"); do
  if vx npm --prefix admin-ui ci --ignore-scripts --include=optional; then
    break
  fi
  if [[ "$attempt" -eq "$max_attempts" ]]; then
    echo "::error::npm ci failed after ${max_attempts} attempts" >&2
    exit 1
  fi
  echo "::warning::npm ci failed (attempt ${attempt}/${max_attempts}); retrying in 15s..." >&2
  sleep 15
done

# npm ci may skip transitive optional deps (npm 10.x quirk).
# Explicitly install the platform-specific lightningcss binary.
LIGHTNING_PKG=$(node -p "
  ({
    'linux-x64': 'lightningcss-linux-x64-gnu',
    'darwin-arm64': 'lightningcss-darwin-arm64',
    'darwin-x64': 'lightningcss-darwin-x64',
    'win32-x64': 'lightningcss-win32-x64-msvc',
    'win32-arm64': 'lightningcss-win32-arm64-msvc',
  })[process.platform+'-'+process.arch] || ''
")
if [ -n "$LIGHTNING_PKG" ]; then
  vx npm --prefix admin-ui install "$LIGHTNING_PKG" --no-save --ignore-scripts
fi

vx npm --prefix admin-ui run build
test -f crates/dcc-mcp-gateway/src/gateway/admin/generated/index.html

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "DCC_MCP_ADMIN_UI_PREBUILT=1" >> "$GITHUB_ENV"
fi
