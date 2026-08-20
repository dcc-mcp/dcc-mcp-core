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
//! 3. `stage` — bind a verified component manifest to the exact installation
//! 4. On next launch, `apply_staged` — re-verify and atomically swap the binary
//!
//! Each step is independent so callers (CLI or server) can decide the UX.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

mod update_set;

const MAX_UPDATE_BYTES: u64 = 256 * 1024 * 1024;

pub use update_set::{
    UpdateSetSource, UpdateTarget, apply_staged_binary_update, apply_staged_update_set,
    clear_staged_binary_update, install_verified_sibling, stage_update_set,
    stage_verified_binary_update,
};

#[doc(hidden)]
pub use update_set::{
    apply_staged_binary_update_for, apply_staged_update_set_for, clear_staged_binary_update_for,
    stage_update_set_for, stage_verified_binary_update_for,
};

// ── Types ────────────────────────────────────────────────────────────────────

/// Response from the gateway's version-check endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    /// Whether a newer version is available.
    pub update_available: bool,
    /// The latest version string available.
    pub latest_version: String,
    /// URL to download the new binary; required when an update is available.
    pub download_url: Option<String>,
    /// SHA-256 hex digest; required when an update is available.
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

/// A validated, canonical lowercase SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Require and validate a manifest digest for one binary.
    pub fn require(binary_name: &str, value: Option<&str>) -> Result<Self, UpdateError> {
        let value = value.ok_or_else(|| UpdateError::MissingChecksum {
            binary_name: binary_name.to_owned(),
        })?;
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(UpdateError::InvalidChecksum {
                binary_name: binary_name.to_owned(),
            });
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Borrow the canonical lowercase digest.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the digest and return its canonical lowercase string.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// One downloaded update asset bound to its validated manifest digest.
#[derive(Debug)]
pub struct VerifiedUpdateAsset {
    path: PathBuf,
    sha256: Sha256Digest,
}

impl VerifiedUpdateAsset {
    /// Borrow the atomically persisted local asset path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the manifest digest verified for this asset.
    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    /// Consume the verified asset handle and return its local path.
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid gateway update response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Gateway update check failed ({status}): {error}: {message}")]
    Gateway {
        status: u16,
        error: String,
        message: String,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Update manifest entry for {binary_name} is missing a required SHA-256 digest")]
    MissingChecksum { binary_name: String },

    #[error("Update manifest entry for {binary_name} contains an invalid SHA-256 digest")]
    InvalidChecksum { binary_name: String },

    #[error("Update manifest entry for {binary_name} is missing a download URL")]
    MissingDownloadUrl { binary_name: String },

    #[error("Update binary name must be a portable filename")]
    InvalidBinaryName,

    #[error("Refused a legacy unsigned staged update; run update apply again")]
    LegacyUnsignedStage,

    #[error("Refused and quarantined an invalid staged update: {reason}")]
    RejectedStagedUpdate { reason: String },

    #[error("Update download exceeds the {max_bytes}-byte safety limit")]
    DownloadTooLarge { max_bytes: u64 },

    #[error("Update download is empty")]
    EmptyDownload,

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

/// Raw JSON payload returned by the gateway update endpoint.
#[derive(Debug, Clone)]
pub struct GatewayUpdateJson {
    pub status: u16,
    pub success: bool,
    pub body: Value,
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
        let payload = self.check_update_json().await?;
        self.update_info_from_payload(payload)
    }

    fn update_info_from_payload(
        &self,
        payload: GatewayUpdateJson,
    ) -> Result<UpdateInfo, UpdateError> {
        if !payload.success || payload.body.get("error").is_some() {
            return Err(gateway_error(payload.status, &payload.body));
        }

        let resp: UpdateCheckResponse = serde_json::from_value(payload.body)?;

        let (download_url, sha256) = if resp.update_available {
            let download_url = resp
                .download_url
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| UpdateError::MissingDownloadUrl {
                    binary_name: self.binary_name.clone(),
                })?;
            let sha256 = Sha256Digest::require(&self.binary_name, resp.sha256.as_deref())?;
            (Some(download_url), Some(sha256.into_string()))
        } else {
            (None, None)
        };

        Ok(UpdateInfo {
            update_available: resp.update_available,
            current_version: self.current_version.clone(),
            latest_version: resp.latest_version,
            download_url,
            sha256,
            release_notes: resp.release_notes,
        })
    }

    /// Query the gateway and preserve the raw JSON payload.
    ///
    /// This is useful for CLI `check` commands because gateway error responses
    /// are intentionally structured JSON and should be printable by agents.
    pub async fn check_update_json(&self) -> Result<GatewayUpdateJson, UpdateError> {
        let url = format!(
            "{}/v1/update/check?binary={}&current_version={}",
            self.gateway_url, self.binary_name, self.current_version
        );

        let response = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        let mut body: Value = response.json().await?;

        if let Value::Object(map) = &mut body {
            map.entry("current_version")
                .or_insert_with(|| Value::String(self.current_version.clone()));
            map.entry("binary_name")
                .or_insert_with(|| Value::String(self.binary_name.clone()));
        }

        let payload = GatewayUpdateJson {
            status: status.as_u16(),
            success: status.is_success(),
            body,
        };
        if payload.success && payload.body.get("error").is_none() {
            // CLI `update check` preserves successful gateway JSON, but it
            // must enforce the same integrity contract as `update apply`.
            self.update_info_from_payload(payload.clone())?;
        }
        Ok(payload)
    }

    /// Download the update binary to a temporary staging directory.
    ///
    /// Returns the path to the downloaded file.
    pub async fn download_update(&self, info: &UpdateInfo) -> Result<PathBuf, UpdateError> {
        Ok(self.download_verified_update(info).await?.into_path())
    }

    /// Download an update and bind the local asset to its manifest digest.
    pub async fn download_verified_update(
        &self,
        info: &UpdateInfo,
    ) -> Result<VerifiedUpdateAsset, UpdateError> {
        let expected_sha = Sha256Digest::require(&self.binary_name, info.sha256.as_deref())?;
        let download_url = info
            .download_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| UpdateError::MissingDownloadUrl {
                binary_name: self.binary_name.clone(),
            })?;

        let staging_dir = staging_dir(&self.binary_name)?;
        std::fs::create_dir_all(&staging_dir)?;

        // All downloads produce a raw binary (not an archive).
        // The manifest URL determines what we download — clients trust
        // the platform-appropriate URL configured in the manifest.
        let dest_path = staging_dir.join(format!("{}.download", self.binary_name));

        let response = self.client.get(download_url).send().await?;
        let mut response = response.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_UPDATE_BYTES)
        {
            return Err(UpdateError::DownloadTooLarge {
                max_bytes: MAX_UPDATE_BYTES,
            });
        }

        use std::io::Write as _;
        let mut temp = tempfile::NamedTempFile::new_in(&staging_dir)?;
        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        while let Some(chunk) = response.chunk().await? {
            total = total.saturating_add(chunk.len() as u64);
            if total > MAX_UPDATE_BYTES {
                return Err(UpdateError::DownloadTooLarge {
                    max_bytes: MAX_UPDATE_BYTES,
                });
            }
            hasher.update(&chunk);
            temp.write_all(&chunk)?;
        }
        if total == 0 {
            return Err(UpdateError::EmptyDownload);
        }
        temp.as_file().sync_all()?;

        let actual_sha = hex::encode(hasher.finalize().as_slice());
        if actual_sha != expected_sha.as_str() {
            return Err(UpdateError::ChecksumMismatch {
                expected: expected_sha.into_string(),
                actual: actual_sha,
            });
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut permissions = temp.as_file().metadata()?.permissions();
            permissions.set_mode(0o755);
            temp.as_file().set_permissions(permissions)?;
        }
        temp.persist(&dest_path)
            .map_err(|error| UpdateError::Io(error.error))?;
        Ok(VerifiedUpdateAsset {
            path: dest_path,
            sha256: expected_sha,
        })
    }

    /// Stage a downloaded update for replacement on the next launch.
    ///
    /// Compatibility entry point for trusted local files. Network update paths
    /// should use [`Updater::stage_verified_update`] with the manifest digest.
    /// On the next launch, the launcher should call [`apply_staged_update`].
    pub fn stage_update(downloaded: &Path, binary_name: &str) -> Result<(), UpdateError> {
        let actual_sha256 = sha256_file(downloaded)?;
        let expected_sha256 = Sha256Digest::require(binary_name, Some(&actual_sha256))?;
        Self::stage_verified_update(downloaded, binary_name, &expected_sha256)
    }

    /// Stage a manifest-verified update for replacement on the next launch.
    pub fn stage_verified_update(
        downloaded: &Path,
        binary_name: &str,
        expected_sha256: &Sha256Digest,
    ) -> Result<(), UpdateError> {
        stage_verified_binary_update(binary_name, downloaded, expected_sha256.as_str())
    }

    /// Apply a previously staged update by swapping the current binary.
    ///
    /// To be called at startup BEFORE the main application logic runs.
    /// Returns `true` if an update was applied, `false` if no update was staged.
    pub fn apply_staged_update(binary_name: &str) -> Result<bool, UpdateError> {
        if quarantine_legacy_staged_update(binary_name)? {
            return Err(UpdateError::LegacyUnsignedStage);
        }
        apply_staged_binary_update(binary_name)
    }

    /// Remove any staged update artifacts (rollback).
    pub fn clear_staged_update(binary_name: &str) -> Result<(), UpdateError> {
        clear_staged_binary_update(binary_name)?;
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

fn gateway_error(status: u16, body: &Value) -> UpdateError {
    let error = body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("update_check_failed")
        .to_string();
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Gateway update check failed.")
        .to_string();
    UpdateError::Gateway {
        status,
        error,
        message,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn staging_dir(binary_name: &str) -> Result<PathBuf, UpdateError> {
    validate_binary_name(binary_name)?;
    // Use a platform-appropriate data dir for staging updates
    // Falls back to a temp dir if we can't determine the data dir
    let base = dirs_data_dir().unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp"));
    Ok(base.join("update").join(binary_name))
}

fn quarantine_legacy_staged_update(binary_name: &str) -> Result<bool, UpdateError> {
    let dir = staging_dir(binary_name)?;
    let marker = dir.join("pending.marker");
    let binary = dir.join("pending.bin");
    if !marker.exists() && !binary.exists() {
        return Ok(false);
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| UpdateError::Stage(error.to_string()))?
        .as_nanos();
    let quarantine = dir.join(format!("legacy-unsigned-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<(), std::io::Error> {
        std::fs::create_dir_all(&quarantine)?;
        if marker.exists() {
            std::fs::rename(&marker, quarantine.join("pending.marker"))?;
        }
        if binary.exists() {
            std::fs::rename(&binary, quarantine.join("pending.bin"))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(%error, "failed to quarantine legacy unsigned staged update");
    } else {
        tracing::warn!(path = %quarantine.display(), "quarantined legacy unsigned staged update");
    }
    Ok(true)
}

fn validate_binary_name(binary_name: &str) -> Result<(), UpdateError> {
    let portable = binary_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if binary_name.is_empty()
        || !portable
        || binary_name.ends_with('.')
        || matches!(binary_name, "." | "..")
        || Path::new(binary_name)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(binary_name)
    {
        return Err(UpdateError::InvalidBinaryName);
    }
    Ok(())
}

/// Return the lowercase SHA-256 digest for a local file.
pub fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize().as_slice()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize().as_slice())
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
    { None::<PathBuf> }.map(|p| p.join("dcc-mcp"))
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_json_response(
        body: Value,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (addr, server)
    }

    async fn spawn_binary_response(
        body: &'static [u8],
        content_length: u64,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {content_length}\r\n\r\n"
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        (addr, server)
    }

    fn unique_binary_name(label: &str) -> String {
        format!(
            "dcc-mcp-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

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

    #[test]
    fn staging_dir_rejects_untrusted_binary_names() {
        for binary_name in ["", ".", "..", "../escape", "nested/name", "C:\\escape"] {
            let error = staging_dir(binary_name).unwrap_err();
            assert!(matches!(error, UpdateError::InvalidBinaryName));
        }
    }

    #[tokio::test]
    async fn download_update_rejects_http_error_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\ncontent-type: text/html\r\ncontent-length: 15\r\n\r\n<html>no</html>",
                )
                .await
                .unwrap();
        });

        let binary_name = unique_binary_name("http-error");
        let updater = Updater::new("http://127.0.0.1", &binary_name, "0.1.0");
        let info = UpdateInfo {
            update_available: true,
            current_version: "0.1.0".into(),
            latest_version: "0.2.0".into(),
            download_url: Some(format!("http://{addr}/missing")),
            sha256: Some("a".repeat(64)),
            release_notes: None,
        };

        let err = updater.download_update(&info).await.unwrap_err();
        assert!(matches!(err, UpdateError::Http(_)));
        assert!(
            !staging_dir(&binary_name)
                .unwrap()
                .join(format!("{binary_name}.download"))
                .exists()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn download_update_rejects_missing_sha256_before_network() {
        let updater = Updater::new("http://127.0.0.1", "dcc-mcp-cli", "0.1.0");
        let info = UpdateInfo {
            update_available: true,
            current_version: "0.1.0".into(),
            latest_version: "0.2.0".into(),
            download_url: Some("http://127.0.0.1:9/unreachable".into()),
            sha256: None,
            release_notes: None,
        };

        let error = updater.download_update(&info).await.unwrap_err();

        assert!(matches!(
            error,
            UpdateError::MissingChecksum { binary_name } if binary_name == "dcc-mcp-cli"
        ));
    }

    #[tokio::test]
    async fn download_update_streams_and_persists_verified_executable() {
        let body = b"verified-binary";
        let (addr, server) = spawn_binary_response(body, body.len() as u64).await;
        let binary_name = unique_binary_name("verified");
        let updater = Updater::new("http://127.0.0.1", &binary_name, "0.1.0");
        let info = UpdateInfo {
            update_available: true,
            current_version: "0.1.0".into(),
            latest_version: "0.2.0".into(),
            download_url: Some(format!("http://{addr}/binary")),
            sha256: Some(sha256_bytes(body)),
            release_notes: None,
        };

        let asset = updater.download_verified_update(&info).await.unwrap();

        assert_eq!(std::fs::read(asset.path()).unwrap(), body);
        assert_eq!(asset.sha256().as_str(), sha256_bytes(body));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(
                std::fs::metadata(asset.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        server.await.unwrap();
        std::fs::remove_dir_all(staging_dir(&binary_name).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn download_update_rejects_empty_and_oversized_assets() {
        for (label, content_length, expected) in [
            ("empty", 0, "empty"),
            ("oversized", MAX_UPDATE_BYTES + 1, "safety limit"),
        ] {
            let (addr, server) = spawn_binary_response(b"", content_length).await;
            let binary_name = unique_binary_name(label);
            let updater = Updater::new("http://127.0.0.1", &binary_name, "0.1.0");
            let info = UpdateInfo {
                update_available: true,
                current_version: "0.1.0".into(),
                latest_version: "0.2.0".into(),
                download_url: Some(format!("http://{addr}/binary")),
                sha256: Some(sha256_bytes(b"")),
                release_notes: None,
            };

            let error = updater.download_verified_update(&info).await.unwrap_err();

            assert!(error.to_string().contains(expected), "{label}: {error}");
            server.await.unwrap();
            let dir = staging_dir(&binary_name).unwrap();
            if dir.exists() {
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn check_update_rejects_manifest_without_sha256() {
        let (addr, server) = spawn_json_response(serde_json::json!({
            "update_available": true,
            "latest_version": "0.2.0",
            "download_url": "https://example.invalid/dcc-mcp-cli",
            "release_notes": null,
        }))
        .await;

        let updater = Updater::new(&format!("http://{addr}"), "dcc-mcp-cli", "0.1.0");
        let error = updater.check_update().await.unwrap_err();

        assert!(matches!(
            error,
            UpdateError::MissingChecksum { binary_name } if binary_name == "dcc-mcp-cli"
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn check_update_rejects_available_manifest_without_download_url() {
        let (addr, server) = spawn_json_response(serde_json::json!({
            "update_available": true,
            "latest_version": "0.2.0",
            "download_url": null,
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }))
        .await;
        let updater = Updater::new(&format!("http://{addr}"), "dcc-mcp-cli", "0.1.0");

        let error = updater.check_update().await.unwrap_err();

        assert!(matches!(error, UpdateError::MissingDownloadUrl { .. }));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn check_update_rejects_invalid_sha256() {
        let (addr, server) = spawn_json_response(serde_json::json!({
            "update_available": true,
            "latest_version": "0.2.0",
            "download_url": "https://example.invalid/dcc-mcp-cli",
            "sha256": "abc123",
            "release_notes": null,
        }))
        .await;

        let updater = Updater::new(&format!("http://{addr}"), "dcc-mcp-cli", "0.1.0");
        let error = updater.check_update().await.unwrap_err();

        assert!(error.to_string().contains("invalid SHA-256"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn raw_check_update_rejects_invalid_success_payloads() {
        let (addr, server) = spawn_json_response(serde_json::json!({
            "update_available": true,
            "latest_version": "0.2.0",
            "download_url": "https://example.invalid/dcc-mcp-cli",
            "sha256": null,
        }))
        .await;
        let updater = Updater::new(&format!("http://{addr}"), "dcc-mcp-cli", "0.1.0");

        let error = updater.check_update_json().await.unwrap_err();

        assert!(matches!(error, UpdateError::MissingChecksum { .. }));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn check_update_accepts_up_to_date_response_without_download_asset() {
        let (addr, server) = spawn_json_response(serde_json::json!({
            "update_available": false,
            "latest_version": "0.1.0",
            "download_url": "not-needed",
            "sha256": "not-needed",
            "release_notes": null,
        }))
        .await;

        let updater = Updater::new(&format!("http://{addr}"), "dcc-mcp-cli", "0.1.0");
        let info = updater.check_update().await.unwrap();

        assert!(!info.update_available);
        assert!(info.download_url.is_none());
        assert!(info.sha256.is_none());
        server.await.unwrap();
    }
}
