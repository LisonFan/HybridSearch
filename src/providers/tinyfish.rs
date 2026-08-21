use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use crate::providers::SourceProvider;
use crate::providers::http::{get_json, send_json};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct TinyfishProvider {
    client: Client,
    search_api_url: String,
    fetch_api_url: String,
    api_key: String,
}

impl TinyfishProvider {
    pub fn new(
        client: Client,
        search_api_url: String,
        fetch_api_url: String,
        api_key: String,
    ) -> Self {
        Self {
            client,
            search_api_url: search_api_url.trim_end_matches('/').to_string(),
            fetch_api_url: fetch_api_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }
}

#[async_trait]
impl SourceProvider for TinyfishProvider {
    fn name(&self) -> &'static str {
        "tinyfish"
    }

    fn supports_filters(&self) -> bool {
        true
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let mut params = vec![("query", query.to_string())];
        if !filters.include_domains.is_empty() {
            params.push(("include_domains", filters.include_domains.join(",")));
        }
        if !filters.exclude_domains.is_empty() {
            params.push(("exclude_domains", filters.exclude_domains.join(",")));
        }
        if let Some(days) = filters.recency_days {
            params.push((
                "recency_minutes",
                u64::from(days)
                    .saturating_mul(24 * 60)
                    .clamp(1, 5_256_000)
                    .to_string(),
            ));
        }
        let response = get_json(
            self.client
                .get(&self.search_api_url)
                .query(&params)
                .header("X-API-Key", &self.api_key),
            "TinyFish",
        )
        .await?;
        let mut sources = normalize_results(&response);
        sources.truncate(max_results);
        Ok(sources)
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = send_json(
            self.client
                .post(&self.fetch_api_url)
                .header("X-API-Key", &self.api_key)
                .json(&json!({ "urls": [url], "format": "markdown" })),
            "TinyFish",
        )
        .await?;
        let result = response
            .get("results")
            .and_then(Value::as_array)
            .and_then(|items| items.first());
        let content = result
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                let detail = response
                    .get("errors")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("no content returned");
                HybridSearchError::Provider(format!("TinyFish fetch failed: {detail}"))
            })?;
        Ok(FetchedPage {
            content: content.to_string(),
            title: result
                .and_then(|item| item.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_date: None,
        })
    }
}

fn normalize_results(response: &Value) -> Vec<Source> {
    response
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let mut source = Source::new(item.get("url")?.as_str()?, "tinyfish");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item.get("snippet").and_then(Value::as_str) {
                source = source.with_description(description);
            }
            if let Some(date) = item.get("date").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}
