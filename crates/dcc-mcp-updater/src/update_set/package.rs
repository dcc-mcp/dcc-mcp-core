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
    /// Optional hash of a legacy foreground binary verified against an
    /// official, version/platform-bound manifest by the caller. Never derive
    /// this proof solely from the executable's self-reported runtime contract.
    pub verified_legacy_foreground: Option<PackageFile<'a>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageJournal {
    bound_executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transaction_id: Option<String>,
    created_directories: Vec<PathBuf>,
    #[cfg(unix)]
    original_modes: Vec<Option<u32>>,
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
            || part.to_ascii_lowercase().contains(".dcc-mcp-")
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
    install_package(
        binary_name,
        current_exe,
        identity,
        None,
        source_root,
        directories,
        files,
        |_| Ok(()),
        validate_installed,
    )
}

/// Exact installation bytes required under the shared installation lock.
#[derive(Debug, Clone, Copy)]
pub struct PackagePreconditions<'a> {
    pub executable_sha256: &'a str,
    pub foreground: PackageFile<'a>,
}

/// A package installation with mandatory byte admission and no implicit recovery.
#[derive(Debug, Clone, Copy)]
pub struct CheckedPackage<'a> {
    pub identity: PackageIdentity<'a>,
    pub preconditions: PackagePreconditions<'a>,
}

