//! Gateway-controlled auto-update logic for dcc-mcp binaries.
//!
//! # Design
//!
//! This crate provides the core logic for checking, downloading, and staging
//! binary updates through the dcc-mcp gateway. It follows a **staged update**
//! pattern:
//!
//! 1. `check` — query the gateway for the latest version
//! 2. `download` — fetch the new binary to a temp staging directory
//! 3. `stage` — write a "pending update" marker so the next launch applies it
//! 4. On next launch, `apply_staged` — atomically swap the old binary
//!
//! Each step is independent so callers (CLI or server) can decide the UX.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

// ── Types ────────────────────────────────────────────────────────────────────

/// Response from the gateway's version-check endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    /// Whether a newer version is available.
    pub update_available: bool,
    /// The latest version string available.
    pub latest_version: String,
    /// URL to download the new binary archive.
    pub download_url: Option<String>,
    /// SHA-256 hex digest of the downloadable archive (optional).
    pub sha256: Option<String>,
    /// Human-readable release notes / changelog excerpt.
    pub release_notes: Option<String>,
}

/// Result of checking for an update — carries the current version for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub update_available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub download_url: Option<String>,
    pub sha256: Option<String>,
    pub release_notes: Option<String>,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Cannot determine current executable path")]
    NoExePath,

    #[error("Staging directory error: {0}")]
    Stage(String),
}

// ── Version helpers ──────────────────────────────────────────────────────────

/// Simple three-part semver comparison. Treats "0.18.16" etc. as comparable
/// triples. Non-numeric suffixes (pre-release tags) are ignored for comparison.
///
/// Parameter order matches the gateway's `is_newer_version(candidate, current)`
/// convention (see `dcc_mcp_gateway::gateway::version::is_newer_version`).
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    fn parse_segment(s: &str) -> u64 {
        s.split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0)
    }

    let can_parts: Vec<u64> = candidate.split('.').map(parse_segment).collect();
    let cur_parts: Vec<u64> = current.split('.').map(parse_segment).collect();

    for i in 0..3 {
        let n = can_parts.get(i).copied().unwrap_or(0);
        let c = cur_parts.get(i).copied().unwrap_or(0);
        if n > c {
            return true;
        }
        if n < c {
            return false;
        }
    }
    false
}

// ── Updater ───────────────────────────────────────────────────────────────────

/// The updater coordinates with the dcc-mcp gateway to check for and apply
/// binary updates.
pub struct Updater {
    gateway_url: String,
    binary_name: String,
    current_version: String,
    client: reqwest::Client,
}

impl Updater {
    /// Create a new updater instance.
    ///
    /// * `gateway_url` — base URL of the dcc-mcp gateway (e.g. `http://127.0.0.1:9765`)
    /// * `binary_name` — name of the binary to update (`dcc-mcp-cli` or `dcc-mcp-server`)
    /// * `current_version` — the currently installed version string
    pub fn new(gateway_url: &str, binary_name: &str, current_version: &str) -> Self {
        Self {
            gateway_url: gateway_url.trim_end_matches('/').to_string(),
            binary_name: binary_name.to_string(),
            current_version: current_version.to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest Client should build with default settings"),
        }
    }

    /// The binary name this updater was configured for.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }

    /// Query the gateway for available update information.
    ///
    /// Makes a `GET /v1/update/check?binary={binary_name}&current_version={ver}`
    /// request to the gateway.
    pub async fn check_update(&self) -> Result<UpdateInfo, UpdateError> {
        let url = format!(
            "{}/v1/update/check?binary={}&current_version={}",
            self.gateway_url, self.binary_name, self.current_version
        );

        let resp: UpdateCheckResponse = self.client.get(&url).send().await?.json().await?;

        Ok(UpdateInfo {
            update_available: resp.update_available,
            current_version: self.current_version.clone(),
            latest_version: resp.latest_version,
            download_url: resp.download_url,
            sha256: resp.sha256,
            release_notes: resp.release_notes,
        })
    }

    /// Download the update archive to a temporary staging directory.
    ///
    /// Returns the path to the downloaded file.
    pub async fn download_update(&self, info: &UpdateInfo) -> Result<PathBuf, UpdateError> {
        let download_url = info
            .download_url
            .as_deref()
            .ok_or_else(|| UpdateError::Stage("no download_url in update info".into()))?;

        let staging_dir = staging_dir(&self.binary_name)?;
        std::fs::create_dir_all(&staging_dir)?;

        // All downloads produce a raw binary (not an archive).
        // The manifest URL determines what we download — clients trust
        // the platform-appropriate URL configured in the manifest.
        let dest_path = staging_dir.join(format!("{}.download", self.binary_name));

        let response = self.client.get(download_url).send().await?;
        let bytes = response.bytes().await?;

        // Verify SHA-256 if provided
        if let Some(expected_sha) = &info.sha256 {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let actual_sha = hex::encode(hasher.finalize().as_slice());
            if !actual_sha.eq_ignore_ascii_case(expected_sha) {
                return Err(UpdateError::ChecksumMismatch {
                    expected: expected_sha.clone(),
                    actual: actual_sha,
                });
            }
        }

        tokio::fs::write(&dest_path, &bytes).await?;
        Ok(dest_path)
    }

