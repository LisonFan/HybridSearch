use anyhow::{Context, Result};
use hybridsearch_mcp::config::Config;
use hybridsearch_mcp::mcp::HybridSearchServer;
use hybridsearch_mcp::service::SearchService;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("hybrid-search {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "HybridSearch MCP server\n\nUsage: hybrid-search [--help|--version]\n\nThe default command starts an MCP server over stdio."
            );
            return Ok(());
        }
        Some(argument) => anyhow::bail!("unknown argument: {argument}"),
        None => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let config = Config::from_env().context("failed to load HybridSearch configuration")?;
    tracing::info!(
        providers = ?config.effective_provider_order(),
        "starting HybridSearch MCP server"
    );
    let server = HybridSearchServer::new(SearchService::new(config)?);
    let service = server
        .serve(stdio())
        .await
        .context("failed to start MCP stdio transport")?;
    service.waiting().await.context("MCP server stopped")?;
    Ok(())
}
