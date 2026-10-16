//! Synchronous, crash-recoverable installation of a verified companion package.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

mod ownership;
use ownership::{prepare_receipt, receipt_name};

const MAX_PACKAGE_ENTRIES: usize = 4096;
const MAX_PACKAGE_JOURNAL_BYTES: u64 = 4 * 1024 * 1024;

/// One manifest-declared file relative to the package and installation roots.
#[derive(Debug, Clone, Copy)]
pub struct PackageFile<'a> {
    pub path: &'a str,
    pub sha256: &'a str,
}

/// Identity of the official package represented by a verified install plan.
#[derive(Debug, Clone, Copy)]
pub struct PackageIdentity<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageJournal {
    bound_executable: PathBuf,
    created_directories: Vec<PathBuf>,
    update: UpdateJournal,
}

/// Reject ambiguous, non-portable, overlapping or escaping package paths.
///
/// Case folding is deliberate on every platform: the same manifest must remain
/// unambiguous on a case-insensitive Windows installation.
pub fn validate_package_paths(
    directories: &[String],
    files: &[PackageFile<'_>],
) -> Result<(), UpdateError> {
    if files.is_empty() || files.len().saturating_add(directories.len()) > MAX_PACKAGE_ENTRIES {
        return Err(UpdateError::Stage(
            "package install plan is empty or too large".into(),
        ));
    }
    let mut paths = BTreeMap::new();
    let mut spellings = BTreeMap::new();
    for (path, is_file) in directories
        .iter()
        .map(|path| (path.as_str(), false))
        .chain(files.iter().map(|file| (file.path, true)))
    {
        validate_package_path(path)?;
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if spellings
                .insert(prefix.to_ascii_lowercase(), prefix.clone())
                .is_some_and(|previous| previous != prefix)
            {
                return Err(UpdateError::Stage(
                    "package contains case-colliding directory names".into(),
                ));
            }
        }
        if paths.insert(path.to_ascii_lowercase(), is_file).is_some() {
            return Err(UpdateError::Stage(
                "package contains duplicate or case-colliding paths".into(),
            ));
        }
    }
    for path in paths.keys() {
        let mut parent = path.as_str();
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            if paths.get(prefix) == Some(&true) {
                return Err(UpdateError::Stage(
                    "package file conflicts with a parent directory".into(),
                ));
            }
            parent = prefix;
        }
    }
    for file in files {
        validate_expected_sha(file.sha256)?;
    }
    Ok(())
}

fn validate_package_path(path: &str) -> Result<(), UpdateError> {
    if path.len() > 512 || path.split('/').count() > 32 {
        return Err(UpdateError::Stage(
            "package path exceeds the size limit".into(),
        ));
    }
    for part in path.split('/') {
        validate_sibling_name(part)?;
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            || part.to_ascii_lowercase().ends_with(".dcc-mcp-new")
            || part.to_ascii_lowercase().ends_with(".dcc-mcp-backup")
        {
            return Err(UpdateError::Stage(
                "package contains a reserved filename".into(),
            ));
        }
    }
    Ok(())
}