    /// Stage a downloaded update for replacement on the next launch.
    ///
    /// Writes a pending-update marker and the new binary to the staging
    /// directory. On the next launch, the launcher should call
    /// [`apply_staged_update`].
    pub fn stage_update(downloaded: &Path, binary_name: &str) -> Result<(), UpdateError> {
        let dir = staging_dir(binary_name)?;
        std::fs::create_dir_all(&dir)?;

        let staged_bin = dir.join("pending.bin");
        std::fs::copy(downloaded, &staged_bin)?;

        // Write a marker so the next launch knows to apply the update
        let marker_path = dir.join("pending.marker");
        std::fs::write(&marker_path, "pending")?;

        tracing::info!("Update staged at {}", staged_bin.display());
        Ok(())
    }

    /// Apply a previously staged update by swapping the current binary.
    ///
    /// To be called at startup BEFORE the main application logic runs.
    /// Returns `true` if an update was applied, `false` if no update was staged.
    pub fn apply_staged_update(binary_name: &str) -> Result<bool, UpdateError> {
        let dir = staging_dir(binary_name)?;
        let marker_path = dir.join("pending.marker");
        let staged_bin = dir.join("pending.bin");

        if !marker_path.exists() || !staged_bin.exists() {
            return Ok(false);
        }

        let current_exe = std::env::current_exe().map_err(|_| UpdateError::NoExePath)?;

        tracing::info!(
            "Applying staged update: {} → {}",
            staged_bin.display(),
            current_exe.display()
        );

        // On Windows we can't replace a running exe, so we rename the current
        // binary aside and copy the new one in place.
        #[cfg(target_os = "windows")]
        {
            let backup = current_exe.with_extension("exe.bak");
            let _ = std::fs::remove_file(&backup);
            std::fs::rename(&current_exe, &backup)?;
            std::fs::copy(&staged_bin, &current_exe)?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Unix, rename(2) atomically replaces the target even if running
            // (the inode stays open in the running process; new exec gets the new inode).
            std::fs::rename(&staged_bin, &current_exe)?;
        }

        // Clean up markers
        let _ = std::fs::remove_file(&marker_path);

        tracing::info!("Staged update applied successfully");
        Ok(true)
    }

    /// Remove any staged update artifacts (rollback).
    pub fn clear_staged_update(binary_name: &str) -> Result<(), UpdateError> {
        let dir = staging_dir(binary_name)?;
        for entry in ["pending.bin", "pending.marker"] {
            let p = dir.join(entry);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
            }
        }
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn staging_dir(binary_name: &str) -> Result<PathBuf, UpdateError> {
    // Use a platform-appropriate data dir for staging updates
    // Falls back to a temp dir if we can't determine the data dir
    let base = dirs_data_dir().unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp"));
    Ok(base.join("update").join(binary_name))
}

fn dirs_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local").join("share"))
            })
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None::<PathBuf>
    }
    .map(|p| p.join("dcc-mcp"))
}

// ── hex is needed for SHA-256 display ────────────────────────────────────────
mod hex {
    pub(crate) fn encode(bytes: &[u8]) -> String {
        use std::fmt::Write;
        let mut hex = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            write!(hex, "{b:02x}").unwrap();
        }
        hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer_version("0.18.16", "0.18.15"));
        assert!(is_newer_version("0.19.0", "0.18.16"));
        assert!(!is_newer_version("0.18.16", "0.19.0"));
        assert!(!is_newer_version("0.18.16", "0.18.16"));
        // Pre-release tags are treated as equal to the base version (ignored suffix)
        assert!(!is_newer_version("0.18.16", "0.18.16-alpha"));
        assert!(!is_newer_version("0.18.16-alpha", "0.18.16"));
    }

    #[test]
    fn staging_dir_is_reasonable() {
        let dir = staging_dir("dcc-mcp-cli").unwrap();
        assert!(dir.to_string_lossy().contains("dcc-mcp"));
        assert!(dir.to_string_lossy().contains("update"));
    }
}
