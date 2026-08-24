use std::path::PathBuf;

use dcc_mcp_models::{FeedbackReport, FeedbackSeverity};
use serde_json::Value;

use crate::application::control_plane::DccControlPlane;
use crate::application::doctor::DoctorContext;
use crate::application::feedback::FeedbackRouteService;
use crate::application::feedback_bundle::{FeedbackBundleRequest, FeedbackBundleService};
use crate::application::feedback_file::{
    FeedbackFileAuthorization, FeedbackFileDecision, FeedbackFileRequest, FeedbackFileService,
};
use crate::domain::feedback_bundle::{DEFAULT_HOST_ERROR_LINES, MAX_HOST_ERROR_LINES};
use crate::domain::rest::FeedbackQueryRequest;
use crate::infra::github_issues::GhFeedbackIssueTracker;

use super::output::OutputFormat;

/// Arguments for gateway-owned feedback that survives DCC instance exit.
#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct FeedbackArgs {
    #[command(subcommand)]
    pub(super) action: Option<FeedbackAction>,
    /// Tool or operation that failed or blocked the workflow.
    #[arg(long)]
    pub(super) tool_name: Option<String>,
    /// Goal the agent was trying to accomplish.
    #[arg(long)]
    pub(super) intent: Option<String>,
    /// Parameters or approach already attempted.
    #[arg(long)]
    pub(super) attempt: Option<String>,
    /// Failure or limitation that prevented completion.
    #[arg(long)]
    pub(super) blocker: Option<String>,
    /// Feedback severity.
    #[arg(long, default_value = "blocked")]
    pub(super) severity: FeedbackSeverity,
    /// DCC type involved, if known.
    #[arg(long)]
    pub(super) dcc_type: Option<String>,
    /// Live or dead instance id involved, if known.
    #[arg(long)]
    pub(super) instance_id: Option<String>,
    /// Last known gateway request id, if available.
    #[arg(long)]
    pub(super) request_id: Option<String>,
    /// Last known job id, if available.
    #[arg(long)]
    pub(super) job_id: Option<String>,
}

