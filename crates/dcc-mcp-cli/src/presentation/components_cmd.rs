use anyhow::Context;
use clap::Subcommand;
use serde_json::Value;

use crate::application::components::ComponentService;

#[derive(Debug, Subcommand)]
pub(crate) enum ComponentsAction {
    /// Inspect one installed companion executable without downloading anything.
    Status {
        #[arg(value_parser = ["dcc-cua"])]
        component: String,
    },
    /// Install or reconcile one companion executable from its official manifest.
    Ensure {
        #[arg(value_parser = ["dcc-cua"])]
        component: String,
        /// Install an exact stable release instead of the latest release.
        #[arg(long)]
        version: Option<String>,
        /// Confirm the filesystem mutation. This command never prompts.
        #[arg(long)]
        yes: bool,
    },
}

pub(crate) async fn run(action: ComponentsAction) -> anyhow::Result<Value> {
    let service = ComponentService::for_current_process()
        .context("failed to initialize component installer")?;
    match action {
        ComponentsAction::Status { component } => {
            debug_assert_eq!(component, "dcc-cua");
            service.status()
        }
        ComponentsAction::Ensure {
            component,
            version,
            yes,
        } => {
            debug_assert_eq!(component, "dcc-cua");
            if !yes {
                anyhow::bail!("components ensure requires explicit --yes consent");
            }
            service.ensure(version.as_deref()).await
        }
    }
}
