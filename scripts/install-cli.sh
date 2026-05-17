#!/usr/bin/env sh
set -eu

REPO="${DCC_MCP_REPO:-loonghao/dcc-mcp-core}"
VERSION="${DCC_MCP_VERSION:-latest}"
INSTALL_DIR="${DCC_MCP_INSTALL_DIR:-$HOME/.local/bin}"

usage() {
    cat <<'EOF'
Install dcc-mcp-cli from GitHub Releases.

Usage:
  install-cli.sh [--version v0.17.4] [--install-dir ~/.local/bin]

One-line install:
  curl -fsSL https://raw.githubusercontent.com/loonghao/dcc-mcp-core/main/scripts/install-cli.sh | bash

Environment:
  DCC_MCP_REPO         GitHub repo, default loonghao/dcc-mcp-core
  DCC_MCP_VERSION      Release tag, default latest
  DCC_MCP_INSTALL_DIR  Install directory, default ~/.local/bin
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            if [ "$#" -lt 2 ]; then
                echo "--version requires a value" >&2
                exit 2
            fi
            VERSION="$2"
            shift 2
            ;;
        --install-dir)
            if [ "$#" -lt 2 ]; then
                echo "--install-dir requires a value" >&2
                exit 2
            fi
            INSTALL_DIR="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        if [ "$ARCH" != "x86_64" ] && [ "$ARCH" != "amd64" ]; then
            echo "Unsupported Linux architecture: $ARCH" >&2
            exit 1
        fi
        ASSET="dcc-mcp-cli-linux-x86_64"
        ;;
    Darwin)
        ASSET="dcc-mcp-cli-macos-universal2"
        ;;
    *)
        echo "Unsupported OS: $OS" >&2
        exit 1
        ;;
esac

if [ "$VERSION" = "latest" ]; then
    URL="https://github.com/$REPO/releases/latest/download/$ASSET"
else
    URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

mkdir -p "$INSTALL_DIR"
echo "Downloading $URL"
if command -v curl >/dev/null 2>&1; then
    curl -fL "$URL" -o "$TMP_DIR/dcc-mcp-cli"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$TMP_DIR/dcc-mcp-cli" "$URL"
else
    echo "curl or wget is required" >&2
    exit 1
fi

chmod 0755 "$TMP_DIR/dcc-mcp-cli"
mv "$TMP_DIR/dcc-mcp-cli" "$INSTALL_DIR/dcc-mcp-cli"

echo "Installed dcc-mcp-cli to $INSTALL_DIR/dcc-mcp-cli"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Add $INSTALL_DIR to PATH to run dcc-mcp-cli from any shell."
        ;;
esac
