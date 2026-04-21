//! [`ScheduleSpec`] and [`TriggerSpec`] — the sibling-file schema.
//!
//! Schedule files live alongside a skill's `SKILL.md`, for example:
//!
//! ```yaml
//! schedules:
//!   - id: nightly_cleanup
//!     workflow: scene_cleanup
//!     inputs:
//!       scope: all-scenes
//!     trigger:
//!       kind: cron
//!       # 6-field cron: sec min hour day month weekday
//!       expression: "0 0 3 * * *"
//!       timezone: "UTC"
//!       jitter_secs: 120
//!     enabled: true
//!     max_concurrent: 1
//!
//!   - id: on_upload
//!     workflow: validate_upload
//!     inputs:
//!       path: "{{trigger.payload.file_path}}"
//!     trigger:
//!       kind: webhook
//!       path: "/webhooks/upload"
//!       secret_env: UPLOAD_WEBHOOK_SECRET
//!     enabled: true
//! ```
//!
//! The sibling-file pattern (issue #356) keeps `SKILL.md` small; the skill
//! points at one or more `*.schedules.yaml` files via
//! `metadata.dcc-mcp.workflow.schedules`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SchedulerError;

/// A single registered schedule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleSpec {
    /// Stable id, unique across all schedule files loaded by the scheduler.
    pub id: String,
    /// Name of the `WorkflowSpec` to fire (looked up in the
    /// `WorkflowCatalog` by the [`JobSink`](crate::JobSink)).
    pub workflow: String,
    /// Static inputs passed to the workflow. May contain template
    /// placeholders like `{{trigger.payload.<jsonpath>}}` that are rendered
    /// from the trigger payload at fire time.
    #[serde(default)]
    pub inputs: serde_json::Value,
    /// Trigger configuration.
    pub trigger: TriggerSpec,
    /// If `false`, this schedule is ignored at load time.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum number of concurrent in-flight invocations. A fire is
    /// skipped (logged) when the counter would exceed this value.
    /// `0` means unlimited.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
}

const fn default_enabled() -> bool {
    true
}

const fn default_max_concurrent() -> u32 {
    1
}

/// How a schedule is triggered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TriggerSpec {
    /// Fire on a cron schedule.
    Cron {
        /// Cron expression in the 6- or 7-field form understood by the
        /// [`cron`](https://crates.io/crates/cron) crate:
        /// `sec min hour day_of_month month day_of_week [year]`. The
        /// seconds field is **required** — a classic 5-field expression
        /// like `"0 3 * * *"` will fail to parse; use
        /// `"0 0 3 * * *"` for "every day at 03:00".
        expression: String,
        /// `chrono_tz` timezone name. Defaults to `"UTC"`.
        #[serde(default = "default_timezone")]
        timezone: String,
        /// Random jitter in seconds, added to each fire time. `0`
        /// disables jitter. Defaults to `0`.
        #[serde(default)]
        jitter_secs: u32,
    },
    /// Fire on an HTTP POST webhook.
    Webhook {
        /// Absolute URL path, e.g. `"/webhooks/upload"`.
        path: String,
        /// Optional env var name holding the HMAC-SHA256 shared secret.
        /// When set, the webhook verifies `X-Hub-Signature-256` with
        /// constant-time comparison; mismatch returns 401.
        #[serde(default)]
        secret_env: Option<String>,
    },
}

fn default_timezone() -> String {
    "UTC".to_string()
}

/// Top-level YAML schema: a `schedules:` list.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleFile {
    /// Schedules declared in this file.
    #[serde(default)]
    pub schedules: Vec<ScheduleSpec>,
}

