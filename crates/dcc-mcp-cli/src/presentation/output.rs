//! ADR 018 CLI output contract: unified error envelope, semantic exit codes,
//! four-channel output (human/json/ndjson/toon), and stdout/stderr separation.
//!
//! ## Normative layers
//! 1. `OutputFormat` — `Human`, `Json`, `Ndjson`, `Toon` + TTY auto-detection
//! 2. `OutputWriter` — stdout=data, stderr=diagnostics
//! 3. `ExitCode` — semantic 0-7 per ADR 018
//! 4. `ErrorEnvelope` — unified structured error payload

use std::io::{IsTerminal, Write};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

pub(crate) fn to_json(value: impl Serialize) -> anyhow::Result<Value> {
    serde_json::to_value(value).context("failed to serialize command output")
}

pub(crate) fn exit_code_to_error_code(exit_code: ExitCode) -> &'static str {
    match exit_code {
        ExitCode::Success => "OK",
        ExitCode::GeneralError => "GENERAL_ERROR",
        ExitCode::InvalidInput => "INVALID_INPUT",
        ExitCode::Unavailable => "UNAVAILABLE",
        ExitCode::Timeout => "TIMEOUT",
        ExitCode::Cancelled => "CANCELLED",
        ExitCode::PermissionDenied => "PERMISSION_DENIED",
        ExitCode::Conflict => "CONFLICT",
    }
}

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// Output format for humans, scripts, streams, and token-efficient agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable (default when TTY detected).
    Human,
    /// Compact machine-readable JSON on stdout.
    Json,
    /// Newline-delimited JSON stream on stdout.
    Ndjson,
    /// Compact TOON for token-efficient agent consumption.
    Toon,
}

impl OutputFormat {
    /// Parse from `--output` flag value.
    pub fn from_flag(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "human" | "pretty" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "ndjson" => Ok(Self::Ndjson),
            "toon" => Ok(Self::Toon),
            other => Err(format!(
                "invalid output format '{other}'; expected human, json, ndjson, or toon"
            )),
        }
    }

    /// Auto-detect: `Human` when stdout is a TTY, `Json` otherwise.
    pub fn auto_detect() -> Self {
        if std::io::stdout().is_terminal() {
            Self::Human
        } else {
            Self::Json
        }
    }
}

// ---------------------------------------------------------------------------
// ExitCode
// ---------------------------------------------------------------------------

/// Semantic exit codes per ADR 018 § Exit Codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// 0 — command completed successfully.
    Success = 0,
    /// 1 — general / unclassified error.
    GeneralError = 1,
    /// 2 — invalid user input (bad args, missing required value).
    InvalidInput = 2,
    /// 3 — remote service unavailable (connection refused, 503, gateway down).
    Unavailable = 3,
    /// 4 — operation timed out.
    Timeout = 4,
    /// 5 — cancelled by signal (SIGINT / Ctrl-C).
    Cancelled = 5,
    /// 6 — permission denied / forbidden (401, 403, ACL reject).
    PermissionDenied = 6,
    /// 7 — resource conflict / precondition failed (409, duplicate).
    Conflict = 7,
}

impl ExitCode {
    /// Map from HTTP status code ranges.
    pub fn from_http_status(status: u16) -> Self {
        match status {
            200..=299 => Self::Success,
            400 => Self::InvalidInput,
            401 | 403 => Self::PermissionDenied,
            404 => Self::InvalidInput,
            408 => Self::Timeout,
            409 => Self::Conflict,
            429 => Self::Unavailable,
            500..=599 => Self::Unavailable,
            _ => Self::GeneralError,
        }
    }

    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

// ---------------------------------------------------------------------------
// ErrorEnvelope
// ---------------------------------------------------------------------------

/// Unified error envelope per ADR 018 § Error Envelope.
///
/// All CLI error output is serialised through this struct and written to
/// stderr (human) or stdout (json/ndjson).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    /// Machine-readable error code (e.g. "INVALID_INPUT").
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Semantic exit code (0-7).
    pub exit_code: i32,
    /// Whether the caller can safely retry.
    pub retryable: bool,
    /// Optional structured details (stack, input, request id, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ErrorEnvelope {
    pub fn new(code: impl Into<String>, message: impl Into<String>, exit_code: ExitCode) -> Self {
        Self {
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
                exit_code: exit_code.as_i32(),
                retryable: false,
                details: None,
            },
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.error.retryable = retryable;
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.error.details = Some(details);
        self
    }
}

// ---------------------------------------------------------------------------
// OutputWriter
// ---------------------------------------------------------------------------

