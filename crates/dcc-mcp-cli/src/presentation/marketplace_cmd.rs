use std::path::PathBuf;

use anyhow::Context;
use serde_json::Value;

#[derive(Debug, clap::Args)]
pub(crate) struct MarketplacePackArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    /// Output zip path or output directory. Defaults to ../<package>.zip.
    #[arg(long)]
    pub(crate) out: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct MarketplacePublishArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    /// Local marketplace.json path to update.
    #[arg(long)]
    pub(crate) catalog: PathBuf,
    /// URL users will install from, usually a GitHub Release zip asset.
    #[arg(long = "install-url")]
    pub(crate) install_url: String,
    /// Install source type.
    #[arg(long = "install-type", default_value = "zip")]
    pub(crate) install_type: String,
    /// Git ref/tag for git installs.
    #[arg(long = "install-ref")]
    pub(crate) install_ref: Option<String>,
    /// Skill directories to install from the source. Repeat for multi-skill packages.
    #[arg(long = "skill-root")]
    pub(crate) skill_roots: Vec<String>,
    /// Archive SHA-256, optionally prefixed with sha256:.
    #[arg(long)]
    pub(crate) sha256: Option<String>,
    /// Override package name when PATH has no root SKILL.md.
    #[arg(long)]
    pub(crate) name: Option<String>,
    /// Override package description when PATH has no root SKILL.md.
    #[arg(long)]
    pub(crate) description: Option<String>,
    /// Target DCC. Repeat for multi-DCC packages.
    #[arg(long)]
    pub(crate) dcc: Vec<String>,
    #[arg(long)]
    pub(crate) version: Option<String>,
    #[arg(long)]
    pub(crate) maintainer: Option<String>,
    /// Extra searchable tag. Repeat as needed.
    #[arg(long = "tag")]
    pub(crate) tags: Vec<String>,
    #[arg(long = "min-core-version")]
    pub(crate) min_core_version: Option<String>,
    #[arg(long = "homepage-url")]
    pub(crate) homepage_url: Option<String>,
    #[arg(long)]
    pub(crate) icon: Option<String>,
}

pub(crate) fn run_pack(args: MarketplacePackArgs) -> anyhow::Result<Value> {
    let result = dcc_mcp_marketplace::pack_marketplace_package(
        dcc_mcp_marketplace::MarketplacePackOptions {
            source_dir: args.path,
            out: args.out,
        },
    )?;
    to_json(result)
}

pub(crate) fn run_publish(args: MarketplacePublishArgs) -> anyhow::Result<Value> {
    let result = dcc_mcp_marketplace::publish_marketplace_package(
        dcc_mcp_marketplace::MarketplacePublishOptions {
            package_dir: args.path,
            catalog_path: args.catalog,
            install_url: args.install_url,
            install_type: args.install_type,
            install_ref: args.install_ref,
            skill_roots: args.skill_roots,
            sha256: args.sha256,
            name: args.name,
            description: args.description,
            dcc: args.dcc,
            version: args.version,
            maintainer: args.maintainer,
            tags: args.tags,
            min_core_version: args.min_core_version,
            homepage_url: args.homepage_url,
            icon: args.icon,
        },
    )?;
    to_json(result)
}

fn to_json(value: impl serde::Serialize) -> anyhow::Result<Value> {
    serde_json::to_value(value).context("failed to serialize command output")
}
