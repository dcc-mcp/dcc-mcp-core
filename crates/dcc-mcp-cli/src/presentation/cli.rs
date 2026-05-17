use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::Value;

use crate::application::client::DccMcpClient;
use crate::application::install::InstallService;
use crate::domain::install::InstallRequest;
use crate::domain::rest::{CallRequest, DescribeRequest, Endpoint, SearchRequest};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9765";

#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-cli", about, version)]
pub struct Args {
    #[arg(long, env = "DCC_MCP_BASE_URL", default_value = DEFAULT_BASE_URL)]
    base_url: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    output: OutputFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Pretty,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the configured gateway or per-DCC REST endpoint.
    Health,
    /// List live DCC instances from the gateway.
    List,
    /// Search callable tools through the REST dynamic-capability surface.
    Search {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        dcc_type: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Describe one tool slug.
    Describe { tool_slug: String },
    /// Invoke one tool slug.
    Call {
        tool_slug: String,
        #[arg(long = "json", default_value = "{}")]
        arguments_json: String,
        #[arg(long)]
        meta_json: Option<String>,
    },
    /// Build an auditable DCC adapter installation plan.
    Install {
        #[arg(long)]
        dcc_type: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, env = "DCC_MCP_CATALOG_PATH")]
        catalog: Option<PathBuf>,
    },
}

pub async fn run() -> anyhow::Result<()> {
    run_with_args(Args::parse()).await
}

async fn run_with_args(args: Args) -> anyhow::Result<()> {
    let value = match args.command {
        Command::Health => {
            let client = DccMcpClient::new(Endpoint::new(args.base_url));
            client.health().await?
        }
        Command::List => {
            let client = DccMcpClient::new(Endpoint::new(args.base_url));
            client.list_instances().await?
        }
        Command::Search {
            query,
            dcc_type,
            limit,
        } => {
            let client = DccMcpClient::new(Endpoint::new(args.base_url));
            client
                .search(SearchRequest {
                    query,
                    dcc_type,
                    limit,
                })
                .await?
        }
        Command::Describe { tool_slug } => {
            let client = DccMcpClient::new(Endpoint::new(args.base_url));
            client.describe(DescribeRequest { tool_slug }).await?
        }
        Command::Call {
            tool_slug,
            arguments_json,
            meta_json,
        } => {
            let arguments = parse_json_object(&arguments_json, "--json")?;
            let meta = meta_json
                .as_deref()
                .map(|raw| parse_json_object(raw, "--meta-json"))
                .transpose()?;
            let client = DccMcpClient::new(Endpoint::new(args.base_url));
            client
                .call(CallRequest {
                    tool_slug,
                    arguments,
                    meta,
                })
                .await?
        }
        Command::Install {
            dcc_type,
            version,
            catalog,
        } => {
            let service = InstallService::new(PathBuf::from("dcc-mcp-catalog.yml"));
            to_json(service.plan(InstallRequest {
                dcc_type,
                version,
                catalog_path: catalog,
            })?)?
        }
    };

    print_value(&value, args.output)
}

fn parse_json_object(raw: &str, flag_name: &str) -> anyhow::Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{flag_name} must be valid JSON"))?;
    if value.is_object() {
        Ok(value)
    } else {
        anyhow::bail!("{flag_name} must be a JSON object")
    }
}

fn to_json(value: impl Serialize) -> anyhow::Result<Value> {
    serde_json::to_value(value).context("failed to serialize command output")
}

fn print_value(value: &Value, output: OutputFormat) -> anyhow::Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Pretty => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}
