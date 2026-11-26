#!/usr/bin/env sh
set -eu

REPO="${DCC_MCP_REPO:-dcc-mcp/dcc-mcp-cli}"
VERSION="${DCC_MCP_VERSION:-latest}"
INSTALL_DIR="${DCC_MCP_INSTALL_DIR:-$HOME/.local/bin}"
RELEASE_FALLBACK="${DCC_MCP_RELEASE_FALLBACK:-1}"
ALLOW_UNSIGNED="${DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS:-0}"

usage() {
    cat <<'EOF'
Install DCCMCP from GitHub Releases.

Usage:
  install-cli.sh [--version v0.17.4] [--install-dir ~/.local/bin]

One-line install:
  curl -fsSL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-cli/main/scripts/install-cli.sh | bash

Environment:
  DCC_MCP_REPO         GitHub repo, default dcc-mcp/dcc-mcp-core
  DCC_MCP_VERSION      Release tag, default latest
  DCC_MCP_INSTALL_DIR  Install directory, default ~/.local/bin
  DCC_MCP_RELEASE_FALLBACK
                       When latest asset is missing, download the newest
                       release that includes the asset. Set to 0/false/no to
                       disable. Default enabled.
  DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS
                       Set to 1 only for a trusted legacy mirror that does
                       not publish .sha256 sidecars. Default disabled.
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
        ASSET="dccmcp-linux-x86_64"
        LEGACY_ASSET="dcc-mcp-cli-linux-x86_64"
        ;;
    Darwin)
        ASSET="dccmcp-macos-universal2"
        LEGACY_ASSET="dcc-mcp-cli-macos-universal2"
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

download_url() {
    url="$1"
    echo "Downloading $url"
    if command -v curl >/dev/null 2>&1; then
        curl --fail --location --proto '=https' --tlsv1.2 --proto-redir '=https' \
            "$url" -o "$TMP_DIR/dcc-mcp-cli"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --secure-protocol=TLSv1_2 -O "$TMP_DIR/dcc-mcp-cli" "$url"
    else
        echo "curl or wget is required to download release assets" >&2
        return 1
    fi
}

download_checksum() {
    url="$1.sha256"
    if command -v curl >/dev/null 2>&1; then
        curl --fail --location --proto '=https' --tlsv1.2 --proto-redir '=https' \
            "$url" -o "$TMP_DIR/dcc-mcp-cli.sha256"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --secure-protocol=TLSv1_2 -O "$TMP_DIR/dcc-mcp-cli.sha256" "$url"
    else
        return 1
    fi
}

verify_download() {
    url="$1"
    if ! download_checksum "$url"; then
        case "$ALLOW_UNSIGNED" in
            1|true|TRUE|yes|YES)
                echo "Warning: checksum sidecar unavailable; continuing by explicit opt-out." >&2
                return 0
                ;;
            *)
                echo "Checksum sidecar missing for $url. Set DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS=1 only for a trusted legacy mirror." >&2
                return 1
                ;;
        esac
    fi

    expected="$(awk 'NF { print tolower($1); exit }' "$TMP_DIR/dcc-mcp-cli.sha256")"
    if ! printf '%s\n' "$expected" | awk 'length($0) == 64 && $0 ~ /^[0-9a-fA-F]+$/ { exit 0 } { exit 1 }'; then
        echo "Invalid SHA-256 sidecar for $url" >&2
        return 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$TMP_DIR/dcc-mcp-cli" | awk '{print tolower($1)}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$TMP_DIR/dcc-mcp-cli" | awk '{print tolower($1)}')"
    else
        echo "sha256sum or shasum is required to verify the release" >&2
        return 1
    fi
    if [ "$actual" != "$expected" ]; then
        echo "SHA-256 mismatch for $url: expected $expected, got $actual" >&2
        return 1
    fi
}

release_fallback_enabled() {
    case "$RELEASE_FALLBACK" in
        0|false|FALSE|no|NO)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

latest_release_asset_url() {
    releases_json="$TMP_DIR/releases.json"
    api_url="https://api.github.com/repos/$REPO/releases?per_page=30"
    echo "Looking for the newest release containing $ASSET" >&2
    if command -v curl >/dev/null 2>&1; then
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            curl -fsSL \
                -H "Authorization: Bearer $GITHUB_TOKEN" \
                -H "X-GitHub-Api-Version: 2022-11-28" \
                "$api_url" -o "$releases_json"
        else
            curl -fsSL "$api_url" -o "$releases_json"
        fi
    elif command -v wget >/dev/null 2>&1; then
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            wget -q \
                --header="Authorization: Bearer $GITHUB_TOKEN" \
                --header="X-GitHub-Api-Version: 2022-11-28" \
                -O "$releases_json" "$api_url"
        else
            wget -q -O "$releases_json" "$api_url"
        fi
    else
        return 1
    fi
    tr ',' '\n' < "$releases_json" \
        | sed -n 's/.*"browser_download_url":[[:space:]]*"\([^"]*\/'"$ASSET"'\)".*/\1/p' \
        | head -n 1
}

try_download_asset() {
    ASSET="$1"
    if [ "$VERSION" = "latest" ]; then
        URL="https://github.com/$REPO/releases/latest/download/$ASSET"
    else
        URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
    fi

    if download_url "$URL"; then
        DOWNLOADED_URL="$URL"
        return 0
    fi
    if [ "$VERSION" = "latest" ] && release_fallback_enabled; then
        fallback_url="$(latest_release_asset_url || true)"
        if [ -n "$fallback_url" ]; then
            echo "Latest release did not provide $ASSET; falling back to $fallback_url"
            download_url "$fallback_url"
            DOWNLOADED_URL="$fallback_url"
            return 0
        fi
    fi
    return 1
}

mkdir -p "$INSTALL_DIR"
if ! try_download_asset "$ASSET"; then
    echo "DCCMCP asset unavailable, trying compatibility asset $LEGACY_ASSET"
    if ! try_download_asset "$LEGACY_ASSET"; then
        echo "No DCCMCP or dcc-mcp-cli release asset was found." >&2
        exit 1
    fi
fi

verify_download "$DOWNLOADED_URL"

chmod 0755 "$TMP_DIR/dcc-mcp-cli"
mv -f "$TMP_DIR/dcc-mcp-cli" "$INSTALL_DIR/dccmcp"
cp -f "$INSTALL_DIR/dccmcp" "$INSTALL_DIR/dcc-mcp-cli"

echo "Installed dccmcp to $INSTALL_DIR/dccmcp"
echo "Installed compatibility alias to $INSTALL_DIR/dcc-mcp-cli"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Add $INSTALL_DIR to PATH to run dccmcp from any shell."
        ;;
esac