impl ScheduleFile {
    /// Parse a YAML document into a [`ScheduleFile`].
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::Load`] on parse failure.
    pub fn from_yaml_str(source: &str, path_hint: &str) -> Result<Self, SchedulerError> {
        serde_yaml_ng::from_str(source).map_err(|e| SchedulerError::Load {
            path: path_hint.to_string(),
            message: e.to_string(),
        })
    }

    /// Load a schedule file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::Load`] on IO or parse failure.
    pub fn load(path: &Path) -> Result<Self, SchedulerError> {
        let source = std::fs::read_to_string(path).map_err(|e| SchedulerError::Load {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::from_yaml_str(&source, &path.display().to_string())
    }
}

impl ScheduleSpec {
    /// Structural validation — checks the cron expression parses, the
    /// timezone resolves, and webhook paths are non-empty.
    ///
    /// # Errors
    ///
    /// Returns a [`SchedulerError`] variant describing the first problem
    /// encountered.
    pub fn validate(&self) -> Result<(), SchedulerError> {
        if self.id.trim().is_empty() {
            return Err(SchedulerError::Validation("schedule id is empty".into()));
        }
        if self.workflow.trim().is_empty() {
            return Err(SchedulerError::Validation(format!(
                "schedule {:?}: workflow name is empty",
                self.id
            )));
        }
        match &self.trigger {
            TriggerSpec::Cron {
                expression,
                timezone,
                ..
            } => {
                crate::service::parse_cron(expression)?;
                crate::service::parse_timezone(timezone)?;
            }
            TriggerSpec::Webhook { path, .. } => {
                if path.trim().is_empty() || !path.starts_with('/') {
                    return Err(SchedulerError::Validation(format!(
                        "schedule {:?}: webhook path must start with '/'",
                        self.id
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_cron_schedule() {
        let yaml = r#"
schedules:
  - id: nightly
    workflow: cleanup
    trigger:
      kind: cron
      expression: "0 3 * * *"
"#;
        let file = ScheduleFile::from_yaml_str(yaml, "test.yaml").unwrap();
        assert_eq!(file.schedules.len(), 1);
        let s = &file.schedules[0];
        assert_eq!(s.id, "nightly");
        assert!(s.enabled);
        assert_eq!(s.max_concurrent, 1);
        match &s.trigger {
            TriggerSpec::Cron {
                expression,
                timezone,
                jitter_secs,
            } => {
                assert_eq!(expression, "0 3 * * *");
                assert_eq!(timezone, "UTC");
                assert_eq!(*jitter_secs, 0);
            }
            _ => panic!("wrong trigger kind"),
        }
    }

    #[test]
    fn parses_webhook_schedule() {
        let yaml = r#"
schedules:
  - id: on_upload
    workflow: validate_upload
    inputs:
      path: "{{trigger.payload.file_path}}"
    trigger:
      kind: webhook
      path: /webhooks/upload
      secret_env: UPLOAD_WEBHOOK_SECRET
"#;
        let file = ScheduleFile::from_yaml_str(yaml, "t").unwrap();
        let s = &file.schedules[0];
        match &s.trigger {
            TriggerSpec::Webhook { path, secret_env } => {
                assert_eq!(path, "/webhooks/upload");
                assert_eq!(secret_env.as_deref(), Some("UPLOAD_WEBHOOK_SECRET"));
            }
            _ => panic!("wrong trigger kind"),
        }
    }

    #[test]
    fn rejects_empty_workflow() {
        let s = ScheduleSpec {
            id: "x".into(),
            workflow: " ".into(),
            inputs: serde_json::Value::Null,
            trigger: TriggerSpec::Cron {
                expression: "* * * * * *".into(),
                timezone: "UTC".into(),
                jitter_secs: 0,
            },
            enabled: true,
            max_concurrent: 1,
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn rejects_bad_cron_expression() {
        let s = ScheduleSpec {
            id: "x".into(),
            workflow: "w".into(),
            inputs: serde_json::Value::Null,
            trigger: TriggerSpec::Cron {
                expression: "not a cron".into(),
                timezone: "UTC".into(),
                jitter_secs: 0,
            },
            enabled: true,
            max_concurrent: 1,
        };
        assert!(matches!(
            s.validate(),
            Err(SchedulerError::InvalidCron { .. })
        ));
    }

    #[test]
    fn rejects_bad_timezone() {
        let s = ScheduleSpec {
            id: "x".into(),
            workflow: "w".into(),
            inputs: serde_json::Value::Null,
            trigger: TriggerSpec::Cron {
                expression: "* * * * * *".into(),
                timezone: "Mars/Olympus".into(),
                jitter_secs: 0,
            },
            enabled: true,
            max_concurrent: 1,
        };
        assert!(matches!(
            s.validate(),
            Err(SchedulerError::InvalidTimezone { .. })
        ));
    }
}