/// Install only declared files, preserving all unrelated installation content.
///
/// All source hashes and destination paths are checked before preparing files.
/// The installation uses the same lock and verified file rollback machinery as
/// binary updates. The caller's final runtime validation is part of the
/// transaction; an error restores the complete previous package. File renames
/// are individually atomic, while the journal makes the whole set recoverable.
pub fn install_verified_package(
    binary_name: &str,
    current_exe: &Path,
    identity: PackageIdentity<'_>,
    source_root: &Path,
    directories: &[String],
    files: &[PackageFile<'_>],
    validate_installed: impl FnOnce(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    validate_sibling_name(identity.name)?;
    validate_package_paths(directories, files)?;
    let source_root = std::fs::canonicalize(source_root)?;
    for directory in directories {
        require_directory_chain(&source_root, Path::new(directory), false)?;
    }
    for file in files {
        check_parent_chain(&source_root, Path::new(file.path))?;
        require_file_hash(&source_root.join(file.path), file.sha256, "package source")?;
    }
    let current_exe = canonical_existing(current_exe)?;
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| UpdateError::Stage("current executable has no parent".into()))?;
    let root = installation_staging_dir(binary_name, &current_exe)?;
    let _lock = acquire_installation_lock(&root, &current_exe, UPDATE_LOCK_TIMEOUT)?;
    let journal_path = root.join(format!("package-{}.json", identity.name));
    recover_package(&journal_path, &current_exe)?;
    let receipt = prepare_receipt(install_dir, identity, directories, files)?;
    let mut created_directories = BTreeSet::new();
    for directory in directories {
        collect_missing_directories(install_dir, Path::new(directory), &mut created_directories)?;
    }
    let receipt_name = receipt_name(identity.name);
    let receipt_hash = sha256_bytes(&receipt);
    let receipt_file = PackageFile {
        path: &receipt_name,
        sha256: &receipt_hash,
    };
    let mut entries = Vec::new();
    for file in files.iter().chain(std::iter::once(&receipt_file)) {
        let relative = Path::new(file.path);
        if let Some(parent) = relative.parent() {
            collect_missing_directories(install_dir, parent, &mut created_directories)?;
        }
        let target = install_dir.join(relative);
        if paths_equal(&target, &current_exe) {
            return Err(UpdateError::Stage(
                "companion package cannot replace the running executable".into(),
            ));
        }
        let name = relative.file_name().and_then(|name| name.to_str()).unwrap();
        let prepared = target.with_file_name(format!("{name}.dcc-mcp-new"));
        let backup = target.with_file_name(format!("{name}.dcc-mcp-backup"));
        if ensure_reserved_file_path(&prepared, "package prepared path")?
            || ensure_reserved_file_path(&backup, "package backup path")?
        {
            return Err(UpdateError::Stage(
                "package has an unrecognized transaction sidecar".into(),
            ));
        }
        let had_original = ensure_reserved_file_path(&target, "package destination")?;
        let original_sha256 = had_original.then(|| sha256_file(&target)).transpose()?;
        entries.push(JournalEntry {
            target,
            prepared,
            backup,
            had_original,
            original_sha256,
            expected_sha256: file.sha256.to_ascii_lowercase(),
        });
    }
    if entries.len().saturating_add(created_directories.len()) > MAX_PACKAGE_ENTRIES {
        return Err(UpdateError::Stage(
            "expanded package install plan exceeds the size limit".into(),
        ));
    }
    // A complete installation still enters the shared lock and validates its
    // receipt. Avoid needless replacements after a successful previous ensure.
    if entries
        .iter()
        .all(|entry| entry.original_sha256.as_deref() == Some(entry.expected_sha256.as_str()))
        && created_directories.is_empty()
        && package_modes_match(&source_root, install_dir, files)?
    {
        return validate_installed(install_dir);
    }
    let mut journal = PackageJournal {
        bound_executable: current_exe.clone(),
        created_directories: created_directories.into_iter().collect(),
        update: UpdateJournal {
            state: JournalState::Applying,
            entries,
        },
    };
    validate_package_journal(&journal, &current_exe)?;
    // Persist original hashes and directory ownership before the first write
    // beneath the installation root, including preparation of nested paths.
    if serde_json::to_vec_pretty(&journal)?.len() as u64 > MAX_PACKAGE_JOURNAL_BYTES {
        return Err(UpdateError::Stage(
            "package journal exceeds the size limit".into(),
        ));
    }
    write_json_atomic(&journal_path, &journal)?;
    let result = (|| {
        for directory in &journal.created_directories {
            std::fs::create_dir(directory)?;
        }
        for (file, entry) in files.iter().zip(&journal.update.entries) {
            check_parent_chain(install_dir, entry.target.strip_prefix(install_dir).unwrap())?;
            prepare_verified_copy(
                &source_root.join(file.path),
                &entry.prepared,
                "package prepared file",
                file.sha256,
            )?;
        }
        let receipt_entry = journal.update.entries.last().unwrap();
        std::fs::write(&receipt_entry.prepared, &receipt)?;
        require_file_hash(
            &receipt_entry.prepared,
            &receipt_hash,
            "prepared package receipt",
        )?;
        apply_entries(&journal.update)?;
        validate_installed(install_dir)
    })();
    if let Err(error) = result {
        if let Err(recovery_error) = recover_package(&journal_path, &current_exe) {
            return Err(UpdateError::Stage(format!(
                "package install failed ({error}); recovery failed ({recovery_error})"
            )));
        }
        return Err(error);
    }
    journal.update.state = JournalState::Committed;
    if let Err(error) = write_json_atomic(&journal_path, &journal) {
        recover_package(&journal_path, &current_exe)?;
        return Err(error);
    }
    recover_package(&journal_path, &current_exe)
}

fn package_modes_match(
    source: &Path,
    target: &Path,
    files: &[PackageFile<'_>],
) -> Result<bool, UpdateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in files {
            let source_mode = std::fs::metadata(source.join(file.path))?
                .permissions()
                .mode();
            let target_mode = std::fs::metadata(target.join(file.path))?
                .permissions()
                .mode();
            if source_mode & 0o111 != target_mode & 0o111 {
                return Ok(false);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (source, target, files);
    Ok(true)
}

fn collect_missing_directories(
    root: &Path,
    relative: &Path,
    missing: &mut BTreeSet<PathBuf>,
) -> Result<(), UpdateError> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => (),
            Ok(_) => {
                return Err(UpdateError::Stage(format!(
                    "package directory is not a real directory: {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.insert(path.clone());
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn check_parent_chain(root: &Path, relative: &Path) -> Result<(), UpdateError> {
    if let Some(parent) = relative.parent() {
        require_directory_chain(root, parent, true)?;
    }
    Ok(())
}

fn require_directory_chain(
    root: &Path,
    relative: &Path,
    allow_missing: bool,
) -> Result<(), UpdateError> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => (),
            Ok(_) => {
                return Err(UpdateError::Stage(format!(
                    "package path contains a non-directory or symbolic link: {}",
                    path.display()
                )));
            }
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn recover_package(journal_path: &Path, current_exe: &Path) -> Result<(), UpdateError> {
    if !ensure_reserved_file_path(journal_path, "package journal")? {
        return Ok(());
    }
    if std::fs::metadata(journal_path)?.len() > MAX_PACKAGE_JOURNAL_BYTES {
        return Err(UpdateError::Stage(
            "package journal exceeds the size limit".into(),
        ));
    }
    let journal: PackageJournal = read_json(journal_path)?;
    validate_package_journal(&journal, current_exe)?;
    if journal.update.state == JournalState::Applying {
        rollback(&journal.update)?;
    } else {
        for entry in &journal.update.entries {
            require_file_hash(
                &entry.target,
                &entry.expected_sha256,
                "committed package file",
            )?;
        }
    }
    cleanup_committed_entries(&journal.update)?;
    if journal.update.state == JournalState::Applying {
        for directory in journal.created_directories.iter().rev() {
            match std::fs::remove_dir(directory) {
                Ok(()) => (),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    std::fs::remove_file(journal_path)?;
    Ok(())
}

fn validate_package_journal(
    journal: &PackageJournal,
    current_exe: &Path,
) -> Result<(), UpdateError> {
    if journal.bound_executable != current_exe
        || journal.update.entries.is_empty()
        || journal
            .update
            .entries
            .len()
            .saturating_add(journal.created_directories.len())
            > MAX_PACKAGE_ENTRIES
    {
        return Err(UpdateError::Stage(
            "package journal does not match this installation".into(),
        ));
    }
    let root = current_exe.parent().unwrap();
    let relative = |path: &Path| -> Result<String, UpdateError> {
        let path = path.strip_prefix(root).map_err(|_| {
            UpdateError::Stage("package journal path escapes the installation".into())
        })?;
        let text = path
            .components()
            .map(|part| match part {
                std::path::Component::Normal(value) => value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| UpdateError::Stage("package journal path is not UTF-8".into())),
                _ => Err(UpdateError::Stage(
                    "package journal path is not canonical".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        validate_package_path(&text)?;
        if Path::new(&text) != path {
            return Err(UpdateError::Stage(
                "package journal path is not canonical".into(),
            ));
        }
        check_parent_chain(root, path)?;
        Ok(text)
    };
    let directories = journal
        .created_directories
        .iter()
        .map(|path| relative(path))
        .collect::<Result<Vec<_>, _>>()?;
    for directory in &directories {
        require_directory_chain(root, Path::new(directory), true)?;
    }
    let paths = journal
        .update
        .entries
        .iter()
        .map(|entry| relative(&entry.target))
        .collect::<Result<Vec<_>, _>>()?;
    let files = paths
        .iter()
        .zip(&journal.update.entries)
        .map(|(path, entry)| PackageFile {
            path,
            sha256: &entry.expected_sha256,
        })
        .collect::<Vec<_>>();
    validate_package_paths(&directories, &files)?;
    for entry in &journal.update.entries {
        let name = entry
            .target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap();
        if paths_equal(&entry.target, current_exe)
            || entry.prepared != entry.target.with_file_name(format!("{name}.dcc-mcp-new"))
            || entry.backup
                != entry
                    .target
                    .with_file_name(format!("{name}.dcc-mcp-backup"))
            || entry.had_original != entry.original_sha256.is_some()
        {
            return Err(UpdateError::Stage(
                "package journal has an invalid target or sidecar".into(),
            ));
        }
        if let Some(hash) = &entry.original_sha256 {
            validate_expected_sha(hash)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
