use crate::application::client::TransportMode;

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct TransportArgs {
    /// Tool-call transport: auto (REST, then MCP fallback), rest, or mcp.
    #[arg(
        long,
        global = true,
        env = "DCC_MCP_CLI_TRANSPORT",
        value_enum,
        default_value_t = TransportMode::Auto
    )]
    pub(crate) transport: TransportMode,
}
