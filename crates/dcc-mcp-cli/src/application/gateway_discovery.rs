//! Gateway binary discovery — three-layer strategy.
//!
//! 1. Same-directory lookup (alongside current executable)
//! 2. GitHub Releases download (with local cache)
//! 3. Sub-command fallback (current executable)
//!
//! The discovery chain stops at the first successful resolution.

use std::path::PathBuf;

use anyhow::Context;
use serde::Serialize;

#[cfg(test)]
mod bootstrap_tests;

/// Gateway binary filename, platform-aware.
const GATEWAY_BINARY_NAME: &str = "dcc-mcp-server";

#[cfg(windows)]
const GATEWAY_BINARY_NAME_WITH_EXT: &str = "dcc-mcp-server.exe";
#[cfg(not(windows))]
const GATEWAY_BINARY_NAME_WITH_EXT: &str = GATEWAY_BINARY_NAME;

/// Environment variable to override the GitHub Releases download base URL.
const ENV_GITHUB_RELEASE_BASE: &str = "DCC_MCP_GATEWAY_DOWNLOAD_BASE";

/// Default GitHub Releases base URL for gateway binary downloads.
const DEFAULT_GITHUB_RELEASE_BASE: &str =
    "https://github.com/dcc-mcp/dcc-mcp-core/releases/download";

/// Resolve the gateway binary using the full discovery chain:
///
/// 1. Explicit path (if provided by the caller)
/// 2. Same-directory lookup alongside the current executable
/// 3. Cached download from a previous GitHub Releases fetch
/// 4. GitHub Releases download (matching the current CLI version)
/// 5. Sub-command fallback (current executable)
///
/// Returns an error only when the explicit path is missing; all other
/// failures fall through to the next layer.
pub async fn resolve_gateway_bin(gateway_bin: Option<&PathBuf>) -> anyhow::Result<PathBuf> {
    // Layer 0: Explicit path provided by caller
    if let Some(explicit) = gateway_bin {
        if explicit.exists() {
            return Ok(explicit.clone());
        }
        anyhow::bail!("explicit gateway binary not found: {}", explicit.display());
    }

    // Layer 1: Same-directory lookup
    if let Some(found) = find_in_same_dir() {
        return Ok(found);
    }

    // Layer 2: Cached binary from a previous download
    if let Some(cached) = find_cached_binary() {
        return Ok(cached);
    }

    // Layer 3: GitHub Releases download
    let version = env!("CARGO_PKG_VERSION");
    let bootstrap_error = match download_from_github_releases(version).await {
        Ok(downloaded) => return Ok(downloaded),
        Err(error) => error,
    };

    // Layer 4: Sub-command fallback (current executable)
    let current =
        std::env::current_exe().context("resolving current executable for sub-command fallback")?;
    eprintln!(
        "warning: {}",
        bootstrap_fallback_message(&bootstrap_error, &current)
    );
    Ok(current)
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayBinaryDiagnostic {
    pub status: &'static str,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_path: Option<PathBuf>,
    pub would_download_if_started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Diagnose the gateway/server binary path without downloading or launching it.
pub fn diagnose_gateway_bin(gateway_bin: Option<&PathBuf>) -> GatewayBinaryDiagnostic {
    if let Some(explicit) = gateway_bin {
        if explicit.exists() {
            return binary_diagnostic(
                "ok",
                "explicit",
                Some(explicit.clone()),
                Some(explicit.clone()),
                false,
            );
        }
        return GatewayBinaryDiagnostic {
            status: "missing",
            source: "explicit",
            path: None,
            configured_path: Some(explicit.clone()),
            would_download_if_started: false,
            version: None,
            error: Some(format!(
                "explicit gateway binary not found: {}",
                explicit.display()
            )),
        };
    }

    if let Some(found) = find_in_same_dir() {
        return binary_diagnostic("ok", "same_dir", Some(found), None, false);
    }

    if let Some(cached) = find_cached_binary() {
        return binary_diagnostic("ok", "cache", Some(cached), None, false);
    }

    GatewayBinaryDiagnostic {
        status: "missing",
        source: "download_on_start",
        path: None,
        configured_path: None,
        would_download_if_started: true,
        version: None,
        error: Some(format!(
            "{GATEWAY_BINARY_NAME_WITH_EXT} was not found beside the CLI or in the gateway cache"
        )),
    }
}

fn binary_diagnostic(
    status: &'static str,
    source: &'static str,
    path: Option<PathBuf>,
    configured_path: Option<PathBuf>,
    would_download_if_started: bool,
) -> GatewayBinaryDiagnostic {
    let (version, error) = match path.as_ref() {
        Some(path) => probe_binary_version(path),
        None => (None, None),
    };
    GatewayBinaryDiagnostic {
        status,
        source,
        path,
        configured_path,
        would_download_if_started,
        version,
        error,
    }
}

fn probe_binary_version(path: &std::path::Path) -> (Option<String>, Option<String>) {
    match std::process::Command::new(path).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                return (Some(text), None);
            }
            let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !text.is_empty() {
                return (Some(text), None);
            }
            (
                None,
                Some("--version succeeded but produced no output".to_string()),
            )
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("--version exited with status {}", output.status)
            } else {
                stderr
            };
            (None, Some(message))
        }
        Err(err) => (None, Some(format!("running --version failed: {err}"))),
    }
}

// ── Layer 1: Same-directory lookup ──────────────────────────────────────

