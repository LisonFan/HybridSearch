use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use crate::providers::SourceProvider;
use crate::providers::http::send_json;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct FirecrawlProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl FirecrawlProvider {
    pub fn new(client: Client, api_url: String, api_key: String) -> Self {
        Self {
            client,
            api_url: api_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        send_json(
            self.client
                .post(format!("{}/{}", self.api_url, path.trim_start_matches('/')))
                .bearer_auth(&self.api_key)
                .json(body),
            "Firecrawl",
        )
        .await
    }
}

#[async_trait]
impl SourceProvider for FirecrawlProvider {
    fn name(&self) -> &'static str {
        "firecrawl"
    }

    fn supports_filters(&self) -> bool {
        false
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let response = self
            .post("search", &json!({ "query": query, "limit": max_results }))
            .await?;
        Ok(normalize_results(&response))
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = self
            .post("scrape", &json!({ "url": url, "formats": ["markdown"] }))
            .await?;
        let data = response.get("data").unwrap_or(&response);
        let content = data
            .get("markdown")
            .or_else(|| data.get("content"))
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                HybridSearchError::Provider("Firecrawl scrape returned empty content".to_string())
            })?;
        let metadata = data.get("metadata");
        Ok(FetchedPage {
            content: content.to_string(),
            title: metadata
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_date: metadata
                .and_then(|value| {
                    value
                        .get("publishedTime")
                        .or_else(|| value.get("article:published_time"))
                })
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

fn normalize_results(response: &Value) -> Vec<Source> {
    response
        .get("data")
        .or_else(|| response.get("results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if let Some(url) = item.as_str() {
                return Some(Source::new(url, "firecrawl"));
            }
            let mut source = Source::new(item.get("url")?.as_str()?, "firecrawl");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item
                .get("description")
                .or_else(|| item.get("markdown"))
                .or_else(|| item.get("content"))
                .and_then(Value::as_str)
            {
                source = source.with_description(description);
            }
            Some(source)
        })
        .collect()
}
