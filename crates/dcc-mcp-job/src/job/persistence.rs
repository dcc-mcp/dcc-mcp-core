use std::sync::Arc;
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
        completed: Option<SyncSender<()>>,
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
    sender: Option<SyncSender<PersistenceCommand>>,
    worker: Option<std::thread::JoinHandle<()>>,
    worker_done: Option<Mutex<Receiver<()>>>,
    circuit: Arc<Mutex<PersistenceCircuit>>,
    wait_for_completion: bool,
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
        let worker = std::thread::Builder::new()
            .name("dcc-mcp-job-persistence".to_string())
            .spawn(move || {
                let _done = WorkerDone(worker_done_tx);
                while let Ok(command) = receiver.recv() {
                    match command {
                        PersistenceCommand::Put { job, completed } => {
                            persist_one(&storage, &worker_circuit, &job);
                            if let Some(completed) = completed {
                                let _ = completed.send(());
                            }
                        }
                        PersistenceCommand::DeleteOlderThan { cutoff, completed } => {
                            let can_write = worker_circuit.lock().can_write();
                            if can_write && let Err(error) = storage.delete_older_than(cutoff)
                            {
                                tracing::warn!(error = %error, "JobStorage.delete_older_than failed during gc_stale");
                            }
                            if let Some(completed) = completed {
                                let _ = completed.send(());
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            worker_done: Some(Mutex::new(worker_done_rx)),
            circuit,
            wait_for_completion,
        })
    }

    pub(super) fn put(&self, job: &Job) {
        if !self.circuit.lock().can_write() {
            return;
        }
        let Some(sender) = &self.sender else {
            self.disable("worker_unavailable");
            return;
        };

        if self.wait_for_completion {
            let (completed_tx, completed_rx) = mpsc::sync_channel(0);
            if sender
                .send(PersistenceCommand::Put {
                    job: Box::new(job.clone()),
                    completed: Some(completed_tx),
                })
                .is_err()
            {
                self.disable("worker_unavailable");
                return;
            }
            if completed_rx.recv().is_err() {
                self.disable("worker_unavailable");
            }
            return;
        }

        match sender.try_send(PersistenceCommand::Put {
            job: Box::new(job.clone()),
            completed: None,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.disable("queue_full"),
            Err(TrySendError::Disconnected(_)) => self.disable("worker_unavailable"),
        }
    }

    pub(super) fn delete_older_than(&self, cutoff: DateTime<Utc>) {
        let can_write = self.circuit.lock().can_write();
        if !can_write {
            return;
        }
        let Some(sender) = &self.sender else {
            self.disable("worker_unavailable");
            return;
        };

        if self.wait_for_completion {
            let (completed_tx, completed_rx) = mpsc::sync_channel(0);
            if sender
                .send(PersistenceCommand::DeleteOlderThan {
                    cutoff,
                    completed: Some(completed_tx),
                })
                .is_err()
            {
                self.disable("worker_unavailable");
                return;
            }
            if completed_rx.recv().is_err() {
                self.disable("worker_unavailable");
            }
            return;
        }

        match sender.try_send(PersistenceCommand::DeleteOlderThan {
            cutoff,
            completed: None,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.disable("queue_full"),
            Err(TrySendError::Disconnected(_)) => self.disable("worker_unavailable"),
        }
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

fn error_kind(error: &JobStorageError) -> &'static str {
    match error {
        JobStorageError::Backend(_) => "backend",
        JobStorageError::Decode(_) => "decode",
        JobStorageError::FeatureDisabled => "feature_disabled",
    }
}
