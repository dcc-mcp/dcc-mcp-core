//! Parse before the legacy CLI startup path can apply a staged update.
//!
//! This executable entrypoint uses the canonical Clap tree. Parse errors, help,
//! and version requests finish here, including malformed inspection requests.

use std::ffi::OsString;

use clap::{CommandFactory, FromArgMatches};

use super::cli::Args;
use super::output::{ExitCode, OutputFormat, OutputWriter};
use super::update_cmd::{InspectArgs, run_inspection};

fn inspection_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<(InspectArgs, OutputFormat)>, clap::Error> {
    let matches = Args::command().try_get_matches_from(arguments)?;
    let Some(("update", update)) = matches.subcommand() else {
        return Ok(None);
    };
    let Some(("inspect", inspect)) = update.subcommand() else {
        return Ok(None);
    };
    let output = matches
        .get_one::<OutputFormat>("output")
        .copied()
        .unwrap_or_else(OutputFormat::auto_detect);
    Ok(Some((InspectArgs::from_arg_matches(inspect)?, output)))
}

/// Return `None` only for a valid command using the existing execution path.
///
/// Call this before `cli::run`; calling the latter directly retains its existing
/// startup update behavior and is not a read-only inspection entrypoint.
pub async fn run() -> anyhow::Result<Option<ExitCode>> {
    let parsed = match inspection_args(std::env::args_os()) {
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
    let Some((args, output)) = parsed else {
        return Ok(None);
    };
    let result = run_inspection(args).await?;
    OutputWriter::new(output).write_data(&result.value)?;
    Ok(Some(result.exit_code))
}

#[cfg(test)]
#[path = "read_only_tests.rs"]
mod tests;
