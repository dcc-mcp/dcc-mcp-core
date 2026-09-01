use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::Job;
use crate::job_storage::JobStorage;
use crate::job_storage::JobStorageError;

pub(super) const PERSISTENCE_FAILURE_THRESHOLD: u32 = 3;
const PERSISTENCE_QUEUE_CAPACITY: usize = 64;
const PERSISTENCE_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

/// Runtime state of the optional job-persistence backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPersistenceState {
    /// No persistence backend was configured.
    NotConfigured,
    /// The configured backend is accepting writes.
    Healthy,
    /// A transient write failure occurred below the disable threshold.
    Degraded,
    /// Repeated identical write failures disabled persistence for this manager.
    Disabled,
}

/// Public, payload-safe snapshot of job-persistence health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPersistenceStatus {
    /// Current persistence state.
    pub state: JobPersistenceState,
    /// Consecutive occurrences of the same write error.
    pub consecutive_failures: u32,
    /// Stable error category without backend paths or raw messages.
    pub last_error_kind: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct PersistenceCircuit {
    configured: bool,
    disabled: bool,
    consecutive_failures: u32,
    last_error: Option<String>,
    last_error_kind: Option<String>,
}

impl PersistenceCircuit {
    pub(super) fn configured() -> Self {
        Self {
            configured: true,
            ..Self::default()
        }
    }

    pub(super) fn can_write(&self) -> bool {
        self.configured && !self.disabled
    }

    pub(super) fn record_success(&mut self) -> bool {
        let recovered = self.consecutive_failures > 0;
        self.consecutive_failures = 0;
        self.last_error = None;
        self.last_error_kind = None;
        recovered
    }

    pub(super) fn record_failure(&mut self, error: &JobStorageError) -> bool {
        let fingerprint = error.to_string();
        if self.last_error.as_deref() == Some(fingerprint.as_str()) {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        } else {
            self.consecutive_failures = 1;
            self.last_error = Some(fingerprint);
        }
        self.last_error_kind = Some(error_kind(error).to_string());

        if self.consecutive_failures >= PERSISTENCE_FAILURE_THRESHOLD {
            self.disabled = true;
            true
        } else {
            false
        }
    }

    pub(super) fn disable(&mut self, error_kind: &str) -> bool {
        let changed = !self.disabled;
        self.disabled = true;
        self.consecutive_failures = 0;
        self.last_error = None;
        self.last_error_kind = Some(error_kind.to_string());
        changed
    }

    pub(super) fn can_retry_retention(&self) -> bool {
        self.configured
            && self.disabled
            && self.last_error_kind.as_deref() == Some("retention_prune_failed")
    }

    fn record_retention_success(&mut self) -> bool {
        if self.can_retry_retention() {
            self.disabled = false;
            self.consecutive_failures = 0;
            self.last_error = None;
            self.last_error_kind = None;
            true
        } else {
            false
        }
    }

    pub(super) fn status(&self) -> JobPersistenceStatus {
        let state = if !self.configured {
            JobPersistenceState::NotConfigured
        } else if self.disabled {
            JobPersistenceState::Disabled
        } else if self.consecutive_failures > 0 {
            JobPersistenceState::Degraded
        } else {
            JobPersistenceState::Healthy
        };
        JobPersistenceStatus {
            state,
            consecutive_failures: self.consecutive_failures,
            last_error_kind: self.last_error_kind.clone(),
        }
    }
}

enum PersistenceCommand {
    Put {
        job: Box<Job>,
        completed: Option<SyncSender<()>>,
    },
    DeleteOlderThan {
        cutoff: DateTime<Utc>,
        completed: Option<SyncSender<bool>>,
    },
    Shutdown {
        completed: SyncSender<()>,
    },
}

struct WorkerDone(SyncSender<()>);

