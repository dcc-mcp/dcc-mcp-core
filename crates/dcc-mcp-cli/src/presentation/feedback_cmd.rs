use dcc_mcp_models::{FeedbackReport, FeedbackSeverity};
use serde_json::Value;

use crate::application::control_plane::DccControlPlane;
use crate::domain::rest::FeedbackQueryRequest;

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
}

impl FeedbackArgs {
    pub(crate) fn requests_json(&self) -> bool {
        match self.action.as_ref() {
            Some(FeedbackAction::List(query) | FeedbackAction::Export(query)) => query.json,
            None => false,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.action.is_some() {
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

    pub(crate) async fn run(self, control: &DccControlPlane) -> anyhow::Result<Value> {
        match self.into_command().map_err(anyhow::Error::msg)? {
            FeedbackCommand::Report(report) => control.feedback(report).await,
            FeedbackCommand::List(request) | FeedbackCommand::Export(request) => {
                control.feedback_entries(request).await
            }
        }
    }
}