#[derive(Debug, clap::Subcommand)]
pub(super) enum FeedbackAction {
    /// List persisted feedback newest first.
    List(FeedbackQueryArgs),
    /// Export the largest bounded persisted-feedback window as structured output.
    Export(FeedbackQueryArgs),
    /// Resolve a Finding v1 JSON file to its owning issue tracker without a gateway.
    Route(FeedbackRouteArgs),
    /// Assemble a bounded public-safe diagnostic bundle from a Finding v1 file.
    Bundle(FeedbackBundleArgs),
    /// Deduplicate and explicitly file a public-safe Finding v1 report on GitHub.
    File(FeedbackFileArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct FeedbackRouteArgs {
    /// Finding v1 JSON file to route.
    #[arg(value_name = "FINDING_JSON")]
    pub(super) finding: PathBuf,
    /// Optional catalog YAML/JSON file; defaults to the bundled public catalog.
    #[arg(long)]
    pub(super) catalog: Option<PathBuf>,
    /// Emit JSON (shortcut for the global `--output json`).
    #[arg(long)]
    pub(super) json: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct FeedbackBundleArgs {
    /// Public-safe Finding v1 JSON file to bundle.
    #[arg(value_name = "FINDING_JSON")]
    pub(super) finding: PathBuf,
    /// Host-error log directory; defaults to DCC_MCP_LOG_DIR or the platform log directory.
    #[arg(long)]
    pub(super) log_dir: Option<PathBuf>,
    /// DCC process id; defaults to evidence.dcc_pid in the finding.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(super) dcc_pid: Option<u32>,
    /// Maximum host-error records to include.
    #[arg(
        long,
        default_value_t = DEFAULT_HOST_ERROR_LINES as u16,
        value_parser = clap::value_parser!(u16).range(1..=MAX_HOST_ERROR_LINES as i64)
    )]
    pub(super) host_error_lines: u16,
    /// Emit JSON (shortcut for the global `--output json`).
    #[arg(long)]
    pub(super) json: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct FeedbackFileArgs {
    /// Public-safe Finding v1 JSON file to deduplicate and file.
    #[arg(value_name = "FINDING_JSON")]
    pub(super) finding: PathBuf,
    /// Optional catalog YAML/JSON file; defaults to the bundled public catalog.
    #[arg(long)]
    pub(super) catalog: Option<PathBuf>,
    /// Comment on this existing issue after deduplication.
    #[arg(
        long,
        conflicts_with = "create",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub(super) existing: Option<u64>,
    /// Create a new issue after deduplication.
    #[arg(long, conflicts_with = "existing")]
    pub(super) create: bool,
    /// Authorize the selected GitHub write; omitted means plan only.
    #[arg(long)]
    pub(super) yes: bool,
    /// Canonical Finding path bound by the reviewed plan.
    #[arg(long)]
    pub(super) expected_finding_path: Option<PathBuf>,
    /// Finding file content digest bound by the reviewed plan.
    #[arg(long)]
    pub(super) expected_finding_sha256: Option<String>,
    /// Finding fingerprint bound by the reviewed plan.
    #[arg(long)]
    pub(super) expected_fingerprint: Option<String>,
    /// Routed GitHub repository bound by the reviewed plan.
    #[arg(long)]
    pub(super) expected_repo: Option<String>,
    /// Catalog content digest bound by the reviewed plan.
    #[arg(long)]
    pub(super) expected_catalog_sha256: Option<String>,
    /// Emit JSON (shortcut for the global `--output json`).
    #[arg(long)]
    pub(super) json: bool,
}

impl FeedbackFileArgs {
    fn decision(&self) -> Option<FeedbackFileDecision> {
        self.existing
            .map(FeedbackFileDecision::Existing)
            .or(self.create.then_some(FeedbackFileDecision::Create))
    }

    fn validate(&self) -> Result<(), String> {
        if self.existing.is_some() && self.create {
            return Err("--existing and --create are mutually exclusive".to_string());
        }
        if self.yes && self.decision().is_none() {
            return Err(
                "--yes requires exactly one filing decision: --existing <number> or --create"
                    .to_string(),
            );
        }
        let bindings = [
            self.expected_finding_path.is_some(),
            self.expected_finding_sha256.is_some(),
            self.expected_fingerprint.is_some(),
            self.expected_repo.is_some(),
            self.expected_catalog_sha256.is_some(),
        ];
        if self.yes && !bindings.iter().all(|present| *present) {
            return Err(
                "--yes requires the complete authorization binding emitted by a prior plan"
                    .to_string(),
            );
        }
        if !self.yes && bindings.iter().any(|present| *present) {
            return Err("authorization binding arguments require --yes".to_string());
        }
        Ok(())
    }

    fn into_request(self) -> FeedbackFileRequest {
        let decision = self.decision();
        let authorization =
            self.expected_finding_path
                .map(|canonical_finding_path| FeedbackFileAuthorization {
                    canonical_finding_path,
                    finding_sha256: self
                        .expected_finding_sha256
                        .expect("validated authorization binding"),
                    fingerprint: self
                        .expected_fingerprint
                        .expect("validated authorization binding"),
                    repo: self.expected_repo.expect("validated authorization binding"),
                    catalog_sha256: self
                        .expected_catalog_sha256
                        .expect("validated authorization binding"),
                });
        FeedbackFileRequest {
            finding_path: self.finding,
            catalog_path: self.catalog,
            decision,
            authorized: self.yes,
            authorization,
        }
    }
}

#[derive(Debug, clap::Args)]
pub(super) struct FeedbackQueryArgs {
    /// Time window: 1h, 24h, 7d, or all.
    #[arg(long, default_value = "7d", value_parser = ["1h", "24h", "7d", "all"])]
    pub(super) range: String,
    /// Filter by DCC type.
    #[arg(long)]
    pub(super) dcc: Option<String>,
    /// Filter by feedback severity.
    #[arg(
        long,
        value_parser = ["blocked", "blocker", "degraded", "workaround_found", "suggestion"]
    )]
    pub(super) severity: Option<String>,
    /// Maximum records to return (1-1000).
    #[arg(long)]
    pub(super) limit: Option<usize>,
    /// Emit JSON (shortcut for the global `--output json`).
    #[arg(long)]
    pub(super) json: bool,
}

impl FeedbackQueryArgs {
    pub(super) fn into_request(self, default_limit: usize) -> FeedbackQueryRequest {
        FeedbackQueryRequest {
            range: self.range,
            dcc_type: self.dcc,
            severity: self.severity,
            limit: self.limit.unwrap_or(default_limit),
        }
    }
}

#[derive(Debug)]
pub(crate) enum FeedbackCommand {
    Report(FeedbackReport),
    List(FeedbackQueryRequest),
    Export(FeedbackQueryRequest),
    Route(FeedbackRouteArgs),
    Bundle(FeedbackBundleArgs),
    File(FeedbackFileRequest),
}

impl FeedbackArgs {
    pub(crate) fn requests_json(&self) -> bool {
        match self.action.as_ref() {
            Some(FeedbackAction::List(query) | FeedbackAction::Export(query)) => query.json,
            Some(FeedbackAction::Route(route)) => route.json,
            Some(FeedbackAction::Bundle(bundle)) => bundle.json,
            Some(FeedbackAction::File(file)) => file.json,
            None => false,
        }
    }

    pub(crate) fn requires_gateway(&self) -> bool {
        !matches!(
            self.action,
            Some(FeedbackAction::Route(_) | FeedbackAction::Bundle(_) | FeedbackAction::File(_))
        )
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.action.is_some() {
            if let Some(FeedbackAction::File(file)) = self.action.as_ref() {
                return file.validate();
            }
            return Ok(());
        }
        for (name, value) in [
            ("--tool-name", self.tool_name.as_ref()),
            ("--intent", self.intent.as_ref()),
            ("--blocker", self.blocker.as_ref()),
        ] {
            if value.is_none() {
                return Err(format!("{name} is required when filing feedback"));
            }
        }
        Ok(())
    }

