use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

#[cfg(test)]
pub(crate) fn path_digest(path: &Path, allow_symlink: bool) -> Result<Option<String>, String> {
    path_digest_with_policy(path, allow_symlink, allow_symlink, false)
}

pub(super) fn path_digest_with_policy(
    path: &Path,
    allow_nested_symlink: bool,
    allow_root_symlink: bool,
    exclude_git: bool,
) -> Result<Option<String>, String> {
    if let Err(error) = fs::symlink_metadata(path) {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!("failed to inspect owned path: {error}"));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"dcc-mcp-owned-path-v2\0");
    let root_is_symlink = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?
        .file_type()
        .is_symlink();
    if root_is_symlink && !allow_root_symlink {
        return Err("receipt ownership found a symlink root".into());
    }
    digest_path(
        path,
        Path::new(""),
        allow_nested_symlink || root_is_symlink,
        exclude_git,
        &mut hasher,
    )?;
    Ok(Some(digest_hex(hasher.finalize())))
}

pub(super) fn path_digest_for_receipt(
    path: &Path,
    allow_nested_symlink: bool,
    allow_root_symlink: bool,
    exclude_git: bool,
    digest_version: u8,
) -> Result<Option<String>, String> {
    if digest_version == 1 {
        return legacy_path_digest(path);
    }
    path_digest_with_policy(path, allow_nested_symlink, allow_root_symlink, exclude_git)
}

fn legacy_path_digest(path: &Path) -> Result<Option<String>, String> {
    if let Err(error) = fs::symlink_metadata(path) {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!("failed to inspect owned path: {error}"));
    }
    let mut hasher = Sha256::new();
    legacy_digest_path(path, path, &mut hasher)?;
    Ok(Some(digest_hex(hasher.finalize())))
}

fn legacy_digest_path(path: &Path, root: &Path, hasher: &mut Sha256) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("legacy receipt ownership found a symlink".into());
    }
    if metadata.is_file() {
        let relative = path.strip_prefix(root).unwrap_or(path);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(
            fs::read(path).map_err(|error| {
                format!("failed to read owned path for legacy receipt: {error}")
            })?,
        );
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err("legacy receipt ownership found an unsupported entry".into());
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(path).map_err(|error| format!("failed to enumerate owned path: {error}"))?
    {
        entries.push(
            entry
                .map_err(|error| format!("failed to read owned path entry: {error}"))?
                .path(),
        );
    }
    entries.sort();
    for entry in entries {
        let relative = entry.strip_prefix(root).unwrap_or(&entry);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0]);
        legacy_digest_path(&entry, root, hasher)?;
    }
    Ok(())
}

fn digest_hex<D>(digest: D) -> String
where
    D: AsRef<[u8]>,
{
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest_path(
    path: &Path,
    relative: &Path,
    allow_nested_symlink: bool,
    exclude_git: bool,
    hasher: &mut Sha256,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?;
    if metadata.file_type().is_symlink() {
        if !allow_nested_symlink {
            return Err("receipt ownership found a symlink".into());
        }
        let target = fs::read_link(path)
            .map_err(|error| format!("failed to read owned symlink: {error}"))?;
        hasher.update(b"symlink\0");
        update_digest_path(hasher, relative);
        update_digest_os_str(hasher, target.as_os_str());
        return Ok(());
    }
    if metadata.is_file() {
        hasher.update(b"file\0");
        update_digest_path(hasher, relative);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hasher.update(metadata.permissions().mode().to_le_bytes());
        }
        #[cfg(not(unix))]
        hasher.update(0_u32.to_le_bytes());
        stream_digest_file(path, metadata.len(), hasher)?;
        return Ok(());
    }
    if metadata.is_dir() {
        hasher.update(b"directory\0");
        update_digest_path(hasher, relative);
    } else {
        return Err("receipt ownership found an unsupported filesystem entry".into());
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(path).map_err(|error| format!("failed to enumerate owned path: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to read owned path entry: {error}"))?;
        entries.push(entry.path());
    }
    entries.sort_by(|left, right| {
        left.file_name()
            .unwrap_or_default()
            .cmp(right.file_name().unwrap_or_default())
    });
    for entry in entries {
        if exclude_git
            && relative.as_os_str().is_empty()
            && entry
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(".git"))
        {
            continue;
        }
        let child_relative = relative.join(entry.file_name().unwrap_or_default());
        digest_path(
            &entry,
            &child_relative,
            allow_nested_symlink,
            exclude_git,
            hasher,
        )?;
    }
    Ok(())
}

fn update_digest_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn update_digest_path(hasher: &mut Sha256, path: &Path) {
    for component in path.components() {
        hasher.update([match component {
            Component::CurDir => 0,
            Component::ParentDir => 1,
            Component::RootDir => 2,
            Component::Prefix(_) => 3,
            Component::Normal(_) => 4,
        }]);
        update_digest_os_str(hasher, component.as_os_str());
    }
    hasher.update([255]);
}

fn update_digest_os_str(hasher: &mut Sha256, value: &std::ffi::OsStr) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        update_digest_bytes(hasher, value.as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let bytes = value
            .encode_wide()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        update_digest_bytes(hasher, &bytes);
    }
    #[cfg(not(any(unix, windows)))]
    update_digest_bytes(hasher, value.to_string_lossy().as_bytes());
}

fn stream_digest_file(path: &Path, expected_len: u64, hasher: &mut Sha256) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open owned path for reading: {error}"))?;
    hasher.update(expected_len.to_le_bytes());
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read owned path: {error}"))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        hasher.update(&buffer[..count]);
    }
    if total != expected_len {
        return Err("owned path changed while its receipt digest was computed".into());
    }
    Ok(())
}

pub(super) fn replace_receipt_file(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        temporary
            .persist(destination)
            .map_err(|error| format!("failed to commit install receipt: {error}"))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        let temporary = temporary.into_temp_path();
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_TEMPORARY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, SetFileAttributesW,
        };
        let from = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let attributes = unsafe { GetFileAttributesW(from.as_ptr()) };
        if attributes != INVALID_FILE_ATTRIBUTES
            && attributes & FILE_ATTRIBUTE_TEMPORARY != 0
            && unsafe { SetFileAttributesW(from.as_ptr(), attributes & !FILE_ATTRIBUTE_TEMPORARY) }
                == 0
        {
            return Err(format!(
                "failed to prepare install receipt for commit: {}",
                std::io::Error::last_os_error()
            ));
        }
        let to = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(format!(
                "failed to commit install receipt: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}