impl Drop for WorkerDone {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// Single-owner persistence worker.
///
/// The worker owns the complete `can_write -> storage.put -> record result`
/// transaction. That makes the failure threshold atomic across concurrent job
/// mutations without holding the public health-state mutex during backend I/O.
pub(super) struct PersistenceWriter {
    // Keep an explicit owner reference so Drop can fence a backend even when
    // the worker is stuck inside an arbitrary synchronous trait call.
    storage: Arc<dyn JobStorage>,
    sender: Option<SyncSender<PersistenceCommand>>,
    worker: Option<std::thread::JoinHandle<()>>,
    worker_done: Option<Mutex<Receiver<()>>>,
    circuit: Arc<Mutex<PersistenceCircuit>>,
    wait_for_completion: bool,
    enqueue_gate: Mutex<()>,
    closing: AtomicBool,
}

impl PersistenceWriter {
    pub(super) fn new(
        storage: Arc<dyn JobStorage>,
        circuit: Arc<Mutex<PersistenceCircuit>>,
        wait_for_completion: bool,
    ) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::sync_channel(PERSISTENCE_QUEUE_CAPACITY);
        let (worker_done_tx, worker_done_rx) = mpsc::sync_channel(1);
        let worker_circuit = circuit.clone();
        let worker_storage = storage.clone();
        let worker = std::thread::Builder::new()
            .name("dcc-mcp-job-persistence".to_string())
            .spawn(move || {
                let _done = WorkerDone(worker_done_tx);
                while let Ok(command) = receiver.recv() {
                    match command {
                        PersistenceCommand::Put { job, completed } => {
                            persist_one(&worker_storage, &worker_circuit, &job);
                            if let Some(completed) = completed {
                                let _ = completed.send(());
                            }
                        }
                        PersistenceCommand::DeleteOlderThan { cutoff, completed } => {
                            let can_prune = {
                                let circuit = worker_circuit.lock();
                                circuit.can_write() || circuit.can_retry_retention()
                            };
                            let succeeded = if can_prune {
                                match worker_storage.delete_older_than(cutoff) {
                                    Ok(_) => {
                                        worker_circuit.lock().record_retention_success();
                                        true
                                    }
                                    Err(error) => {
                                        worker_circuit.lock().disable("retention_prune_failed");
                                        tracing::warn!(error = %error, "JobStorage.delete_older_than failed during gc_stale");
                                        false
                                    }
                                }
                            } else {
                                false
                            };
                            if let Some(completed) = completed {
                                let _ = completed.send(succeeded);
                            }
                        }
                        PersistenceCommand::Shutdown { completed } => {
                            let _ = completed.send(());
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            storage,
            sender: Some(sender),
            worker: Some(worker),
            worker_done: Some(Mutex::new(worker_done_rx)),
            circuit,
            wait_for_completion,
            enqueue_gate: Mutex::new(()),
            closing: AtomicBool::new(false),
        })
    }

    pub(super) fn put(&self, job: &Job) {
        if self.wait_for_completion {
            let (completed_tx, completed_rx) = mpsc::sync_channel(0);
            let sent = {
                let _enqueue_guard = self.enqueue_gate.lock();
                if self.closing.load(Ordering::Acquire) || !self.circuit.lock().can_write() {
                    return;
                }
                let Some(sender) = &self.sender else {
                    self.disable("worker_unavailable");
                    return;
                };
                match sender.send(PersistenceCommand::Put {
                    job: Box::new(job.clone()),
                    completed: Some(completed_tx),
                }) {
                    Ok(()) => true,
                    Err(_) => {
                        self.disable("worker_unavailable");
                        false
                    }
                }
            };
            if !sent {
                return;
            }
            if completed_rx.recv().is_err() {
                self.disable("worker_unavailable");
            }
            return;
        }

        let _enqueue_guard = self.enqueue_gate.lock();
        if self.closing.load(Ordering::Acquire) || !self.circuit.lock().can_write() {
            return;
        }
        let Some(sender) = &self.sender else {
            self.disable("worker_unavailable");
            return;
        };
        match sender.try_send(PersistenceCommand::Put {
            job: Box::new(job.clone()),
            completed: None,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.disable("queue_full"),
            Err(TrySendError::Disconnected(_)) => self.disable("worker_unavailable"),
        }
    }

    pub(super) fn delete_older_than(&self, cutoff: DateTime<Utc>) -> bool {
        if self.wait_for_completion {
            let (completed_tx, completed_rx) = mpsc::sync_channel(0);
            let sent = {
                let _enqueue_guard = self.enqueue_gate.lock();
                if self.closing.load(Ordering::Acquire) || {
                    let circuit = self.circuit.lock();
                    !circuit.can_write() && !circuit.can_retry_retention()
                } {
                    return false;
                }
                let Some(sender) = &self.sender else {
                    self.disable("worker_unavailable");
                    return false;
                };
                match sender.send(PersistenceCommand::DeleteOlderThan {
                    cutoff,
                    completed: Some(completed_tx),
                }) {
                    Ok(()) => true,
                    Err(_) => {
                        self.disable("worker_unavailable");
                        false
                    }
                }
            };
            if !sent {
                return false;
            }
            match completed_rx.recv() {
                Ok(succeeded) => return succeeded,
                Err(_) => {
                    self.disable("worker_unavailable");
                    return false;
                }
            }
        }

        let _enqueue_guard = self.enqueue_gate.lock();
        if self.closing.load(Ordering::Acquire) || {
            let circuit = self.circuit.lock();
            !circuit.can_write() && !circuit.can_retry_retention()
        } {
            return false;
        }
        let Some(sender) = &self.sender else {
            self.disable("worker_unavailable");
            return false;
        };
        match sender.try_send(PersistenceCommand::DeleteOlderThan {
            cutoff,
            completed: None,
        }) {
            // Offloaded callers (including HTTP request handlers) must not
            // wait on arbitrary backend I/O. The worker performs the durable
            // delete independently; returning false keeps in-memory rows
            // visible until a later confirmed cleanup or restart.
            Ok(()) => false,
            Err(TrySendError::Full(_)) => {
                self.disable("queue_full");
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.disable("worker_unavailable");
                false
            }
        }
    }

    /// Delete terminal rows and wait for the worker to report the result.
    ///
    /// This is intended for callers that already run on a blocking thread
    /// (for example, the explicit `jobs_cleanup` MCP tool). Unlike the
    /// non-blocking offloaded path above, it never returns before the queued
    /// delete has completed, so a reported removal count cannot race a late
    /// durable delete.
    pub(super) fn delete_older_than_blocking(
        &self,
        cutoff: DateTime<Utc>,
        timeout: Duration,
    ) -> bool {
        let (completed_tx, completed_rx) = mpsc::sync_channel(0);
        {
            let _enqueue_guard = self.enqueue_gate.lock();
            if self.closing.load(Ordering::Acquire) || {
                let circuit = self.circuit.lock();
                !circuit.can_write() && !circuit.can_retry_retention()
            } {
                return false;
            }
            let Some(sender) = &self.sender else {
                self.disable("worker_unavailable");
                return false;
            };
            match sender.try_send(PersistenceCommand::DeleteOlderThan {
                cutoff,
                completed: Some(completed_tx),
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.disable("queue_full");
                    return false;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.disable("worker_unavailable");
                    return false;
                }
            }
        }
        match completed_rx.recv_timeout(timeout) {
            Ok(succeeded) => succeeded,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.disable("retention_prune_failed");
                false
            }
            Err(_) => {
                self.disable("worker_unavailable");
                false
            }
        }
    }

    /// Drain commands already accepted by the bounded worker queue.
    ///
    /// The shutdown marker is ordered after queued writes, so a successful
    /// return means those writes reached the backend before ownership closes.
    pub(super) fn drain(&self, timeout: Duration) -> bool {
        let _enqueue_guard = self.enqueue_gate.lock();
        self.closing.store(true, Ordering::Release);
        let Some(sender) = &self.sender else {
            return true;
        };
        let (completed_tx, completed_rx) = mpsc::sync_channel(0);
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match sender.try_send(PersistenceCommand::Shutdown {
                completed: completed_tx.clone(),
            }) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return true,
                Err(TrySendError::Full(_)) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Full(_)) => return false,
            }
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        completed_rx.recv_timeout(remaining).is_ok()
    }

