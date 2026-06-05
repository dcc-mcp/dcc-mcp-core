#!/usr/bin/env bash
set -euo pipefail

# Keep dcc-mcp-server release binaries free of OpenSSL-linked TLS stacks.
# The release job builds this crate on ubuntu-latest before packaging the
# PyPI binary wheel and GitHub Release assets, so this graph must stay aligned
# with the default Linux binary build.
target="${1:-x86_64-unknown-linux-gnu}"

check_absent() {
  local dep="$1"
  local output
  output="$(mktemp)"

  if cargo tree \
    -p dcc-mcp-server \
    -e normal \
    --target "$target" \
    -i "$dep" >"$output" 2>&1; then
    cat "$output"
    echo "::error::$dep is present in the dcc-mcp-server Linux release dependency graph"
    exit 1
  fi

  if ! grep -Eq "did not match any packages|nothing to print" "$output"; then
    cat "$output"
    echo "::error::cargo tree failed unexpectedly while checking $dep"
    exit 1
  fi

  echo "$dep is absent from the dcc-mcp-server Linux release dependency graph"
}

check_absent native-tls
check_absent openssl-sys
