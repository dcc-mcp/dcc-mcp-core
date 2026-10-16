//! Ownership receipts prevent companion resources from replacing user content.

use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u8,
    name: String,
    version: String,
    directories: Vec<String>,
    files: Vec<OwnedFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedFile {
    path: String,
    sha256: String,
}

pub(super) fn receipt_name(name: &str) -> String {
    format!(".{name}-install-receipt.json")
}

pub(super) fn prepare_receipt(
    root: &Path,
    identity: PackageIdentity<'_>,
    directories: &[String],
    files: &[PackageFile<'_>],
) -> Result<Vec<u8>, UpdateError> {
    validate_version(identity.version)?;
    let name = receipt_name(identity.name);
    if directories
        .iter()
        .any(|path| path.eq_ignore_ascii_case(&name))
        || files
            .iter()
            .any(|file| file.path.eq_ignore_ascii_case(&name))
    {
        return Err(UpdateError::Stage(
            "package manifest conflicts with its ownership receipt".into(),
        ));
    }
    let receipt_path = root.join(&name);
    let mut owned = BTreeMap::new();
    let has_receipt = ensure_reserved_file_path(&receipt_path, "package ownership receipt")?;
    if has_receipt {
        if std::fs::metadata(&receipt_path)?.len() > MAX_PACKAGE_JOURNAL_BYTES {
            return Err(UpdateError::Stage(
                "package ownership receipt exceeds the size limit".into(),
            ));
        }
        let receipt: Receipt = read_json(&receipt_path)?;
        if receipt.schema_version != 1 || receipt.name != identity.name {
            return Err(UpdateError::Stage(
                "package ownership receipt has an incompatible identity".into(),
            ));
        }
        validate_version(&receipt.version)?;
        let old_files = receipt
            .files
            .iter()
            .map(|file| PackageFile {
                path: &file.path,
                sha256: &file.sha256,
            })
            .collect::<Vec<_>>();
        validate_package_paths(&receipt.directories, &old_files)?;
        for file in receipt.files {
            if file.path.eq_ignore_ascii_case(&name) {
                return Err(UpdateError::Stage(
                    "package ownership receipt cannot own itself".into(),
                ));
            }
            owned.insert(file.path.to_ascii_lowercase(), file.sha256);
        }
    }
    // Legacy installs have no receipt. Adopt only bytes that already match the
    // requested official manifest. Only the exact foreground may additionally
    // match a caller-verified older official manifest, rechecked under the lock.
    // A modified previously owned resource is also a conflict, not consent to
    // discard user changes. Missing files can still be repaired automatically.
    for file in files {
        check_parent_chain(root, Path::new(file.path))?;
        let target = root.join(file.path);
        if ensure_reserved_file_path(&target, "package destination")? {
            let actual = sha256_file(&target)?;
            let verified_legacy_foreground = !has_receipt
                && identity.verified_legacy_foreground.is_some_and(|legacy| {
                    file.path == legacy.path && actual.eq_ignore_ascii_case(legacy.sha256)
                });
            if !actual.eq_ignore_ascii_case(file.sha256)
                && !verified_legacy_foreground
                && !owned
                    .get(&file.path.to_ascii_lowercase())
                    .is_some_and(|hash| actual.eq_ignore_ascii_case(hash))
            {
                return Err(UpdateError::Stage(format!(
                    "package file conflict at {}: existing content is unowned or modified; preserve it before retrying",
                    file.path
                )));
            }
        }
    }
    let receipt = Receipt {
        schema_version: 1,
        name: identity.name.into(),
        version: identity.version.into(),
        directories: directories.to_vec(),
        files: files
            .iter()
            .map(|file| OwnedFile {
                path: file.path.into(),
                sha256: file.sha256.to_ascii_lowercase(),
            })
            .collect(),
    };
    Ok(serde_json::to_vec_pretty(&receipt)?)
}

fn validate_version(version: &str) -> Result<(), UpdateError> {
    if version.is_empty()
        || version.len() > 128
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(UpdateError::Stage(
            "package ownership receipt has an invalid version".into(),
        ));
    }
    Ok(())
}