    fn disable(&self, error_kind: &'static str) {
        let mut circuit = self.circuit.lock();
        if circuit.disable(error_kind) {
            tracing::warn!(
                error_kind,
                queue_capacity = PERSISTENCE_QUEUE_CAPACITY,
                "job persistence disabled because its bounded worker cannot accept writes"
            );
        }
    }
}

impl Drop for PersistenceWriter {
    fn drop(&mut self) {
        self.sender.take();
        let Some(worker) = self.worker.take() else {
            return;
        };
        let worker_finished = match self.worker_done.take() {
            Some(done) => match done.lock().recv_timeout(PERSISTENCE_SHUTDOWN_TIMEOUT) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
                Err(RecvTimeoutError::Timeout) => false,
            },
            None => false,
        };
        if worker_finished {
            let _ = worker.join();
        } else {
            self.disable("shutdown_timeout");
            if !self.storage.force_close() {
                tracing::warn!(
                    "job persistence backend does not provide force-close; in-flight I/O may outlive shutdown"
                );
            }
            tracing::warn!(
                timeout_ms = PERSISTENCE_SHUTDOWN_TIMEOUT.as_millis(),
                "job persistence worker did not stop within the bounded shutdown window"
            );
            // Rust cannot pre-empt an arbitrary synchronous trait call. Dropping
            // the JoinHandle detaches that call, while the closed sender and
            // disabled circuit ensure it cannot accept new manager work.
        }
    }
}

