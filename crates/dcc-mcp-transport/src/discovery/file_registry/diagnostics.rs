//! Process-local diagnostics for registry write transactions.
//!
//! Values are aggregated by registry directory because multiple in-process
//! handles normally share one registry snapshot. Reads never touch registry
//! files, so read-only callers such as doctor cannot perturb lock semantics.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use serde::Serialize;

type RegistryDiagnosticsMap = DashMap<PathBuf, Arc<RegistryWriteDiagnostics>>;

static REGISTRY_DIAGNOSTICS: LazyLock<RegistryDiagnosticsMap> =
    LazyLock::new(RegistryDiagnosticsMap::new);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct WriteDiagnosticsSnapshot {
    pub heartbeat_lock_timeouts: u64,
    pub last_heartbeat_write_ms: Option<u64>,
    pub registry_lock_wait_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct RegistryWriteDiagnostics {
    heartbeat_lock_timeouts: AtomicU64,
    last_heartbeat_write_ms: AtomicU64,
    registry_lock_wait_ms: AtomicU64,
}

impl RegistryWriteDiagnostics {
    pub fn record_lock_timeout(&self) {
        self.heartbeat_lock_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_lock_wait(&self, wait: Duration) {
        self.registry_lock_wait_ms.store(
            wait.as_millis().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }

    pub fn record_heartbeat_write(&self, elapsed: Duration) {
        self.last_heartbeat_write_ms.store(
            elapsed.as_millis().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }

    pub fn snapshot(&self) -> WriteDiagnosticsSnapshot {
        WriteDiagnosticsSnapshot {
            heartbeat_lock_timeouts: self.heartbeat_lock_timeouts.load(Ordering::Relaxed),
            last_heartbeat_write_ms: load_optional(&self.last_heartbeat_write_ms),
            registry_lock_wait_ms: load_optional(&self.registry_lock_wait_ms),
        }
    }
}

fn load_optional(counter: &AtomicU64) -> Option<u64> {
    match counter.load(Ordering::Relaxed) {
        0 => None,
        encoded => Some(encoded & 0x7fff_ffff_ffff_ffff),
    }
}

/// Return the process-local diagnostics handle for `registry_dir`.
#[must_use]
pub fn handle(registry_dir: &Path) -> Arc<RegistryWriteDiagnostics> {
    REGISTRY_DIAGNOSTICS
        .entry(registry_dir.to_path_buf())
        .or_insert_with(|| Arc::new(RegistryWriteDiagnostics::default()))
        .clone()
}

/// Read a consistent set of counters without opening registry files.
#[must_use]
pub fn snapshot(registry_dir: &Path) -> WriteDiagnosticsSnapshot {
    REGISTRY_DIAGNOSTICS
        .get(registry_dir)
        .map_or_else(WriteDiagnosticsSnapshot::default, |entry| entry.snapshot())
}

/// Reset counters for a test fixture without changing the shared handle.
#[cfg(test)]
pub fn reset_counts(registry_dir: &Path) {
    REGISTRY_DIAGNOSTICS
        .entry(registry_dir.to_path_buf())
        .or_default();
    if let Some(entry) = REGISTRY_DIAGNOSTICS.get(registry_dir) {
        entry.heartbeat_lock_timeouts.store(0, Ordering::Relaxed);
        entry.last_heartbeat_write_ms.store(0, Ordering::Relaxed);
        entry.registry_lock_wait_ms.store(0, Ordering::Relaxed);
    }
}
