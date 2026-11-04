//! Offline installation of externally qualified, explicitly pinned packages.

use super::*;
use sha2::Digest;

#[derive(Debug)]
pub struct VerifiedInstallRequest {
    pub target_core_cli: PathBuf,
    pub expected_core_cli_sha256: String,
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub previous_manifest: PathBuf,
    pub previous_manifest_sha256: String,
    pub source_root: PathBuf,
    pub version: String,
    pub expected_current_sha256: String,
    pub expected_new_sha256: String,
}

/// Never executes or replaces the installation anchor, downloads a release,
/// starts a host, or recovers another transaction. Manifest digests must come
/// from an independently verified official publication, not from this command.
pub fn install(request: VerifiedInstallRequest) -> anyhow::Result<Value> {
    let target = current_target()?;
    let manifest = read_manifest(&request.manifest, &request.manifest_sha256)?;
    validate_install_manifest(&manifest, target, Some(&request.version))?;
    let previous = read_manifest(
        &request.previous_manifest,
        &request.previous_manifest_sha256,
    )?;
    validate_install_manifest(&previous, target, None)?;
    for (plan, expected) in [
        (&manifest.install, request.expected_new_sha256.as_str()),
        (&previous.install, request.expected_current_sha256.as_str()),
    ] {
        validate_sha256(expected, "pinned foreground")?;
        if !plan
            .files
            .iter()
            .any(|file| file.path == component_file_name() && file.sha256 == expected)
        {
            bail!("pinned foreground does not match its qualified install manifest");
        }
    }
    validate_sha256(&request.expected_core_cli_sha256, "installation anchor")?;
    let anchor = canonical_regular_path(&request.target_core_cli, false)?;
    let expected_anchor_name = if cfg!(windows) {
        "dcc-mcp-cli.exe"
    } else {
        "dcc-mcp-cli"
    };
    if anchor.file_name().and_then(OsStr::to_str) != Some(expected_anchor_name) {
        bail!("installation anchor must be the canonical dcc-mcp-cli executable");
    }
    let install_root = anchor
        .parent()
        .context("installation anchor has no parent")?;
    let source = canonical_regular_path(&request.source_root, true)?;
    if source.starts_with(install_root) || install_root.starts_with(&source) {
        bail!("qualified source and installation directories must not overlap");
    }
    // Verify every declared file before executing even a metadata command.
    verify_extracted_install_plan(&source, &manifest.install)?;
    let staged = stage_local_package(&source, &manifest.install)?;
    require_digest(&anchor, &request.expected_core_cli_sha256)?;
    require_digest(
        &install_root.join(component_file_name()),
        &request.expected_current_sha256,
    )?;
    let mut files = package_files(&manifest.install);
    files.sort_by_key(|file| file.path == component_file_name());
    let old = dcc_mcp_updater::PackageFile {
        path: component_file_name(),
        sha256: &request.expected_current_sha256,
    };
    dcc_mcp_updater::install_verified_package_checked(
        "dcc-mcp-cli",
        &anchor,
        dcc_mcp_updater::CheckedPackage {
            identity: dcc_mcp_updater::PackageIdentity {
                name: COMPONENT_NAME,
                version: &manifest.version,
                verified_legacy_foreground: Some(old),
            },
            preconditions: dcc_mcp_updater::PackagePreconditions {
                executable_sha256: &request.expected_core_cli_sha256,
                foreground: old,
            },
        },
        staged.path(),
        &manifest.install.directories,
        &files,
        |root| {
            validate_candidate(&root.join(component_file_name()), Some(&manifest.version))
                .map(|_| ())
                .map_err(|error| dcc_mcp_updater::UpdateError::Stage(error.to_string()))
        },
    )?;
    Ok(json!({
        "schema_version": 1, "component": COMPONENT_NAME,
        "status": "installed", "version": manifest.version, "target": target,
        "path": install_root.join(component_file_name()),
        "manifest_sha256": request.manifest_sha256,
        "foreground_sha256": request.expected_new_sha256,
        "installation_anchor_sha256": request.expected_core_cli_sha256,
        "loaded_callers_restarted": false,
    }))
}

fn stage_local_package(source: &Path, plan: &InstallPlan) -> anyhow::Result<TempDir> {
    // Execute metadata only from a private, fully verified tree. Unlisted
    // siblings in the caller's source cannot become DLLs or runtime resources.
    let staged = TempDir::new().context("cannot stage qualified local package")?;
    for directory in &plan.directories {
        std::fs::create_dir_all(staged.path().join(directory))?;
    }
    let mut total = 0_u64;
    for file in &plan.files {
        let source_file = source.join(&file.path);
        let target = staged.path().join(&file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Read;
        let mut input =
            std::fs::File::open(source_file)?.take(MAX_ARCHIVE_BYTES as u64 - total + 1);
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        total += std::io::copy(&mut input, &mut output)?;
        if total > MAX_ARCHIVE_BYTES as u64 {
            bail!("qualified local package exceeds the size limit");
        }
    }
    verify_extracted_install_plan(staged.path(), plan)?;
    prepare_package_executables(staged.path(), plan)?;
    Ok(staged)
}

fn read_manifest(path: &Path, expected: &str) -> anyhow::Result<InstallManifest> {
    validate_sha256(expected, "manifest")?;
    let path = canonical_regular_path(path, false)?;
    let mut bytes = Vec::new();
    use std::io::Read;
    std::fs::File::open(path)?
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        bail!("qualified install manifest exceeds the size limit");
    }
    let actual: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        bail!("qualified install manifest SHA-256 mismatch");
    }
    serde_json::from_slice(&bytes).context("qualified install manifest is invalid JSON")
}

fn canonical_regular_path(path: &Path, directory: bool) -> anyhow::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(path)?;
    if (directory && !metadata.file_type().is_dir())
        || (!directory && !metadata.file_type().is_file())
    {
        bail!("qualified installation paths must not be symbolic links");
    }
    Ok(std::fs::canonicalize(path)?)
}

fn require_digest(path: &Path, expected: &str) -> anyhow::Result<()> {
    canonical_regular_path(path, false)?;
    if dcc_mcp_updater::sha256_file(path)? != expected {
        bail!("fixed-byte installation precondition changed");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
