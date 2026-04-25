//! `PyCrashRecoveryPolicy` — Python binding for the crash-recovery policy.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use super::helpers::map_process_err;
use crate::recovery::{BackoffStrategy, CrashRecoveryPolicy};
use crate::types::{DccProcessConfig, ProcessStatus};

/// Crash recovery policy for DCC processes.
///
/// # Example (Python)
///
/// ```python
/// policy = PyCrashRecoveryPolicy(max_restarts=3)
/// policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
/// print(policy.should_restart("crashed"))   # True
/// print(policy.next_delay_ms("maya", 0))    # 1000
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "PyCrashRecoveryPolicy")]
pub struct PyCrashRecoveryPolicy {
    inner: CrashRecoveryPolicy,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyCrashRecoveryPolicy {
    /// Create a policy with ``max_restarts`` and fixed 2 s back-off by default.
    #[new]
    #[pyo3(signature = (max_restarts=3))]
    pub fn new(max_restarts: u32) -> Self {
        Self {
            inner: CrashRecoveryPolicy::new(max_restarts),
        }
    }

    /// Switch to exponential back-off.
    pub fn use_exponential_backoff(&mut self, initial_ms: u64, max_delay_ms: u64) {
        self.inner.backoff = BackoffStrategy::Exponential {
            initial_ms,
            max_delay_ms,
        };
    }

    /// Switch to fixed back-off.
    pub fn use_fixed_backoff(&mut self, delay_ms: u64) {
        self.inner.backoff = BackoffStrategy::Fixed { delay_ms };
    }

    /// Returns `True` if the given status string warrants a restart.
    ///
    /// Recognised status values: ``"crashed"``, ``"unresponsive"``.
    /// Always returns `False` when `max_restarts` is 0.
    pub fn should_restart(&self, status: &str) -> PyResult<bool> {
        if self.inner.max_restarts == 0 {
            return Ok(false);
        }
        let s = parse_status(status)?;
        Ok(self.inner.should_restart(s))
    }

    /// Return the delay (ms) before attempt ``attempt`` (0-indexed), or raise
    /// `RuntimeError` if `max_restarts` has been exceeded.
    pub fn next_delay_ms(&self, name: &str, attempt: u32) -> PyResult<u64> {
        let cfg = DccProcessConfig::new(name, "dummy");
        self.inner
            .next_restart_delay(&cfg, attempt)
            .map(|d| d.as_millis() as u64)
            .map_err(map_process_err)
    }

    /// Maximum number of restart attempts.
    #[getter]
    pub fn max_restarts(&self) -> u32 {
        self.inner.max_restarts
    }

    pub fn __repr__(&self) -> String {
        format!(
            "PyCrashRecoveryPolicy(max_restarts={})",
            self.inner.max_restarts
        )
    }
}

pub(super) fn parse_status(s: &str) -> PyResult<ProcessStatus> {
    match s {
        "running" => Ok(ProcessStatus::Running),
        "starting" => Ok(ProcessStatus::Starting),
        "stopped" => Ok(ProcessStatus::Stopped),
        "crashed" => Ok(ProcessStatus::Crashed),
        "unresponsive" => Ok(ProcessStatus::Unresponsive),
        "restarting" => Ok(ProcessStatus::Restarting),
        other => Err(PyValueError::new_err(format!(
            "unknown ProcessStatus: '{other}' — expected one of running/starting/stopped/crashed/unresponsive/restarting"
        ))),
    }
}
