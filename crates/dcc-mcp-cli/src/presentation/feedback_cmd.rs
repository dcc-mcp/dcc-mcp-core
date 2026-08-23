use dcc_mcp_models::{FeedbackReport, FeedbackSeverity};

/// Arguments for gateway-owned feedback that survives DCC instance exit.
#[derive(Debug, clap::Args)]
pub(crate) struct FeedbackArgs {
    /// Tool or operation that failed or blocked the workflow.
    #[arg(long)]
    pub(super) tool_name: String,
    /// Goal the agent was trying to accomplish.
    #[arg(long)]
    pub(super) intent: String,
    /// Parameters or approach already attempted.
    #[arg(long)]
    pub(super) attempt: Option<String>,
    /// Failure or limitation that prevented completion.
    #[arg(long)]
    pub(super) blocker: String,
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

impl From<FeedbackArgs> for FeedbackReport {
    fn from(args: FeedbackArgs) -> Self {
        Self {
            tool_name: args.tool_name,
            intent: args.intent,
            attempt: args.attempt,
            blocker: args.blocker,
            severity: args.severity,
            dcc_type: args.dcc_type,
            instance_id: args.instance_id,
            request_id: args.request_id,
            job_id: args.job_id,
        }
    }
}