/// Install a qualified local package without recovering any pending transaction.
/// The anchor and foreground are checked under the same lock used for writes.
/// Runtime validation runs on the verified source after locked admission, then
/// on the installed tree before commit. Source failure makes no package writes.
pub fn install_verified_package_checked(
    binary_name: &str,
    current_exe: &Path,
    package: CheckedPackage<'_>,
    source_root: &Path,
    directories: &[String],
    files: &[PackageFile<'_>],
    validate_runtime: impl Fn(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    install_package(
        binary_name,
        current_exe,
        package.identity,
        Some(package.preconditions),
        source_root,
        directories,
        files,
        |root| validate_runtime(root),
        |root| validate_runtime(root),
    )
}

#[allow(clippy::too_many_arguments)]
fn install_package(
    binary_name: &str,
    current_exe: &Path,
    identity: PackageIdentity<'_>,
    preconditions: Option<PackagePreconditions<'_>>,
    source_root: &Path,
    directories: &[String],
    files: &[PackageFile<'_>],
    validate_source: impl FnOnce(&Path) -> Result<(), UpdateError>,
    validate_installed: impl FnOnce(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    validate_sibling_name(identity.name)?;
    if let Some(file) = identity.verified_legacy_foreground {
        validate_expected_sha(file.sha256)?;
        if file.path != identity.name && file.path != format!("{}.exe", identity.name) {
            return Err(UpdateError::Stage(
                "legacy package proof must name the foreground executable".into(),
            ));
        }
    }
    validate_package_paths(directories, files)?;
    let source_root = std::fs::canonicalize(source_root)?;
    for directory in directories {
        require_directory_chain(&source_root, Path::new(directory), false)?;
    }
    for file in files {
        check_parent_chain(&source_root, Path::new(file.path))?;
        require_file_hash(&source_root.join(file.path), file.sha256, "package source")?;
    }
    if preconditions.is_some() {
        // Reject a linked anchor before canonicalization can hide it.
        ensure_reserved_file_path(current_exe, "installation anchor")?;
    }
    let current_exe = canonical_existing(current_exe)?;
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| UpdateError::Stage("current executable has no parent".into()))?;
    let root = installation_staging_dir(binary_name, &current_exe)?;
    let _lock = acquire_installation_lock(&root, &current_exe, UPDATE_LOCK_TIMEOUT)?;
    let journal_path = root.join(format!("package-{}.json", identity.name));
    if let Some(expected) = preconditions {
        validate_expected_sha(expected.executable_sha256)?;
        validate_expected_sha(expected.foreground.sha256)?;
        if expected.foreground.path != identity.name
            && expected.foreground.path != format!("{}.exe", identity.name)
        {
            return Err(UpdateError::Stage(
                "precondition must name the package foreground".into(),
            ));
        }
        if !files
            .iter()
            .any(|file| file.path == expected.foreground.path)
        {
            return Err(UpdateError::Stage(
                "package plan must include the admitted foreground".into(),
            ));
        }
        if ensure_reserved_file_path(&journal_path, "package journal")? {
            return Err(UpdateError::Stage(
                "package recovery required before a fixed-byte installation".into(),
            ));
        }
        require_file_hash(
            &current_exe,
            expected.executable_sha256,
            "installation anchor",
        )?;
        check_parent_chain(install_dir, Path::new(expected.foreground.path))?;
        require_file_hash(
            &install_dir.join(expected.foreground.path),
            expected.foreground.sha256,
            "expected package foreground",
        )?;
    } else {
        recover_package(&journal_path, &current_exe)?;
    }
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
    let transaction_id = unique_nonce()?;
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
        let prepared = target.with_file_name(format!("{name}.dcc-mcp-{transaction_id}-new"));
        let backup = target.with_file_name(format!("{name}.dcc-mcp-{transaction_id}-backup"));
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
    // Fixed-byte entrypoints validate metadata only after locked admission and
    // ownership preflight, before the first installation journal or file write.
    // Source files may have changed while this caller waited for the lock.
    for directory in directories {
        require_directory_chain(&source_root, Path::new(directory), false)?;
    }
    for file in files {
        check_parent_chain(&source_root, Path::new(file.path))?;
        require_file_hash(
            &source_root.join(file.path),
            file.sha256,
            "locked package source",
        )?;
    }
    validate_source(&source_root)?;
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
        transaction_id: Some(transaction_id),
        created_directories: created_directories.into_iter().collect(),
        #[cfg(unix)]
        original_modes: entries
            .iter()
            .map(original_mode)
            .collect::<Result<_, _>>()?,
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
            let source = source_root.join(file.path);
            let mut input = File::open(&source)?;
            let mut output = create_package_prepared(&entry.prepared)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            drop(output);
            std::fs::set_permissions(&entry.prepared, std::fs::metadata(&source)?.permissions())?;
            require_file_hash(&entry.prepared, file.sha256, "package prepared file")?;
        }
        let receipt_entry = journal.update.entries.last().unwrap();
        {
            use std::io::Write;
            let mut output = create_package_prepared(&receipt_entry.prepared)?;
            output.write_all(&receipt)?;
            output.sync_all()?;
        }
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
        if let Err(recovery_error) = recover_package(&journal_path, &current_exe) {
            return Err(UpdateError::Stage(format!(
                "package commit persistence failed ({error}); recovery failed ({recovery_error})"
            )));
        }
        return Err(error);
    }
    recover_package(&journal_path, &current_exe).map_err(|error| {
        UpdateError::Stage(format!("package committed; cleanup required ({error})"))
    })
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
    for entry in &journal.update.entries {
        if ensure_reserved_file_path(&entry.prepared, "package prepared file")? {
            let hash = sha256_file(&entry.prepared)?;
            if !hash.eq_ignore_ascii_case(&entry.expected_sha256)
                && !entry
                    .original_sha256
                    .as_ref()
                    .is_some_and(|old| hash.eq_ignore_ascii_case(old))
            {
                return Err(UpdateError::Stage(
                    "unrecognized package prepared file preserved; recovery requires inspection"
                        .into(),
                ));
            }
        }
    }
    if journal.update.state == JournalState::Applying {
        rollback(&journal.update)?;
        #[cfg(unix)]
        restore_original_modes(&journal)?;
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

fn create_package_prepared(path: &Path) -> Result<File, UpdateError> {
    // Even a transaction-specific filename may have appeared after preflight.
    // Never truncate it, follow a link, or claim it merely because it is here.
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

fn validate_package_journal(
    journal: &PackageJournal,
    current_exe: &Path,
) -> Result<(), UpdateError> {
    if journal.transaction_id.as_ref().is_some_and(|id| {
        id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
    }) {
        return Err(UpdateError::Stage(
            "package journal has an invalid transaction identity".into(),
        ));
    }
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
    #[cfg(unix)]
    if journal.original_modes.len() != journal.update.entries.len()
        || journal
            .original_modes
            .iter()
            .zip(&journal.update.entries)
            .any(|(mode, entry)| {
                mode.is_some() != entry.had_original || mode.is_some_and(|mode| mode & !0o777 != 0)
            })
    {
        return Err(UpdateError::Stage(
            "package journal has invalid original permissions".into(),
        ));
    }
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
        let prefix = journal
            .transaction_id
            .as_ref()
            .map(|id| format!("{name}.dcc-mcp-{id}"))
            .unwrap_or_else(|| format!("{name}.dcc-mcp"));
        if paths_equal(&entry.target, current_exe)
            || entry.prepared != entry.target.with_file_name(format!("{prefix}-new"))
            || entry.backup != entry.target.with_file_name(format!("{prefix}-backup"))
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

#[cfg(unix)]
fn original_mode(entry: &JournalEntry) -> Result<Option<u32>, UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    if !entry.had_original {
        return Ok(None);
    }
    let mode = std::fs::metadata(&entry.target)?.permissions().mode();
    if mode & 0o7000 != 0 {
        return Err(UpdateError::Stage(
            "package destination has unsupported special permission bits".into(),
        ));
    }
    Ok(Some(mode & 0o777))
}

#[cfg(unix)]
fn restore_original_modes(journal: &PackageJournal) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    for (entry, mode) in journal.update.entries.iter().zip(&journal.original_modes) {
        if let Some(mode) = mode {
            require_file_hash(
                &entry.target,
                entry.original_sha256.as_deref().unwrap(),
                "restored package file",
            )?;
            std::fs::set_permissions(&entry.target, std::fs::Permissions::from_mode(*mode))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