/// Contract-aware writer: routes data to stdout and diagnostics to stderr.
///
/// - **Human mode**: pretty-printed data on stdout, errors on stderr.
/// - **Json mode**: compact JSON on stdout, errors on stderr.
/// - **Ndjson mode**: one JSON object per line on stdout, errors on stderr.
/// - **Toon mode**: compact TOON on stdout and stderr for agent consumption.
pub struct OutputWriter {
    format: OutputFormat,
}

impl OutputWriter {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Write successful data to stdout.
    pub fn write_data(&self, value: &Value) -> anyhow::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match self.format {
            OutputFormat::Human if is_list_payload(value) => print_list_pretty(value),
            format => {
                writeln!(stdout, "{}", serialize_value(format, value)?)?;
            }
        }
        stdout.flush()?;
        Ok(())
    }

    /// Write an error envelope to stderr (always, per ADR 018 stdout/stderr
    /// separation: stdout=data, stderr=diagnostics including errors).
    ///
    /// In json/ndjson mode the envelope is still written as compact JSON to
    /// stderr so consuming tooling can capture it separately from data on
    /// stdout.
    pub fn write_error(&self, envelope: &ErrorEnvelope) -> anyhow::Result<()> {
        let payload = match self.format {
            OutputFormat::Toon => toon_format::encode_default(&serde_json::to_value(envelope)?)?,
            _ => serde_json::to_string(&envelope)?,
        };
        let mut stderr = std::io::stderr().lock();
        match self.format {
            OutputFormat::Human => {
                writeln!(
                    stderr,
                    "error [{}]: {}",
                    envelope.error.code, envelope.error.message
                )?;
                if let Some(ref details) = envelope.error.details {
                    writeln!(
                        stderr,
                        "  details: {}",
                        serde_json::to_string_pretty(details)?
                    )?;
                }
            }
            OutputFormat::Json | OutputFormat::Ndjson | OutputFormat::Toon => {
                writeln!(stderr, "{payload}")?;
            }
        }
        stderr.flush()?;
        Ok(())
    }

    /// Write a diagnostic message (always to stderr regardless of format).
    pub fn diagnostic(&self, msg: &str) -> anyhow::Result<()> {
        let mut stderr = std::io::stderr().lock();
        writeln!(stderr, "{msg}")?;
        stderr.flush()?;
        Ok(())
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }
}

fn serialize_value(format: OutputFormat, value: &Value) -> anyhow::Result<String> {
    match format {
        OutputFormat::Human => Ok(serde_json::to_string_pretty(value)?),
        OutputFormat::Json | OutputFormat::Ndjson => Ok(serde_json::to_string(value)?),
        OutputFormat::Toon => Ok(toon_format::encode_default(value)?),
    }
}

// ---------------------------------------------------------------------------
// Pretty-print helpers (human mode)
// ---------------------------------------------------------------------------

fn is_list_payload(value: &Value) -> bool {
    value.get("instances").is_some() && value.get("gateway").is_some()
}

fn print_list_pretty(value: &Value) {
    let gateway = value.get("gateway").unwrap_or(&Value::Null);
    println!("Gateway");
    if let Some(current) = gateway.get("current").filter(|v| !v.is_null()) {
        println!(
            "  owner      {}",
            gateway_summary(
                current,
                current
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("active")
            )
        );
    } else if let Some(error) = gateway.get("error").and_then(Value::as_str) {
        println!("  owner      unknown ({error})");
    } else {
        println!("  owner      unknown");
    }

    let candidates = gateway
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if candidates.is_empty() {
        println!("  candidates none");
    } else {
        println!("  candidates");
        for candidate in candidates {
            println!("    {}", gateway_summary(&candidate, "challenger"));
        }
    }

    println!();
    println!("Instances");
    let instances = value
        .get("instances")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if instances.is_empty() {
        println!("  none");
        return;
    }
    for instance in instances {
        let dcc = instance
            .get("dcc_type")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let short = instance
            .get("instance_short")
            .or_else(|| instance.get("instance_id"))
            .and_then(Value::as_str)
            .unwrap_or("-");
        let name = instance
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let pid = instance
            .get("pid")
            .and_then(Value::as_u64)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        let status = instance
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("available");
        let mcp_url = instance
            .get("mcp_url")
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("  {dcc:<12} {short:<12} {status:<12} pid={pid:<8} name={name} mcp={mcp_url}");
    }
}

fn gateway_summary(value: &Value, fallback_role: &str) -> String {
    let name = value
        .get("name")
        .or_else(|| value.get("display_name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(fallback_role);
    let pid = value
        .get("pid")
        .and_then(Value::as_u64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let dcc = value
        .get("adapter_dcc")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let version = value
        .get("adapter_version")
        .or_else(|| value.get("version"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let host = value.get("host").and_then(Value::as_str).unwrap_or("-");
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("{name} role={role} pid={pid} dcc={dcc} version={version} addr={host}:{port}")
}

#[cfg(test)]
mod tests;
