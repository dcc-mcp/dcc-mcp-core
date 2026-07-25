use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dcc_mcp_cli::presentation::cli;
use dcc_mcp_cli::presentation::output::{ErrorEnvelope, ExitCode, OutputWriter};

#[tokio::main]
async fn main() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();

    // SIGINT handler → exit code 5 (Cancelled) per ADR 018.
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancelled_clone.store(true, Ordering::SeqCst);
        // Write cancellation error to stderr and exit immediately.
        let writer = OutputWriter::new(dcc_mcp_cli::presentation::output::OutputFormat::Human);
        let envelope = ErrorEnvelope::new(
            "CANCELLED",
            "operation cancelled by signal (SIGINT)",
            ExitCode::Cancelled,
        );
        let _ = writer.write_error(&envelope);
        std::process::exit(ExitCode::Cancelled.as_i32());
    });

    // Check for cancellation before running.
    if cancelled.load(Ordering::SeqCst) {
        std::process::exit(ExitCode::Cancelled.as_i32());
    }

    let result = cli::run().await;

    match result {
        Ok(()) => std::process::exit(ExitCode::Success.as_i32()),
        Err(e) => {
            // Error already handled in run_with_args; fallback for uncaught errors.
            let writer = OutputWriter::new(dcc_mcp_cli::presentation::output::OutputFormat::Human);
            let envelope =
                ErrorEnvelope::new("INTERNAL_ERROR", format!("{e:#}"), ExitCode::GeneralError);
            let _ = writer.write_error(&envelope);
            std::process::exit(ExitCode::GeneralError.as_i32());
        }
    }
}
