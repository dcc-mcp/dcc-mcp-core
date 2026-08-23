use serde::{Deserialize, Serialize};

use crate::job_storage::JobStorageError;

pub(super) const PERSISTENCE_FAILURE_THRESHOLD: u32 = 3;

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

fn error_kind(error: &JobStorageError) -> &'static str {
    match error {
        JobStorageError::Backend(_) => "backend",
        JobStorageError::Decode(_) => "decode",
        JobStorageError::FeatureDisabled => "feature_disabled",
    }
}
