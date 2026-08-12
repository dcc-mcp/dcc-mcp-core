//! Installation boundary for independently released companion executables.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use flate2::read::GzDecoder;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;

const OFFICIAL_RELEASES: &str = "https://github.com/dcc-mcp/dcc-cua/releases";
const COMPONENT_NAME: &str = "dcc-cua";
const MANIFEST_SCHEMA_VERSION: u64 = 1;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_ARCHIVE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallManifest {
    pub schema_version: u64,
    pub name: String,
    pub version: String,
    pub target: String,
    pub asset: InstallAsset,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallAsset {
    pub name: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct ComponentService {
    client: reqwest::Client,
    current_exe: PathBuf,
    target: &'static str,
}

impl ComponentService {
    pub fn for_current_process() -> anyhow::Result<Self> {
        Self::new(
            std::env::current_exe().context("cannot resolve the dcc-mcp-cli executable")?,
            current_target()?,
        )
    }

    pub fn new(current_exe: PathBuf, target: &'static str) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            current_exe,
            target,
        })
    }

    pub fn status(&self) -> anyhow::Result<Value> {
        let path = self.component_path()?;
        if !path.is_file() {
            return Ok(json!({
                "schema_version": 1,
                "component": COMPONENT_NAME,
                "status": "missing",
                "target": self.target,
                "path": path,
            }));
        }
        match validate_candidate(&path, None) {
            Ok(version) => Ok(json!({
                "schema_version": 1,
                "component": COMPONENT_NAME,
                "status": "ready",
                "target": self.target,
                "version": version,
                "path": path,
            })),
            Err(error) => Ok(json!({
                "schema_version": 1,
                "component": COMPONENT_NAME,
                "status": "incompatible",
                "target": self.target,
                "path": path,
                "message": error.to_string(),
            })),
        }
    }

    pub async fn ensure(&self, requested_version: Option<&str>) -> anyhow::Result<Value> {
        let manifest_url = manifest_url(self.target, requested_version)?;
        let manifest_bytes =
            download_bounded(&self.client, &manifest_url, MAX_MANIFEST_BYTES).await?;
        let manifest: InstallManifest = serde_json::from_slice(&manifest_bytes)
            .context("official dcc-cua install manifest is invalid JSON")?;
        validate_install_manifest(&manifest, self.target, requested_version)?;

        if let Ok(status) = self.status()
            && status["status"] == "ready"
            && status["version"] == manifest.version
        {
            return Ok(json!({
                "schema_version": 1,
                "component": COMPONENT_NAME,
                "status": "existing",
                "target": self.target,
                "version": manifest.version,
                "path": self.component_path()?,
            }));
        }

        let archive =
            download_bounded(&self.client, &manifest.asset.url, MAX_ARCHIVE_BYTES).await?;
        let transaction = TempDir::new().context("cannot create dcc-cua install transaction")?;
        let archive_path = transaction.path().join(&manifest.asset.name);
        std::fs::write(&archive_path, &archive)?;
        let actual_sha = dcc_mcp_updater::sha256_file(&archive_path)?;
        if actual_sha != manifest.asset.sha256 {
            bail!(
                "dcc-cua archive SHA-256 mismatch: expected {}, got {actual_sha}",
                manifest.asset.sha256
            );
        }

        let extracted = transaction.path().join("extracted");
        std::fs::create_dir(&extracted)?;
        extract_archive(&archive_path, &extracted)?;
        let candidate = extracted.join(component_file_name());
        let candidate_version = validate_candidate(&candidate, Some(&manifest.version))?;
        let candidate_sha = dcc_mcp_updater::sha256_file(&candidate)?;
        dcc_mcp_updater::install_verified_sibling(
            "dcc-mcp-cli",
            &self.current_exe,
            &candidate,
            component_file_name(),
            &candidate_sha,
        )?;
        let installed = self.component_path()?;
        validate_candidate(&installed, Some(&manifest.version))?;

        Ok(json!({
            "schema_version": 1,
            "component": COMPONENT_NAME,
            "status": "installed",
            "target": self.target,
            "version": candidate_version,
            "path": installed,
            "archive_sha256": manifest.asset.sha256,
        }))
    }

    fn component_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self
            .current_exe
            .parent()
            .context("dcc-mcp-cli executable has no parent directory")?
            .join(component_file_name()))
    }
}

