#!/usr/bin/env sh
set -eu

VERSION="${DCC_MCP_VERSION:-latest}"
INSTALL_DIR="${DCC_MCP_INSTALL_DIR:-$HOME/.local/bin}"
OFFICIAL_RELEASES="https://github.com/dcc-mcp/dcc-mcp-cli/releases"

usage() {
    cat <<'EOF'
Install DCCMCP from the official GitHub release manifest.

Usage:
  install-cli.sh [--version v0.21.2] [--install-dir ~/.local/bin]

Environment:
  DCC_MCP_VERSION      Release version, default latest
  DCC_MCP_INSTALL_DIR  Install directory, default ~/.local/bin
EOF
}

fail() {
    echo "Error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || fail "--version requires a value"
            VERSION="$2"
            shift 2
            ;;
        --install-dir)
            [ "$#" -ge 2 ] || fail "--install-dir requires a value"
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
        [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "amd64" ] || fail "unsupported Linux architecture: $ARCH"
        PLATFORM="linux-x86_64"
        ASSET="dccmcp-linux-x86_64"
        ;;
    Darwin)
        PLATFORM="macos-universal2"
        ASSET="dccmcp-macos-universal2"
        ;;
    *)
        fail "unsupported OS: $OS"
        ;;
esac

REQUESTED_VERSION="${VERSION#v}"
case "$VERSION" in
    latest)
        MANIFEST_URL="$OFFICIAL_RELEASES/latest/download/dcc-mcp-update-manifest-$PLATFORM.json"
        ;;
    *)
        printf '%s' "$REQUESTED_VERSION" \
            | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$' \
            || fail "invalid release version: $VERSION"
        MANIFEST_URL="$OFFICIAL_RELEASES/download/v$REQUESTED_VERSION/dcc-mcp-update-manifest-$PLATFORM.json"
        ;;
esac

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

download_to() {
    url="$1"
    destination="$2"
    if command -v curl >/dev/null 2>&1; then
        curl --fail --location --proto '=https' --tlsv1.2 --proto-redir '=https' "$url" -o "$destination"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --secure-protocol=TLSv1_2 -O "$destination" "$url"
    else
        fail "curl or wget is required to download the release"
    fi
}

manifest_field() {
    entry="$1"
    field="$2"
    awk -v entry="$entry" -v field="$field" '
        $0 ~ "^[[:space:]]*\"" entry "\"[[:space:]]*:[[:space:]]*\\{" {
            in_entry = 1
            next
        }
        in_entry && /^[[:space:]]*}[,]?[[:space:]]*$/ { exit }
        in_entry && $0 ~ "^[[:space:]]*\"" field "\"[[:space:]]*:" {
            value = $0
            sub("^[^:]*:[[:space:]]*\"", "", value)
            sub("\"[[:space:]]*,?[[:space:]]*$", "", value)
            print value
            exit
        }
    ' "$MANIFEST_FILE"
}

calculate_sha256() {
    path="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$path" | awk '{print tolower($1)}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$path" | awk '{print tolower($1)}'
    elif command -v openssl >/dev/null 2>&1; then
        openssl dgst -sha256 "$path" | awk '{print tolower($NF)}'
    else
        fail "sha256sum, shasum, or openssl is required to verify the release"
    fi
}

MANIFEST_FILE="$TMP_DIR/manifest.json"
download_to "$MANIFEST_URL" "$MANIFEST_FILE" || fail "official release manifest download failed"

ENTRY="dccmcp"
ASSET_URL="$(manifest_field "$ENTRY" url)"
if [ -z "$ASSET_URL" ]; then
    ENTRY="dcc-mcp-cli"
    ASSET_URL="$(manifest_field "$ENTRY" url)"
fi
EXPECTED_SHA256="$(manifest_field "$ENTRY" sha256)"
[ -n "$ASSET_URL" ] || fail "official release manifest has no CLI asset"
[ -n "$EXPECTED_SHA256" ] || fail "official release manifest has no CLI SHA-256"
printf '%s' "$EXPECTED_SHA256" | grep -Eq '^[0-9a-fA-F]{64}$' \
    || fail "official release manifest contains an invalid SHA-256"
case "$ASSET_URL" in
    "https://github.com/dcc-mcp/dcc-mcp-cli/releases/download/"*"/$ASSET") ;;
    *) fail "official release manifest contains a non-official asset URL" ;;
esac

DOWNLOAD_FILE="$TMP_DIR/dccmcp"
download_to "$ASSET_URL" "$DOWNLOAD_FILE" || fail "CLI asset download failed"
ACTUAL_SHA256="$(calculate_sha256 "$DOWNLOAD_FILE")"
NORMALIZED_SHA256="$(printf '%s' "$EXPECTED_SHA256" | tr '[:upper:]' '[:lower:]')"
[ "$ACTUAL_SHA256" = "$NORMALIZED_SHA256" ] || fail "SHA-256 mismatch for the DCCMCP release"

mkdir -p "$INSTALL_DIR"
chmod 0755 "$DOWNLOAD_FILE"
mv -f "$DOWNLOAD_FILE" "$INSTALL_DIR/dccmcp"
cp -f "$INSTALL_DIR/dccmcp" "$INSTALL_DIR/dcc-mcp-cli"

echo "Installed dccmcp to $INSTALL_DIR/dccmcp"
echo "Installed compatibility alias to $INSTALL_DIR/dcc-mcp-cli"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "Add $INSTALL_DIR to PATH to run dccmcp from any shell." ;;
esac
