use anyhow::{Context, Result};
use hybridsearch_mcp::config::Config;
use hybridsearch_mcp::help::{help, parse_topic, render_cli};
use hybridsearch_mcp::mcp::HybridSearchServer;
use hybridsearch_mcp::service::SearchService;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [argument] if matches!(argument.as_str(), "--version" | "-V") => {
            println!("hybrid-search {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [argument] if matches!(argument.as_str(), "--help" | "-h" | "help") => {
            println!("{}", render_cli(&help(None)));
            return Ok(());
        }
        [command, topic] if command == "help" => {
            print_topic_help(topic)?;
            return Ok(());
        }
        [topic, command] if command == "help" => {
            print_topic_help(topic)?;
            return Ok(());
        }
        [argument, ..] => anyhow::bail!("unknown argument: {argument}; run `hybrid-search help`"),
        [] => {}
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

fn print_topic_help(topic: &str) -> Result<()> {
    let topic = parse_topic(topic)
        .with_context(|| format!("unknown help topic: {topic}; run `hybrid-search help`"))?;
    println!("{}", render_cli(&help(Some(topic))));
    Ok(())
}
