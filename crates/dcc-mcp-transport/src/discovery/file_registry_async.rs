//! Async adapters for blocking [`FileRegistry`] operations.

use std::sync::Arc;
use std::time::Duration;

use super::file_registry::FileRegistry;
use super::types::{ServiceEntry, ServiceKey, ServiceStatus};
use crate::error::{TransportError, TransportResult};

async fn run_registry_io<T>(
    operation: impl FnOnce() -> TransportResult<T> + Send + 'static,
) -> TransportResult<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| TransportError::Internal(format!("registry I/O task failed: {error}")))?
}

impl FileRegistry {
    /// Register a service without blocking the async runtime.
    pub async fn register_async(self: &Arc<Self>, entry: ServiceEntry) -> TransportResult<()> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.register(entry)).await
    }

    /// Deregister a service without blocking the async runtime.
    pub async fn deregister_async(
        self: &Arc<Self>,
        key: ServiceKey,
    ) -> TransportResult<Option<ServiceEntry>> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.deregister(&key)).await
    }

    /// Reload and list one DCC family on the blocking pool.
    pub async fn list_instances_async(
        self: &Arc<Self>,
        dcc_type: String,
    ) -> TransportResult<Vec<ServiceEntry>> {
        let registry = Arc::clone(self);
        run_registry_io(move || Ok(registry.list_instances(&dcc_type))).await
    }

    /// Reload and list every service on the blocking pool.
    pub async fn list_all_async(self: &Arc<Self>) -> TransportResult<Vec<ServiceEntry>> {
        let registry = Arc::clone(self);
        run_registry_io(move || Ok(registry.list_all())).await
    }

    /// Persist a heartbeat on the blocking pool.
    pub async fn heartbeat_async(self: &Arc<Self>, key: ServiceKey) -> TransportResult<bool> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.heartbeat(&key)).await
    }

    /// Persist a status transition on the blocking pool.
    pub async fn update_status_async(
        self: &Arc<Self>,
        key: ServiceKey,
        status: ServiceStatus,
    ) -> TransportResult<bool> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.update_status(&key, status)).await
    }

    /// Persist a compare-and-set status transition on the blocking pool.
    pub async fn update_status_if_unchanged_async(
        self: &Arc<Self>,
        observed: ServiceEntry,
        status: ServiceStatus,
    ) -> TransportResult<bool> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.update_status_if_unchanged(&observed, status)).await
    }

    /// Remove stale rows on the blocking pool.
    pub async fn cleanup_stale_async(
        self: &Arc<Self>,
        stale_timeout: Duration,
    ) -> TransportResult<usize> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.cleanup_stale(stale_timeout)).await
    }

    /// Acquire a pool lease on the blocking pool.
    pub async fn acquire_lease_async(
        self: &Arc<Self>,
        dcc_type: String,
        instance_id: Option<String>,
        owner: String,
        current_job_id: Option<String>,
        ttl: Option<Duration>,
    ) -> TransportResult<Option<ServiceEntry>> {
        let registry = Arc::clone(self);
        run_registry_io(move || {
            registry.acquire_lease(
                &dcc_type,
                instance_id.as_deref(),
                owner,
                current_job_id,
                ttl,
            )
        })
        .await
    }

    /// Release a pool lease on the blocking pool.
    pub async fn release_lease_async(
        self: &Arc<Self>,
        key: ServiceKey,
        owner: Option<String>,
    ) -> TransportResult<Option<ServiceEntry>> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.release_lease(&key, owner.as_deref())).await
    }

    /// Prune dead entries on the blocking pool.
    pub async fn prune_dead_entries_async(self: &Arc<Self>) -> TransportResult<usize> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.prune_dead_entries()).await
    }

    /// Prune dead process owners on the blocking pool.
    pub async fn prune_dead_pids_async(self: &Arc<Self>) -> TransportResult<usize> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.prune_dead_pids()).await
    }

    /// Read live entries and persist any eviction on the blocking pool.
    pub async fn read_alive_async(self: &Arc<Self>) -> TransportResult<(Vec<ServiceEntry>, usize)> {
        let registry = Arc::clone(self);
        run_registry_io(move || registry.read_alive()).await
    }
}
