#!/usr/bin/env bash
# Probe DCC-MCP gateway health and instance registry; emit one-line JSON summary.
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$DIR/check_gateway.py"
