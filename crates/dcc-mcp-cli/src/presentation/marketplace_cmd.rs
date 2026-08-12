use std::path::PathBuf;

use anyhow::Context;
use serde_json::Value;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum MarketplaceAction {
    Add {
        source: String,
    },
    List,
    Search {
        #[arg(short, long, conflicts_with = "query_terms")]
        query: Option<String>,
        #[arg(value_name = "QUERY", num_args = 1.., conflicts_with = "query")]
        query_terms: Vec<String>,
        #[arg(long, visible_alias = "dcc-type")]
        dcc: Option<String>,
        #[arg(long, conflicts_with = "dcc")]
        target: Option<String>,
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        skip_validation: bool,
    },
    Inspect {
        name: String,
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long)]
        skip_validation: bool,
    },
    Install {
        name: String,
        #[arg(long)]
        dcc: Option<String>,
        #[arg(long, conflicts_with = "dcc")]
        target: Option<String>,
        #[arg(long)]
        reload: bool,
        #[arg(long = "source")]
        sources: Vec<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        skip_validation: bool,
    },
    Uninstall {
        name: String,
        #[arg(long)]
        dcc: Option<String>,
        #[arg(long, conflicts_with = "dcc")]
        target: Option<String>,
        #[arg(long)]
        reload: bool,
    },
    ListInstalled {
        #[arg(long)]
        dcc: Option<String>,
        #[arg(long, conflicts_with = "dcc")]
        target: Option<String>,
    },
    Outdated {
        #[arg(long)]
        dcc: Option<String>,
        names: Vec<String>,
    },
    Update {
        name: Option<String>,
        #[arg(long, short = 'a')]
        all: bool,
        #[arg(long)]
        dcc: Option<String>,
    },
    AddRepo {
        repo_ref: String,
        #[arg(long)]
        dcc: Option<String>,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        force: bool,
    },
    Pack(MarketplacePackArgs),
    Publish(Box<MarketplacePublishArgs>),
}

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
    /// Generic target in KIND:ID form. Repeat for multi-target packages.
    #[arg(long = "target")]
    pub(crate) targets: Vec<String>,
    /// Typed package format.
    #[arg(long = "format")]
    pub(crate) package_format: Option<String>,
    /// Typed component in KIND:ID=ROOT form. Repeat for composite packages.
    #[arg(long = "component")]
    pub(crate) components: Vec<String>,
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
    /// Marketplace showcase image path or absolute URL.
    #[arg(long)]
    pub(crate) showcase: Option<String>,
    /// Required environment variable name. Declared only; never installed. Repeat as needed.
    #[arg(long = "requires-env")]
    pub(crate) requires_env: Vec<String>,
    /// Required executable name on PATH. Declared only; never installed. Repeat as needed.
    #[arg(long = "requires-bin")]
    pub(crate) requires_bin: Vec<String>,
    /// Required Python package or import name. Declared only; never installed. Repeat as needed.
    #[arg(long = "requires-python")]
    pub(crate) requires_python: Vec<String>,
    /// Required DCC-MCP skill name. Declared only; never installed. Repeat as needed.
    #[arg(long = "requires-skill")]
    pub(crate) requires_skill: Vec<String>,
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
    let targets = args
        .targets
        .iter()
        .map(|value| {
            dcc_mcp_marketplace::parse_target(value)
                .map_err(|_| anyhow::anyhow!("invalid target '{value}'; expected KIND:ID"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let package_format = args
        .package_format
        .as_deref()
        .map(parse_package_format)
        .transpose()?;
    let components = args
        .components
        .iter()
        .map(|value| parse_component(value))
        .collect::<anyhow::Result<Vec<_>>>()?;
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
            targets,
            package_format,
            components,
            version: args.version,
            maintainer: args.maintainer,
            tags: args.tags,
            min_core_version: args.min_core_version,
            homepage_url: args.homepage_url,
            icon: args.icon,
            showcase: args.showcase,
            requires_env: args.requires_env,
            requires_bin: args.requires_bin,
            requires_python: args.requires_python,
            requires_skill: args.requires_skill,
        },
    )?;
    to_json(result)
}

fn parse_package_format(value: &str) -> anyhow::Result<dcc_mcp_catalog::CatalogPackageFormat> {
    use dcc_mcp_catalog::CatalogPackageFormat;
    match value {
        "skill" => Ok(CatalogPackageFormat::Skill),
        "skill-bundle" => Ok(CatalogPackageFormat::SkillBundle),
        "agent-plugin" => Ok(CatalogPackageFormat::AgentPlugin),
        "cua-profile" => Ok(CatalogPackageFormat::CuaProfile),
        "composite" => Ok(CatalogPackageFormat::Composite),
        _ => anyhow::bail!("invalid package format '{value}'"),
    }
}

fn parse_component(value: &str) -> anyhow::Result<dcc_mcp_catalog::CatalogComponent> {
    use dcc_mcp_catalog::CatalogComponentKind;
    let (identity, root) = value
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("invalid component '{value}'; expected KIND:ID=ROOT"))?;
    let (kind, id) = identity
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid component '{value}'; expected KIND:ID=ROOT"))?;
    let kind = match kind {
        "skill" => CatalogComponentKind::Skill,
        "cua-profile" => CatalogComponentKind::CuaProfile,
        _ => anyhow::bail!("invalid component kind '{kind}'"),
    };
    if id.is_empty() || root.is_empty() {
        anyhow::bail!("invalid component '{value}'; id and root are required");
    }
    Ok(dcc_mcp_catalog::CatalogComponent {
        kind,
        id: id.to_string(),
        root: root.to_string(),
    })
}

fn to_json(value: impl serde::Serialize) -> anyhow::Result<Value> {
    serde_json::to_value(value).context("failed to serialize command output")
}
