//! Error types for the scheduler crate.

use thiserror::Error;

/// Errors returned by the scheduler.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// YAML parse / IO failure while loading a `*.schedules.yaml` file.
    #[error("failed to load schedules from {path}: {message}")]
    Load {
        /// Offending path.
        path: String,
        /// Underlying error message.
        message: String,
    },

    /// A cron expression failed to parse.
    #[error("invalid cron expression {expression:?}: {message}")]
    InvalidCron {
        /// The raw cron string.
        expression: String,
        /// Underlying parser message.
        message: String,
    },

    /// Unknown `chrono_tz` timezone name.
    #[error("unknown timezone {timezone:?}")]
    InvalidTimezone {
        /// The raw timezone name.
        timezone: String,
    },

    /// A duplicate schedule id was declared across the loaded files.
    #[error("duplicate schedule id {id:?}")]
    DuplicateId {
        /// The offending id.
        id: String,
    },

    /// A webhook secret env var is referenced but not set.
    #[error("webhook secret env var {var:?} not set")]
    MissingSecretEnv {
        /// Env var name.
        var: String,
    },

    /// Validation against the schema failed.
    #[error("invalid schedule spec: {0}")]
    Validation(String),
}
