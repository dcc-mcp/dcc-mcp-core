//! Package prepared files require persisted creation provenance, not just bytes.

use super::*;

pub(super) fn persist(path: &Path, journal: &PackageJournal) -> Result<(), UpdateError> {
    if serde_json::to_vec_pretty(journal)?.len() as u64 > MAX_PACKAGE_JOURNAL_BYTES {
        return Err(UpdateError::Stage(
            "package journal exceeds the size limit".into(),
        ));
    }
    write_json_atomic(path, journal)
}

pub(super) fn create_owned(
    journal_path: &Path,
    journal: &mut PackageJournal,
    index: usize,
) -> Result<File, UpdateError> {
    let prepared = journal.update.entries[index].prepared.clone();
    let file = create_package_prepared(&prepared)?;
    // A crash before this persist leaves an unknown file, which recovery preserves.
    journal.created_prepared.insert(prepared);
    persist(journal_path, journal)?;
    Ok(file)
}

pub(super) fn relinquish(
    journal_path: &Path,
    journal: &mut PackageJournal,
    index: usize,
) -> Result<(), UpdateError> {
    journal
        .created_prepared
        .remove(&journal.update.entries[index].prepared);
    // A crash before rename leaves an unowned sidecar and requires inspection.
    persist(journal_path, journal)
}

fn remove_owned(
    journal_path: &Path,
    journal: &mut PackageJournal,
    index: usize,
) -> Result<(), UpdateError> {
    let entry = journal.update.entries[index].clone();
    if !ensure_reserved_file_path(&entry.prepared, "package prepared file")? {
        if journal.created_prepared.contains(&entry.prepared) {
            relinquish(journal_path, journal, index)?;
        }
        return Ok(());
    }
    if !journal.created_prepared.contains(&entry.prepared) {
        return Err(UpdateError::Stage(
            "unowned package prepared file preserved; recovery requires inspection".into(),
        ));
    }
    let hash = sha256_file(&entry.prepared)?;
    if !hash.eq_ignore_ascii_case(&entry.expected_sha256)
        && !entry
            .original_sha256
            .as_ref()
            .is_some_and(|old| hash.eq_ignore_ascii_case(old))
    {
        return Err(UpdateError::Stage(
            "changed package prepared file preserved; recovery requires inspection".into(),
        ));
    }
    relinquish(journal_path, journal, index)?;
    std::fs::remove_file(&entry.prepared)?;
    Ok(())
}

pub(super) fn rollback_package(
    journal_path: &Path,
    journal: &mut PackageJournal,
) -> Result<(), UpdateError> {
    for index in (0..journal.update.entries.len()).rev() {
        let entry = journal.update.entries[index].clone();
        let target_exists = ensure_reserved_file_path(&entry.target, "package rollback target")?;
        if entry.had_original {
            let old = entry
                .original_sha256
                .as_deref()
                .ok_or_else(|| UpdateError::Stage("package original digest missing".into()))?;
            if target_exists && sha256_file(&entry.target)?.eq_ignore_ascii_case(old) {
                if ensure_reserved_file_path(&entry.backup, "package backup")? {
                    require_file_hash(&entry.backup, old, "package backup")?;
                }
                remove_owned(journal_path, journal, index)?;
                continue;
            }
            require_file_hash(&entry.backup, old, "package backup")?;
            if target_exists {
                require_file_hash(
                    &entry.target,
                    &entry.expected_sha256,
                    "package rollback target",
                )?;
            }
            remove_owned(journal_path, journal, index)?;
            #[cfg(test)]
            prepared_write_hook(&entry.prepared, "recovery");
            let mut input = File::open(&entry.backup)?;
            let mut output = create_owned(journal_path, journal, index)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            drop(output);
            std::fs::set_permissions(
                &entry.prepared,
                std::fs::metadata(&entry.backup)?.permissions(),
            )?;
            require_file_hash(&entry.prepared, old, "package rollback prepared file")?;
            relinquish(journal_path, journal, index)?;
            if target_exists {
                std::fs::remove_file(&entry.target)?;
            }
            std::fs::rename(&entry.prepared, &entry.target)?;
            require_file_hash(&entry.target, old, "restored package target")?;
        } else {
            if entry.original_sha256.is_some()
                || ensure_reserved_file_path(&entry.backup, "unexpected package backup")?
            {
                return Err(UpdateError::Stage(
                    "new package target has unexpected original data".into(),
                ));
            }
            if target_exists {
                require_file_hash(&entry.target, &entry.expected_sha256, "new package target")?;
                std::fs::remove_file(&entry.target)?;
            }
            remove_owned(journal_path, journal, index)?;
        }
    }
    Ok(())
}

pub(super) fn cleanup(
    journal_path: &Path,
    journal: &mut PackageJournal,
) -> Result<(), UpdateError> {
    for index in 0..journal.update.entries.len() {
        remove_owned(journal_path, journal, index)?;
        let entry = &journal.update.entries[index];
        if ensure_reserved_file_path(&entry.backup, "package backup")? {
            let old = entry.original_sha256.as_deref().ok_or_else(|| {
                UpdateError::Stage("unexpected backup for new package target".into())
            })?;
            remove_known_file(&entry.backup, "package backup", old, None)?;
        }
    }
    Ok(())
}