    pub(crate) fn into_command(self) -> Result<FeedbackCommand, String> {
        if let Some(action) = self.action {
            return Ok(match action {
                FeedbackAction::List(query) => FeedbackCommand::List(query.into_request(100)),
                FeedbackAction::Export(query) => FeedbackCommand::Export(query.into_request(1_000)),
                FeedbackAction::Route(route) => FeedbackCommand::Route(route),
                FeedbackAction::Bundle(bundle) => FeedbackCommand::Bundle(bundle),
                FeedbackAction::File(file) => {
                    file.validate()?;
                    FeedbackCommand::File(file.into_request())
                }
            });
        }
        let required = |name: &str, value: Option<String>| {
            value.ok_or_else(|| format!("{name} is required when filing feedback"))
        };
        Ok(FeedbackCommand::Report(FeedbackReport {
            tool_name: required("--tool-name", self.tool_name)?,
            intent: required("--intent", self.intent)?,
            attempt: self.attempt,
            blocker: required("--blocker", self.blocker)?,
            severity: self.severity,
            dcc_type: self.dcc_type,
            instance_id: self.instance_id,
            request_id: self.request_id,
            job_id: self.job_id,
        }))
    }

    pub(crate) fn resolve_output(
        &self,
        output: Option<OutputFormat>,
    ) -> Result<OutputFormat, String> {
        self.validate()?;
        Ok(if self.requests_json() {
            OutputFormat::Json
        } else {
            output.unwrap_or_else(OutputFormat::auto_detect)
        })
    }

    pub(crate) async fn run(
        self,
        control: &DccControlPlane,
        doctor: &DoctorContext,
    ) -> anyhow::Result<Value> {
        match self.into_command().map_err(anyhow::Error::msg)? {
            FeedbackCommand::Report(report) => control.feedback(report).await,
            FeedbackCommand::List(request) | FeedbackCommand::Export(request) => {
                control.feedback_entries(request).await
            }
            FeedbackCommand::Route(route) => {
                let service = FeedbackRouteService::new(PathBuf::from("dcc-mcp-catalog.yml"));
                let result = service.route(&route.finding, route.catalog.as_deref())?;
                serde_json::to_value(result).map_err(Into::into)
            }
            FeedbackCommand::Bundle(bundle) => {
                let result = FeedbackBundleService
                    .bundle(
                        FeedbackBundleRequest {
                            finding_path: bundle.finding,
                            doctor_request: doctor.request(None, None, None),
                            log_dir: bundle.log_dir,
                            dcc_pid: bundle.dcc_pid,
                            host_error_lines: usize::from(bundle.host_error_lines),
                        },
                        control,
                    )
                    .await?;
                serde_json::to_value(result).map_err(Into::into)
            }
            FeedbackCommand::File(request) => {
                let tracker = GhFeedbackIssueTracker::default();
                let result = FeedbackFileService::new(&tracker).file(request)?;
                serde_json::to_value(result).map_err(Into::into)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(existing: Option<u64>, create: bool, yes: bool) -> FeedbackFileArgs {
        FeedbackFileArgs {
            finding: PathBuf::from("finding.json"),
            catalog: None,
            existing,
            create,
            yes,
            expected_finding_path: yes.then(|| PathBuf::from("finding.json")),
            expected_finding_sha256: yes.then(|| format!("sha256:{}", "a".repeat(64))),
            expected_fingerprint: yes.then(|| format!("sha256:{}", "b".repeat(64))),
            expected_repo: yes.then(|| "dcc-mcp/dcc-mcp-godot".to_string()),
            expected_catalog_sha256: yes.then(|| format!("sha256:{}", "c".repeat(64))),
            json: true,
        }
    }

    #[test]
    fn write_authorization_requires_one_explicit_decision() {
        assert_eq!(
            args(None, false, true).validate(),
            Err(
                "--yes requires exactly one filing decision: --existing <number> or --create"
                    .to_string()
            )
        );
        assert!(args(Some(42), false, true).validate().is_ok());
        assert!(args(None, true, true).validate().is_ok());
    }

    #[test]
    fn omitted_authorization_remains_a_read_only_plan() {
        let request = args(None, false, false).into_request();
        assert!(!request.authorized);
        assert_eq!(request.decision, None);

        let request = args(Some(42), false, false).into_request();
        assert!(!request.authorized);
        assert_eq!(request.decision, Some(FeedbackFileDecision::Existing(42)));
    }

    #[test]
    fn write_authorization_requires_a_complete_prior_plan_binding() {
        let mut value = args(None, true, true);
        value.expected_catalog_sha256 = None;

        assert_eq!(
            value.validate(),
            Err(
                "--yes requires the complete authorization binding emitted by a prior plan"
                    .to_string()
            )
        );
    }
}
