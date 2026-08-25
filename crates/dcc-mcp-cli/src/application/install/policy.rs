use std::io::{BufRead, Write};

use crate::domain::install::InstallPlan;

use super::InstallError;

const INSTALL_DISABLED_ENV: &str = "DCC_MCP_INSTALL_DISABLED";
const INSTALL_DISABLED_PROMPT_ENV: &str = "DCC_MCP_INSTALL_DISABLED_PROMPT";
const DEFAULT_INSTALL_DISABLED_PROMPT: &str = "Automatic DCC adapter installation is disabled in this environment. Ask your Pipeline TD or studio deployment owner to deploy {adapter} for {dcc_type}, then start the DCC plugin and rerun `dcc-mcp-cli list`.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AutoInstallPolicy {
    pub(super) enabled: bool,
    pub(super) prompt_template: String,
}

impl AutoInstallPolicy {
    pub(super) fn from_env() -> Self {
        let disabled = std::env::var(INSTALL_DISABLED_ENV)
            .ok()
            .is_some_and(|value| env_flag_enabled(&value));
        let prompt_template = std::env::var(INSTALL_DISABLED_PROMPT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_INSTALL_DISABLED_PROMPT.to_string());
        Self {
            enabled: !disabled,
            prompt_template,
        }
    }

    #[cfg(test)]
    pub(super) fn disabled(prompt_template: impl Into<String>) -> Self {
        Self {
            enabled: false,
            prompt_template: prompt_template.into(),
        }
    }
}

pub(super) fn render_install_policy_prompt(template: &str, plan: &InstallPlan) -> String {
    template
        .replace("{adapter}", &plan.adapter.name)
        .replace("{dcc_type}", &plan.dcc_type)
        .replace("{version}", plan.version.as_deref().unwrap_or(""))
}

/// Prompt the user for Y/n consent. Returns `true` if the user agrees.
pub(super) fn ask_consent(prompt: &str) -> Result<bool, InstallError> {
    let stdin = std::io::stdin();
    let mut stderr = std::io::stderr();

    loop {
        write!(stderr, "{prompt} ")?;
        stderr.flush()?;

        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let trimmed = line.trim().to_lowercase();

        match trimmed.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                write!(stderr, "  Please answer Y or n: ")?;
                stderr.flush()?;
            }
        }
    }
}

fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "disabled" | "disable"
    )
}
