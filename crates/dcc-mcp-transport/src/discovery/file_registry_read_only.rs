//! Non-mutating live registry reads for read-only inventory clients.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::time::Instant;

use fs4::{FileExt, TryLockError};

use super::{
    DEFAULT_REGISTRY_LOCK_BACKOFF_MS, DEFAULT_REGISTRY_LOCK_TIMEOUT_MS, FileRegistry,
    GATEWAY_SENTINEL_DCC_TYPE, REGISTRY_LOCK_BACKOFF_ENV, REGISTRY_LOCK_FILE,
    REGISTRY_LOCK_TIMEOUT_ENV,
};
use crate::discovery::liveness::{is_pid_alive, legacy_sidecar_host_pid};
use crate::discovery::types::ServiceEntry;
use crate::error::{TransportError, TransportResult};

pub(super) fn read_alive_read_only(
    registry_dir: &Path,
) -> TransportResult<(Vec<ServiceEntry>, usize)> {
    let _registry_lock = read_shared_registry_lock(registry_dir)?;
    let entries = FileRegistry::read_snapshot(registry_dir)?;
    let mut live = Vec::with_capacity(entries.len());
    let mut dead = 0;
    for entry in entries {
        if entry.dcc_type == GATEWAY_SENTINEL_DCC_TYPE {
            continue;
        }
        if entry_is_alive_read_only(&entry)? {
            live.push(entry);
        } else {
            dead += 1;
        }
    }
    Ok((live, dead))
}

fn read_shared_registry_lock(registry_dir: &Path) -> TransportResult<Option<File>> {
    let path = registry_dir.join(REGISTRY_LOCK_FILE);
    let file = match OpenOptions::new().read(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(TransportError::RegistryFile(format!(
                "failed to open registry read lock {}: {}",
                path.display(),
                error
            )));
        }
    };
    let started = Instant::now();
    let timeout =
        super::env_duration_ms(REGISTRY_LOCK_TIMEOUT_ENV, DEFAULT_REGISTRY_LOCK_TIMEOUT_MS);
    let backoff =
        super::env_duration_ms(REGISTRY_LOCK_BACKOFF_ENV, DEFAULT_REGISTRY_LOCK_BACKOFF_MS);
    loop {
        match FileExt::try_lock_shared(&file) {
            Ok(()) => return Ok(Some(file)),
            Err(TryLockError::WouldBlock) if started.elapsed() < timeout => {
                std::thread::sleep(backoff.min(timeout.saturating_sub(started.elapsed())));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(TransportError::RegistryFile(format!(
                    "timed out waiting for registry read lock after {}ms",
                    timeout.as_millis()
                )));
            }
            Err(TryLockError::Error(error)) => {
                return Err(TransportError::RegistryFile(format!(
                    "failed to lock registry read lock {}: {}",
                    path.display(),
                    error
                )));
            }
        }
    }
}

fn entry_is_alive_read_only(entry: &ServiceEntry) -> TransportResult<bool> {
    let owner_alive = if let Some(path) = entry.sentinel_path.as_deref() {
        !sentinel_is_dead_read_only(path)?
    } else {
        entry.pid.is_none_or(is_pid_alive)
    };
    let bound_host_pid = entry.host_pid.or_else(|| legacy_sidecar_host_pid(entry));
    let host_alive = bound_host_pid.is_none_or(is_pid_alive);
    Ok(owner_alive && host_alive)
}

fn sentinel_is_dead_read_only(path: &Path) -> TransportResult<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(TransportError::RegistryFile(format!(
                "failed to open sentinel {} for read-only liveness: {}",
                path.display(),
                error
            )));
        }
    };
    match FileExt::try_lock_shared(&file) {
        Ok(()) => {
            let _ = file.unlock();
            Ok(true)
        }
        Err(TryLockError::WouldBlock) => Ok(false),
        Err(TryLockError::Error(error)) => Err(TransportError::RegistryFile(format!(
            "failed to probe sentinel {} for read-only liveness: {}",
            path.display(),
            error
        ))),
    }
}