pub fn manifest_url(target: &str, requested_version: Option<&str>) -> anyhow::Result<String> {
    validate_target(target)?;
    let file_name = format!("dcc-cua-install-manifest-{target}.json");
    match requested_version {
        Some(raw) => {
            let version = parse_stable_version(raw)?;
            Ok(format!(
                "{OFFICIAL_RELEASES}/download/v{version}/{file_name}"
            ))
        }
        None => Ok(format!("{OFFICIAL_RELEASES}/latest/download/{file_name}")),
    }
}

pub fn validate_install_manifest(
    manifest: &InstallManifest,
    expected_target: &str,
    requested_version: Option<&str>,
) -> anyhow::Result<()> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION || manifest.name != COMPONENT_NAME {
        bail!("official dcc-cua install manifest has an unsupported identity");
    }
    validate_target(expected_target)?;
    if manifest.target != expected_target {
        bail!("official dcc-cua install manifest target does not match this CLI");
    }
    let version = parse_stable_version(&manifest.version)?;
    if version.to_string() != manifest.version {
        bail!("official dcc-cua install manifest version is not canonical");
    }
    if let Some(requested) = requested_version
        && parse_stable_version(requested)? != version
    {
        bail!("official dcc-cua install manifest version does not match the requested version");
    }
    let extension = archive_extension(expected_target)?;
    let expected_name = format!("dcc-cua-{version}-{expected_target}.{extension}");
    if manifest.asset.name != expected_name {
        bail!("official dcc-cua install manifest asset name is invalid");
    }
    let expected_url = format!("{OFFICIAL_RELEASES}/download/v{version}/{expected_name}");
    if manifest.asset.url != expected_url {
        bail!("official dcc-cua install manifest contains a non-official asset URL");
    }
    if manifest.asset.sha256.len() != 64
        || !manifest
            .asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("official dcc-cua install manifest contains an invalid SHA-256");
    }
    Ok(())
}

fn parse_stable_version(raw: &str) -> anyhow::Result<Version> {
    let value = raw.strip_prefix('v').unwrap_or(raw);
    let version =
        Version::parse(value).context("dcc-cua version is not valid semantic versioning")?;
    if !version.pre.is_empty() {
        bail!("dcc-cua install manifests must reference a stable release");
    }
    Ok(version)
}

fn current_target() -> anyhow::Result<&'static str> {
    current_target_impl().context("dcc-cua companion installation is unsupported on this target")
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn current_target_impl() -> Option<&'static str> {
    Some("x86_64-pc-windows-msvc")
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn current_target_impl() -> Option<&'static str> {
    Some("x86_64-unknown-linux-gnu")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn current_target_impl() -> Option<&'static str> {
    Some("aarch64-apple-darwin")
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn current_target_impl() -> Option<&'static str> {
    Some("x86_64-apple-darwin")
}

#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64")
)))]
fn current_target_impl() -> Option<&'static str> {
    None
}

fn validate_target(target: &str) -> anyhow::Result<()> {
    if !matches!(
        target,
        "x86_64-pc-windows-msvc"
            | "x86_64-unknown-linux-gnu"
            | "aarch64-apple-darwin"
            | "x86_64-apple-darwin"
    ) {
        bail!("unsupported dcc-cua release target: {target}");
    }
    Ok(())
}

fn archive_extension(target: &str) -> anyhow::Result<&'static str> {
    validate_target(target)?;
    Ok(if target.ends_with("windows-msvc") {
        "zip"
    } else {
        "tar.gz"
    })
}

fn component_file_name() -> &'static str {
    if cfg!(windows) {
        "dcc-cua.exe"
    } else {
        "dcc-cua"
    }
}

async fn download_bounded(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/octet-stream, application/json",
        )
        .send()
        .await?
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        bail!("download exceeds the allowed size");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            bail!("download exceeds the allowed size");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn extract_archive(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    match archive.extension().and_then(OsStr::to_str) {
        Some("zip") => extract_zip(archive, destination),
        Some("gz") => extract_tar_gz(archive, destination),
        _ => bail!("unsupported dcc-cua archive format"),
    }
}

