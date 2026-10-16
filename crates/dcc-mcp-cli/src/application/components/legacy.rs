//! Hash-proven migration of foreground-only installs without executing them.

use std::future::Future;
use std::io::Read;

use super::*;

const MAX_LEGACY_BINARY_BYTES: u64 = 256 * 1024 * 1024;
const VERSION_BANNER: &[u8] = b"dcc-cua ";
const MAX_VERSION_BYTES: usize = 32;

pub(super) async fn verified_foreground_digest(
    client: &reqwest::Client,
    root: &Path,
    target: &str,
    requested: &InstallManifest,
) -> anyhow::Result<Option<String>> {
    verified_foreground_digest_with(root, target, requested, |url| async move {
        download_bounded(client, &url, MAX_MANIFEST_BYTES).await
    })
    .await
}

async fn verified_foreground_digest_with<F, Fut>(
    root: &Path,
    target: &str,
    requested: &InstallManifest,
    load_manifest: F,
) -> anyhow::Result<Option<String>>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<u8>>>,
{
    let receipt = root.join(".dcc-cua-install-receipt.json");
    match std::fs::symlink_metadata(&receipt) {
        // A receipt, including a malformed one or link, must be handled by the
        // normal ownership path. Legacy migration can never bypass it.
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let name = component_file_name();
    if !is_real_package_path(root, name, false)? {
        return Ok(None);
    }
    let path = root.join(name);
    if std::fs::symlink_metadata(&path)?.len() > MAX_LEGACY_BINARY_BYTES {
        bail!("legacy dcc-cua executable exceeds the scan limit; existing files were preserved");
    }
    let expected = requested
        .install
        .files
        .iter()
        .find(|file| file.path == name)
        .context("requested package does not declare its foreground executable")?;
    if dcc_mcp_updater::sha256_file(&path)? == expected.sha256 {
        return Ok(None);
    }
    // This is only an untrusted index into a fixed official source. The old
    // executable is never run to obtain its version or runtime contract.
    let version = read_version_hint(&path).context(
        "cannot identify the legacy dcc-cua release safely; preserve or relocate the conflicting executable before retrying",
    )?;
    let url = manifest_url(target, Some(&version))?;
    let bytes = load_manifest(url).await.context(
        "cannot obtain the official legacy dcc-cua manifest; existing files were preserved",
    )?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        bail!("official legacy dcc-cua manifest exceeds the size limit");
    }
    let manifest: InstallManifest = serde_json::from_slice(&bytes)
        .context("official legacy dcc-cua manifest is invalid JSON")?;
    validate_install_manifest(&manifest, target, Some(&version))?;
    let foreground = manifest
        .install
        .files
        .iter()
        .find(|file| file.path == name)
        .context("official legacy manifest does not declare the foreground executable")?;
    if !is_real_package_path(root, name, false)?
        || dcc_mcp_updater::sha256_file(&path)? != foreground.sha256
    {
        bail!(
            "legacy dcc-cua does not match its official release hash; existing files were preserved"
        );
    }
    // The updater checks this exact file/hash again under its installation lock.
    Ok(Some(foreground.sha256.clone()))
}

fn read_version_hint(path: &Path) -> anyhow::Result<String> {
    if std::fs::symlink_metadata(path)?.len() > MAX_LEGACY_BINARY_BYTES {
        bail!("legacy dcc-cua executable exceeds the scan limit");
    }
    let mut file = std::fs::File::open(path)?;
    let mut chunk = [0_u8; 8192];
    let mut tail = Vec::new();
    let mut versions = BTreeSet::new();
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut chunk)?;
        total = total.saturating_add(count as u64);
        if total > MAX_LEGACY_BINARY_BYTES {
            bail!("legacy dcc-cua executable exceeds the scan limit");
        }
        tail.extend_from_slice(&chunk[..count]);
        for index in 0..tail.len().saturating_sub(VERSION_BANNER.len() - 1) {
            if !tail[index..].starts_with(VERSION_BANNER) {
                continue;
            }
            let suffix = &tail[index + VERSION_BANNER.len()..];
            if !suffix.first().is_some_and(u8::is_ascii_digit) {
                continue;
            }
            let length = suffix
                .iter()
                .take_while(|byte| byte.is_ascii_digit() || **byte == b'.')
                .count();
            if length > MAX_VERSION_BYTES {
                bail!("legacy dcc-cua version banner exceeds the length limit");
            }
            if length == suffix.len() && count != 0 {
                continue; // Retain a split banner until its terminator or EOF.
            }
            if length == 0
                || suffix
                    .get(length)
                    .is_some_and(|byte| matches!(byte, b'-' | b'+'))
            {
                bail!("legacy dcc-cua version banner is not a stable version");
            }
            let text = std::str::from_utf8(&suffix[..length])?;
            let version = Version::parse(text)?;
            if !version.pre.is_empty() || !version.build.is_empty() || version.to_string() != text {
                bail!("legacy dcc-cua version banner is not canonical");
            }
            versions.insert(text.to_owned());
            if versions.len() > 1 {
                bail!("legacy dcc-cua version banners are ambiguous");
            }
        }
        if count == 0 {
            break;
        }
        let retain = VERSION_BANNER.len() + MAX_VERSION_BYTES + 1;
        if tail.len() > retain {
            tail.drain(..tail.len() - retain);
        }
    }
    versions
        .into_iter()
        .next()
        .context("legacy dcc-cua has no version banner")
}

#[cfg(test)]
mod tests;
