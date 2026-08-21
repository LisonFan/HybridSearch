mod chatgpt2api;
mod exa;
mod firecrawl;
mod http;
mod tavily;
mod tinyfish;

use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use async_trait::async_trait;
use reqwest::Client;
use std::sync::Arc;

pub use chatgpt2api::{ChatSearchResult, Chatgpt2apiProvider};
pub use exa::ExaProvider;
pub use firecrawl::FirecrawlProvider;
pub use tavily::TavilyProvider;
pub use tinyfish::TinyfishProvider;

pub fn build_http_client(timeout: std::time::Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
        .gzip(true)
        .pool_idle_timeout(Some(std::time::Duration::from_secs(90)))
        .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
        .tcp_nodelay(true)
        .build()
        .map_err(|error| {
            HybridSearchError::Provider(format!("failed to build HTTP client: {error}"))
        })
}

#[async_trait]
pub trait SourceProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports_filters(&self) -> bool;

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>>;

    async fn fetch(&self, url: &str) -> Result<FetchedPage>;

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Err(HybridSearchError::Provider(format!(
            "{} does not support URL mapping",
            self.name()
        )))
    }
}

pub type SharedSourceProvider = Arc<dyn SourceProvider>;
