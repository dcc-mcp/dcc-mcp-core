//! Thin companion adapter: mcpc owns MCP transport, sessions and discovery.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub async fn execute(
    node: &Path,
    entry: &Path,
    state_dir: &Path,
    arguments: &[String],
    timeout: Duration,
) -> Result<Value> {
    tokio::time::timeout(timeout, execute_with_budget(node, entry, state_dir, arguments, timeout))
        .await
        .context("MCP companion timed out; do not retry an uncertain write; inspect/close the task-owned session")?
}

async fn execute_with_budget(
    node: &Path,
    entry: &Path,
    state_dir: &Path,
    arguments: &[String],
    timeout: Duration,
) -> Result<Value> {
    if !entry.is_absolute() || !entry.is_file() || !state_dir.is_absolute() {
        bail!(
            "mcp requires an existing absolute --client-entry and absolute task-owned --state-dir; no automatic install"
        );
    }
    let first = arguments.first().map(String::as_str).unwrap_or_default();
    if first == "connect" {
        if arguments.len() < 3 || !arguments[2].starts_with('@') || arguments[1].starts_with('-') {
            bail!(
                "connect requires an explicit reviewed endpoint/config and @session; automatic configuration discovery is disabled"
            );
        }
    } else if first == "close" {
        if !arguments
            .get(1)
            .is_some_and(|argument| argument.starts_with('@'))
        {
            bail!("close requires an explicit task-owned @session");
        }
    } else if !first.starts_with('@') {
        bail!(
            "mcp accepts explicit connect/close or @session operations; it does not manage accounts, profiles or global configuration"
        );
    }
    if first.starts_with('@') {
        let operation = arguments.get(1).map(String::as_str).unwrap_or_default();
        if operation.starts_with('-') {
            bail!(
                "place the session operation immediately after @session; output/timeout flags belong to dcc-mcp-cli"
            );
        }
        let capability = if operation.starts_with("resources-") {
            Some("resources")
        } else if operation.starts_with("prompts-") {
            Some("prompts")
        } else if operation.starts_with("tools-") {
            Some("tools")
        } else {
            None
        };
        if let Some(capability) = capability {
            let info = invoke(node, entry, state_dir, &[first.to_owned()], timeout).await?;
            require_capability(&info, capability)?;
        }
    }
    invoke(node, entry, state_dir, arguments, timeout).await
}

fn require_capability(info: &Value, capability: &str) -> Result<()> {
    if !info
        .get("capabilities")
        .and_then(Value::as_object)
        .is_some_and(|capabilities| capabilities.get(capability).is_some_and(Value::is_object))
    {
        bail!(
            "MCP_CAPABILITY_UNSUPPORTED: server did not advertise {capability}; operation was not sent"
        );
    }
    Ok(())
}

async fn invoke(
    node: &Path,
    entry: &Path,
    state_dir: &Path,
    arguments: &[String],
    timeout: Duration,
) -> Result<Value> {
    let mut command = tokio::process::Command::new(node);
    command
        .arg(entry)
        .arg("--json")
        .args(arguments)
        .env("MCPC_HOME_DIR", state_dir)
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW.
        // mcpc's retained bridge is also task-owned; hide its detached console.
        tokio::fs::create_dir_all(state_dir).await?;
        let preload = state_dir.join("dcc-mcp-windows-hide.cjs");
        let source = include_bytes!("../../../../scripts/mcp_interop_windows_hide.cjs");
        if preload.exists() {
            if tokio::fs::read(&preload).await?.as_slice() != source {
                bail!("task-owned Windows helper differs; refusing to overwrite it");
            }
        } else {
            tokio::fs::write(&preload, source).await?;
        }
        let previous = std::env::var("NODE_OPTIONS").unwrap_or_default();
        command.env(
            "NODE_OPTIONS",
            format!(
                "{previous} --require=\"{}\"",
                preload.to_string_lossy().replace('\\', "/")
            ),
        );
    }
    let output = tokio::time::timeout(timeout, command.output()).await
        .context("MCP companion timed out; do not retry an uncertain write; inspect/close the task-owned session")?
        .context("could not execute the explicit MCP companion; no fallback or installation attempted")?;
    if !output.status.success() {
        bail!(
            "MCP companion exited {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout)
        .context("MCP companion did not return structured JSON; output was not treated as success")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn missing_capability_is_not_a_successful_empty_inventory() {
        assert!(require_capability(&json!({"capabilities":{"resources":{}}}), "prompts").is_err());
        assert!(require_capability(&json!({"capabilities":{"prompts":{}}}), "prompts").is_ok());
        assert!(require_capability(&json!({}), "resources").is_err());
    }
}
