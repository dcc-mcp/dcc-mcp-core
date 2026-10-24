use anyhow::Context;
use clap::Subcommand;
use serde_json::Value;
use std::path::PathBuf;

use crate::application::components::ComponentService;

#[derive(Debug, Subcommand)]
pub(crate) enum ComponentsAction {
    /// Install an externally qualified local package with exact byte admission.
    InstallVerified(Box<VerifiedInstallArgs>),
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

#[derive(Debug, clap::Args)]
pub(crate) struct VerifiedInstallArgs {
    #[arg(value_parser = ["dcc-cua"])]
    component: String,
    #[arg(long)]
    target_core_cli: PathBuf,
    #[arg(long)]
    expected_core_cli_sha256: String,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    manifest_sha256: String,
    #[arg(long)]
    previous_manifest: PathBuf,
    #[arg(long)]
    previous_manifest_sha256: String,
    #[arg(long)]
    source_root: PathBuf,
    #[arg(long)]
    version: String,
    #[arg(long)]
    expected_current_sha256: String,
    #[arg(long)]
    expected_new_sha256: String,
    /// Confirm the filesystem mutation. This command never prompts.
    #[arg(long)]
    yes: bool,
}

fn run_verified(args: VerifiedInstallArgs) -> anyhow::Result<Value> {
    if !args.yes {
        anyhow::bail!("components install-verified requires explicit --yes consent");
    }
    crate::application::components::verified_local::install(
        crate::application::components::verified_local::VerifiedInstallRequest {
            target_core_cli: args.target_core_cli,
            expected_core_cli_sha256: args.expected_core_cli_sha256,
            manifest: args.manifest,
            manifest_sha256: args.manifest_sha256,
            previous_manifest: args.previous_manifest,
            previous_manifest_sha256: args.previous_manifest_sha256,
            source_root: args.source_root,
            version: args.version,
            expected_current_sha256: args.expected_current_sha256,
            expected_new_sha256: args.expected_new_sha256,
        },
    )
}

pub(crate) async fn run(action: ComponentsAction) -> anyhow::Result<Value> {
    match action {
        ComponentsAction::InstallVerified(args) => run_verified(*args),
        ComponentsAction::Status { component } => {
            debug_assert_eq!(component, "dcc-cua");
            ComponentService::for_current_process()
                .context("failed to initialize component installer")?
                .status()
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
            ComponentService::for_current_process()
                .context("failed to initialize component installer")?
                .ensure(version.as_deref())
                .await
        }
    }
}
