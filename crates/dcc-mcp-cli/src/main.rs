use dcc_mcp_cli::presentation::cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