/// Look for `dcc-mcp-server` (or `.exe` on Windows) in the same directory
/// as the current executable.
///
/// The executable is resolved first so a symlinked launch (WinGet portable
/// exposes the CLI through `Microsoft\WinGet\Links`) looks beside the real
/// binary, which is where a sibling install would have put the server.
fn find_in_same_dir() -> Option<PathBuf> {
    let current_exe = crate::application::current_exe::current_exe().ok()?;
    let exe_dir = current_exe.parent()?;
    let candidate = exe_dir.join(GATEWAY_BINARY_NAME_WITH_EXT);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

// ── Layer 2: Cached binary ──────────────────────────────────────────────

/// Look for a previously downloaded gateway binary in the cache directory.
fn find_cached_binary() -> Option<PathBuf> {
    let cache_dir = gateway_cache_dir()?;
    let version = env!("CARGO_PKG_VERSION");
    let cached = cache_dir.join(version).join(GATEWAY_BINARY_NAME_WITH_EXT);
    if cached.is_file() { Some(cached) } else { None }
}

// ── Layer 3: GitHub Releases download ───────────────────────────────────

/// Download the gateway binary from GitHub Releases for the given version.
///
/// Writes to a temporary file first, then atomically renames to the final
/// cache location to avoid leaving a partial binary behind on crash/kill.
async fn download_from_github_releases(version: &str) -> anyhow::Result<PathBuf> {
    let base = std::env::var(ENV_GITHUB_RELEASE_BASE)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GITHUB_RELEASE_BASE.to_owned());
    let cache_dir = gateway_cache_dir().context("cannot determine gateway cache directory")?;
    download_release_binary(version, &base, &cache_dir).await
}

fn release_manifest_url(version: &str, base: &str) -> anyhow::Result<String> {
    semver::Version::parse(version).context("invalid gateway release version")?;
    let platform = dcc_mcp_gateway::release_platform()
        .context("no official gateway release for this platform")?;
    Ok(format!(
        "{}/v{version}/dcc-mcp-update-manifest-{platform}.json",
        base.trim_end_matches('/')
    ))
}

async fn download_release_binary(
    version: &str,
    base: &str,
    cache_dir: &std::path::Path,
) -> anyhow::Result<PathBuf> {
    let manifest_url = release_manifest_url(version, base)?;
    let info = dcc_mcp_gateway::resolve_release_update(&manifest_url, GATEWAY_BINARY_NAME, version)
        .await
        .context("resolving gateway bootstrap from release manifest")?;
    let version_dir = cache_dir.join(version);
    std::fs::create_dir_all(&version_dir).context("creating gateway cache version directory")?;
    let transaction =
        tempfile::tempdir_in(&version_dir).context("creating gateway bootstrap transaction")?;
    // The gateway is not running yet; only use the updater's verified download phase.
    let updater = dcc_mcp_updater::Updater::new("", GATEWAY_BINARY_NAME, version);
    let asset = updater
        .download_verified_update_in(&info, transaction.path())
        .await
        .context("downloading and verifying gateway bootstrap asset")?;
    let destination = version_dir.join(GATEWAY_BINARY_NAME_WITH_EXT);
    std::fs::rename(asset.path(), &destination).with_context(|| {
        format!(
            "committing verified gateway binary to {}",
            destination.display()
        )
    })?;
    Ok(destination)
}

fn bootstrap_fallback_message(error: &anyhow::Error, fallback: &std::path::Path) -> String {
    format!(
        "gateway bootstrap failed: {error:#}; using current CLI sub-command fallback {}",
        fallback.display()
    )
    .chars()
    .filter(|c| !c.is_control())
    .take(2048)
    .collect()
}
// ── Cache directory ─────────────────────────────────────────────────────

/// Return the cache directory for downloaded gateway binaries.
///
/// Uses the platform-standard cache directory:
/// - Windows: `%LOCALAPPDATA%\dcc-mcp\gateway`
/// - macOS:   `~/Library/Caches/dcc-mcp/gateway`
/// - Linux:   `~/.cache/dcc-mcp/gateway` (respects `$XDG_CACHE_HOME`)
fn gateway_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("dcc-mcp").join("gateway"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_cache_dir_is_absolute() {
        if let Some(dir) = gateway_cache_dir() {
            assert!(
                dir.is_absolute() || dir.to_string_lossy().starts_with('/'),
                "cache dir should be absolute: {}",
                dir.display()
            );
            assert!(
                dir.ends_with("gateway"),
                "cache dir should end with 'gateway': {}",
                dir.display()
            );
        }
    }

    #[test]
    fn release_manifest_uses_published_platform_names() {
        let url = release_manifest_url("0.20.46", DEFAULT_GITHUB_RELEASE_BASE).unwrap();
        assert!(url.contains("/v0.20.46/dcc-mcp-update-manifest-"));
        assert!(!url.contains("pc-windows-msvc"));
        assert!(!url.contains("unknown-linux-gnu"));
        assert!(release_manifest_url("../escape", DEFAULT_GITHUB_RELEASE_BASE).is_err());
    }

    #[test]
    fn test_find_in_same_dir_returns_none_when_binary_missing() {
        // Our test binary is not named dcc-mcp-server, so this should
        // return None in test builds.
        let result = find_in_same_dir();
        // Accept both None (binary not found) and Some (coincidental match
        // in an unusual test layout).
        if let Some(path) = &result {
            assert!(
                path.file_name()
                    .map(|n| n.to_string_lossy().contains("dcc-mcp-server"))
                    .unwrap_or(false),
                "unexpected found path: {}",
                path.display()
            );
        }
    }

    #[test]
    fn test_gateway_binary_name_has_correct_extension() {
        #[cfg(windows)]
        assert!(GATEWAY_BINARY_NAME_WITH_EXT.ends_with(".exe"));
        #[cfg(not(windows))]
        assert!(!GATEWAY_BINARY_NAME_WITH_EXT.ends_with(".exe"));
    }
}
