//! Parse before the legacy CLI startup path can apply a staged update.
//!
//! This executable entrypoint uses the canonical Clap tree. Parse errors, help,
//! and version requests finish here, including malformed inspection requests.

use std::ffi::OsString;

use clap::{CommandFactory, FromArgMatches};

use super::cli::Args;
use super::cli::result_output::select_fields;
use super::feedback_cmd::{FeedbackBundleArgs, FeedbackFileArgs};
use super::output::{ErrorEnvelope, ExitCode, OutputFormat, OutputWriter};
use super::update_cmd::{DiscoverArgs, InspectArgs, run_discovery, run_inspection};

#[derive(Debug)]
enum ReadOnlyCommand {
    Update(InspectArgs),
    Discover(DiscoverArgs),
    Bundle(FeedbackBundleArgs),
    FeedbackPlan(FeedbackFileArgs),
}

type ReadOnlyArgs = (ReadOnlyCommand, OutputFormat, Vec<String>);

fn read_only_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<ReadOnlyArgs>, clap::Error> {
    let matches = Args::command().try_get_matches_from(arguments)?;
    let mut output = matches
        .get_one::<OutputFormat>("output")
        .copied()
        .unwrap_or_else(OutputFormat::auto_detect);
    let command = match matches.subcommand() {
        Some(("update", update)) => match update.subcommand() {
            Some(("inspect", inspect)) => {
                ReadOnlyCommand::Update(InspectArgs::from_arg_matches(inspect)?)
            }
            Some(("discover", discover)) => {
                ReadOnlyCommand::Discover(DiscoverArgs::from_arg_matches(discover)?)
            }
            _ => return Ok(None),
        },
        Some(("feedback", feedback)) => match feedback.subcommand() {
            Some(("bundle", bundle)) => {
                let args = FeedbackBundleArgs::from_arg_matches(bundle)?;
                if args.json {
                    output = OutputFormat::Json;
                }
                ReadOnlyCommand::Bundle(args)
            }
            Some(("file", file)) => {
                let args = FeedbackFileArgs::from_arg_matches(file)?;
                args.validate().map_err(|message| {
                    Args::command().error(clap::error::ErrorKind::ValueValidation, message)
                })?;
                if args.yes {
                    return Ok(None);
                }
                if args.json {
                    output = OutputFormat::Json;
                }
                ReadOnlyCommand::FeedbackPlan(args)
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    let select = matches
        .get_many::<String>("select")
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    Ok(Some((command, output, select)))
}

/// Return `None` only for a valid command using the existing execution path.
///
/// Call this before `cli::run`; calling the latter directly retains its existing
/// startup update behavior and is not a read-only inspection entrypoint.
pub async fn run() -> anyhow::Result<Option<ExitCode>> {
    let parsed = match read_only_args(std::env::args_os()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let code = if error.exit_code() == 0 {
                ExitCode::Success
            } else {
                ExitCode::InvalidInput
            };
            error.print()?;
            return Ok(Some(code));
        }
    };
    let Some((command, output, select)) = parsed else {
        return Ok(None);
    };
    let writer = OutputWriter::new(output);
    let result = match command {
        ReadOnlyCommand::Update(args) => {
            let result = run_inspection(args).await?;
            writer.write_data(&select_fields(result.value, &select))?;
            return Ok(Some(result.exit_code));
        }
        ReadOnlyCommand::Discover(args) => {
            let result = run_discovery(args).await?;
            writer.write_data(&select_fields(result.value, &select))?;
            return Ok(Some(result.exit_code));
        }
        ReadOnlyCommand::Bundle(args) => {
            use crate::application::feedback_bundle::{
                FeedbackBundleRequest, FeedbackBundleService,
            };
            FeedbackBundleService
                .bundle(FeedbackBundleRequest {
                    finding_path: args.finding,
                    install_report_path: args.install_report,
                    log_dir: args.log_dir,
                    dcc_pid: args.dcc_pid,
                    host_error_lines: usize::from(args.host_error_lines),
                })
                .await
                .map_err(|_| "DIAGNOSTIC_PREVIEW_FAILED")
                .and_then(|bundle| {
                    serde_json::to_value(bundle).map_err(|_| "DIAGNOSTIC_PREVIEW_FAILED")
                })
        }
        ReadOnlyCommand::FeedbackPlan(args) => {
            use crate::application::feedback_file::FeedbackFileService;
            use crate::infra::github_issues::GhFeedbackIssueTracker;
            FeedbackFileService::new(&GhFeedbackIssueTracker::default())
                .file(args.into_request())
                .map_err(|_| "FEEDBACK_PREVIEW_FAILED")
                .and_then(|plan| serde_json::to_value(plan).map_err(|_| "FEEDBACK_PREVIEW_FAILED"))
        }
    };
    match result {
        Ok(value) => {
            writer.write_data(&select_fields(value, &select))?;
            Ok(Some(ExitCode::Success))
        }
        Err(code) => {
            writer.write_error(&ErrorEnvelope::new(code,
                "Preview failed. Check the explicitly selected files and, for duplicate lookup, official issue-tracker access. No issue was submitted.",
                ExitCode::Unavailable))?;
            Ok(Some(ExitCode::Unavailable))
        }
    }
}

#[cfg(test)]
#[path = "read_only_tests.rs"]
mod tests;
