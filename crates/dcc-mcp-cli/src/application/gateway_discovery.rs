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
/// Filesystem inspection errors are returned; absence may fall through.
pub async fn resolve_gateway_bin(gateway_bin: Option<&PathBuf>) -> anyhow::Result<PathBuf> {
    // Layer 0: Explicit path provided by caller
    if let Some(explicit) = gateway_bin {
        if binary_is_file(explicit)? {
            return Ok(explicit.clone());
        }
        anyhow::bail!("explicit gateway binary not found: {}", explicit.display());
    }

    // Layer 1: Same-directory lookup
    if let Some(found) = find_in_same_dir()? {
        return Ok(found);
    }

    // Layer 2: Cached binary from a previous download
    if let Some(cached) = find_cached_binary()? {
        return Ok(cached);
    }

    // Layer 3: GitHub Releases download
    let version = env!("CARGO_PKG_VERSION");
    if let Ok(downloaded) = download_from_github_releases(version).await {
        return Ok(downloaded);
    }

    // Layer 4: Sub-command fallback (current executable)
    let current =
        std::env::current_exe().context("resolving current executable for sub-command fallback")?;
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
        match binary_is_file(explicit) {
            Err(error) => return inspection_error("explicit", Some(explicit.clone()), error),
            Ok(false) => {}
            Ok(true) => {
                return binary_diagnostic(
                    "ok",
                    "explicit",
                    Some(explicit.clone()),
                    Some(explicit.clone()),
                    false,
                );
            }
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

    match find_in_same_dir() {
        Ok(Some(found)) => return binary_diagnostic("ok", "same_dir", Some(found), None, false),
        Err(error) => return inspection_error("same_dir", None, error),
        Ok(None) => {}
    }

    match find_cached_binary() {
        Ok(Some(cached)) => return binary_diagnostic("ok", "cache", Some(cached), None, false),
        Err(error) => return inspection_error("cache", None, error),
        Ok(None) => {}
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

fn inspection_error(
    source: &'static str,
    configured_path: Option<PathBuf>,
    error: anyhow::Error,
) -> GatewayBinaryDiagnostic {
    GatewayBinaryDiagnostic {
        status: "unknown",
        source,
        path: None,
        configured_path,
        would_download_if_started: false,
        version: None,
        error: Some(
            format!("{error:#}")
                .chars()
                .filter(|c| !c.is_control())
                .take(2048)
                .collect(),
        ),
    }
}

fn binary_is_file(path: &std::path::Path) -> anyhow::Result<bool> {
    classify_binary_metadata(std::fs::metadata(path))
        .with_context(|| format!("inspecting gateway binary {}", path.display()))
}

fn classify_binary_metadata(metadata: std::io::Result<std::fs::Metadata>) -> std::io::Result<bool> {
    match metadata {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
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
fn find_in_same_dir() -> anyhow::Result<Option<PathBuf>> {
    let current_exe = crate::application::current_exe::current_exe()?;
    let exe_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let candidate = exe_dir.join(GATEWAY_BINARY_NAME_WITH_EXT);
    Ok(binary_is_file(&candidate)?.then_some(candidate))
}

// ── Layer 2: Cached binary ──────────────────────────────────────────────

/// Look for a previously downloaded gateway binary in the cache directory.
fn find_cached_binary() -> anyhow::Result<Option<PathBuf>> {
    let cache_dir = gateway_cache_dir().context("cannot determine gateway cache directory")?;
    let version = env!("CARGO_PKG_VERSION");
    let cached = cache_dir.join(version).join(GATEWAY_BINARY_NAME_WITH_EXT);
    Ok(binary_is_file(&cached)?.then_some(cached))
}

// ── Layer 3: GitHub Releases download ───────────────────────────────────

/// Download the gateway binary from GitHub Releases for the given version.
///
/// Writes to a temporary file first, then atomically renames to the final
/// cache location to avoid leaving a partial binary behind on crash/kill.
async fn download_from_github_releases(version: &str) -> anyhow::Result<PathBuf> {
    let target = build_release_target();
    let filename = if cfg!(windows) {
        format!("dcc-mcp-server-{target}.exe")
    } else {
        format!("dcc-mcp-server-{target}")
    };

    let base = std::env::var(ENV_GITHUB_RELEASE_BASE)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GITHUB_RELEASE_BASE.to_string());
    let url = format!("{base}/v{version}/{filename}");

    let cache_dir =
        gateway_cache_dir().context("cannot determine cache directory for gateway binary")?;
    let version_dir = cache_dir.join(version);
    std::fs::create_dir_all(&version_dir)
        .with_context(|| format!("creating cache directory {}", version_dir.display()))?;

    let dest = version_dir.join(GATEWAY_BINARY_NAME_WITH_EXT);

    // Download
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("building HTTP client for gateway download")?;

    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("downloading gateway binary from {url}"))?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("GitHub Releases returned HTTP {status} for {url}");
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading response body from {url}"))?;

    if bytes.is_empty() {
        anyhow::bail!("downloaded gateway binary from {url} is empty");
    }

    // Write to a temp file first, then rename atomically.
    let tmp = version_dir.join(format!(".{GATEWAY_BINARY_NAME}.tmp"));
    std::fs::write(&tmp, &bytes)
        .with_context(|| format!("writing downloaded binary to {}", tmp.display()))?;

    // Set executable permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp)
            .with_context(|| format!("reading metadata of {}", tmp.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp, perms)
            .with_context(|| format!("setting permissions on {}", tmp.display()))?;
    }

    std::fs::rename(&tmp, &dest)
        .with_context(|| format!("renaming {} to {}", tmp.display(), dest.display()))?;

    Ok(dest)
}

/// Build the Rust target triple for GitHub release asset naming.
///
/// Matches the naming convention used by the project's release workflow.
fn build_release_target() -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    };

    let os = if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else {
        // Best-effort: use the compile-time OS string directly.
        std::env::consts::OS
    };

    format!("{arch}-{os}")
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
    fn test_build_release_target_is_non_empty() {
        let target = build_release_target();
        assert!(!target.is_empty());
        assert!(
            target.contains('-'),
            "target should contain a dash: {target}"
        );
    }

    #[test]
    fn test_find_in_same_dir_returns_none_when_binary_missing() {
        // Our test binary is not named dcc-mcp-server, so this should
        // return None in test builds.
        let result = find_in_same_dir().unwrap();
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
    fn binary_metadata_distinguishes_missing_from_permission_and_io_errors() {
        use std::io::{Error, ErrorKind};
        assert!(!classify_binary_metadata(Err(Error::from(ErrorKind::NotFound))).unwrap());
        for kind in [ErrorKind::PermissionDenied, ErrorKind::Other] {
            assert_eq!(
                classify_binary_metadata(Err(Error::from(kind)))
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(!binary_is_file(dir.path()).unwrap());
        assert!(!binary_is_file(&dir.path().join("missing")).unwrap());
        let code = if cfg!(windows) { 5 } else { 13 };
        let error =
            anyhow::Error::new(Error::from_raw_os_error(code)).context("inspecting fixture binary");
        let diagnostic = inspection_error("explicit", None, error);
        assert_eq!(diagnostic.status, "unknown");
        assert!(!diagnostic.would_download_if_started);
        assert!(
            diagnostic
                .error
                .unwrap()
                .contains(&format!("os error {code}"))
        );
    }

    #[test]
    fn test_gateway_binary_name_has_correct_extension() {
        #[cfg(windows)]
        assert!(GATEWAY_BINARY_NAME_WITH_EXT.ends_with(".exe"));
        #[cfg(not(windows))]
        assert!(!GATEWAY_BINARY_NAME_WITH_EXT.ends_with(".exe"));
    }
}