fn persist_one(storage: &Arc<dyn JobStorage>, circuit: &Arc<Mutex<PersistenceCircuit>>, job: &Job) {
    if !circuit.lock().can_write() {
        return;
    }

    let result = storage.put(job);
    let mut persistence = circuit.lock();
    if !persistence.can_write() {
        return;
    }
    match result {
        Ok(()) => {
            if persistence.record_success() {
                tracing::info!("job persistence recovered after a transient write failure");
            }
        }
        Err(error) => {
            let disabled = persistence.record_failure(&error);
            let status = persistence.status();
            drop(persistence);
            if disabled {
                tracing::warn!(
                    error = %error,
                    error_kind = status.last_error_kind.as_deref().unwrap_or("unknown"),
                    consecutive_failures = status.consecutive_failures,
                    "job persistence disabled after repeated identical write failures"
                );
            } else {
                tracing::debug!(
                    job_id = %job.id,
                    error = %error,
                    consecutive_failures = status.consecutive_failures,
                    "JobStorage.put failed"
                );
            }
        }
    }
}

pub(super) fn error_kind(error: &JobStorageError) -> &'static str {
    match error {
        JobStorageError::Backend(message) => backend_error_kind(message),
        JobStorageError::Decode(_) => "decode",
        JobStorageError::FeatureDisabled => "feature_disabled",
    }
}

/// Reduce backend failures to a stable, payload-safe category.
///
/// SQLite reports read-only, WAL/sidecar, lock, and disk-full failures with
/// platform-specific wording. Keeping the raw message only inside the
/// circuit fingerprint preserves exact-error latching while exposing a
/// useful, path-free category through health/debug payloads.
fn backend_error_kind(message: &str) -> &'static str {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("readonly")
        || normalized.contains("read-only")
        || normalized.contains("sqlite_readonly")
    {
        "readonly"
    } else if normalized.contains("wal")
        || normalized.contains("-shm")
        || normalized.contains("-wal")
        || normalized.contains("journal")
    {
        "wal"
    } else if normalized.contains("busy")
        || normalized.contains("locked")
        || normalized.contains("lock")
    {
        "busy"
    } else if normalized.contains("disk full") || normalized.contains("database or disk is full") {
        "disk_full"
    } else {
        "backend"
    }
}