fn extract_zip(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        bail!("dcc-cua archive contains too many entries");
    }
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("dcc-cua archive contains an unsafe path")?
            .to_path_buf();
        validate_relative_path(&relative)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("dcc-cua archive contains a symbolic link");
        }
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES as u64 {
            bail!("dcc-cua archive expands beyond the allowed size");
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&output)?;
            std::io::copy(&mut entry, &mut file)?;
            file.flush()?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    let mut count = 0_usize;
    let mut total = 0_u64;
    for entry in tar.entries()? {
        let mut entry = entry?;
        count += 1;
        if count > MAX_ARCHIVE_ENTRIES {
            bail!("dcc-cua archive contains too many entries");
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            bail!("dcc-cua archive contains a link");
        }
        if !(entry_type.is_file() || entry_type.is_dir()) {
            bail!("dcc-cua archive contains an unsupported entry type");
        }
        let relative = entry.path()?.into_owned();
        validate_relative_path(&relative)?;
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES as u64 {
            bail!("dcc-cua archive expands beyond the allowed size");
        }
        let output = destination.join(relative);
        if entry_type.is_dir() {
            std::fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&output)?;
            std::io::copy(&mut entry, &mut file)?;
            file.flush()?;
        }
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> anyhow::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("dcc-cua archive contains an unsafe path");
    }
    Ok(())
}

fn validate_candidate(path: &Path, expected_version: Option<&str>) -> anyhow::Result<String> {
    if !path.is_file() {
        bail!("dcc-cua archive does not contain the expected executable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    let stdout = tempfile::NamedTempFile::new()?;
    let stdout_writer = stdout.reopen()?;
    let mut child = Command::new(path)
        .arg("manifest")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_writer))
        .stderr(Stdio::null())
        .spawn()
        .context("cannot execute the candidate dcc-cua manifest command")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("candidate dcc-cua manifest command timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = std::fs::read(stdout.path())?;
    if !status.success() || output.len() > MAX_MANIFEST_BYTES {
        bail!("candidate dcc-cua manifest command failed");
    }
    let manifest: Value = serde_json::from_slice(&output)
        .context("candidate dcc-cua returned invalid manifest JSON")?;
    if manifest["schema_version"] != 1
        || manifest["name"] != COMPONENT_NAME
        || manifest.pointer("/host/protocol_version") != Some(&json!(1))
        || manifest.pointer("/host/ensure_command/0") != Some(&json!("host-ensure"))
        || manifest.pointer("/core_bridge/command") != Some(&json!(["host-jsonl"]))
        || manifest.pointer("/runtime/separate_driver_required") != Some(&json!(false))
    {
        bail!("candidate dcc-cua runtime contract is incompatible with Core");
    }
    let version = manifest["version"]
        .as_str()
        .context("candidate dcc-cua manifest is missing its version")?;
    let parsed = parse_stable_version(version)?;
    if let Some(expected) = expected_version
        && parsed != parse_stable_version(expected)?
    {
        bail!("candidate dcc-cua version does not match the install manifest");
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(target: &str) -> InstallManifest {
        let version = "0.6.0";
        let extension = archive_extension(target).unwrap();
        let name = format!("dcc-cua-{version}-{target}.{extension}");
        InstallManifest {
            schema_version: 1,
            name: COMPONENT_NAME.into(),
            version: version.into(),
            target: target.into(),
            asset: InstallAsset {
                url: format!("{OFFICIAL_RELEASES}/download/v{version}/{name}"),
                name,
                sha256: "a".repeat(64),
            },
        }
    }

    #[test]
    fn versionless_and_pinned_manifest_urls_are_exact() {
        let target = "x86_64-pc-windows-msvc";
        assert_eq!(
            manifest_url(target, None).unwrap(),
            format!("{OFFICIAL_RELEASES}/latest/download/dcc-cua-install-manifest-{target}.json")
        );
        assert_eq!(
            manifest_url(target, Some("0.6.0")).unwrap(),
            format!("{OFFICIAL_RELEASES}/download/v0.6.0/dcc-cua-install-manifest-{target}.json")
        );
    }

    #[test]
    fn manifest_requires_exact_official_asset_binding_and_sha() {
        let target = "x86_64-pc-windows-msvc";
        let valid = manifest(target);
        validate_install_manifest(&valid, target, None).unwrap();

        let mut replaced = valid.clone();
        replaced.asset.url = "https://example.invalid/dcc-cua.zip".into();
        assert!(validate_install_manifest(&replaced, target, None).is_err());

        let mut uppercase_sha = valid;
        uppercase_sha.asset.sha256 = "A".repeat(64);
        assert!(validate_install_manifest(&uppercase_sha, target, None).is_err());
    }

    #[test]
    fn archive_paths_reject_parent_and_root_components() {
        assert!(validate_relative_path(Path::new("assets/profile.json")).is_ok());
        assert!(validate_relative_path(Path::new("../dcc-cua")).is_err());
        assert!(validate_relative_path(Path::new("/dcc-cua")).is_err());
    }
}
