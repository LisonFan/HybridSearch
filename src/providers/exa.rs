use crate::error::{HybridSearchError, Result};
use crate::model::{FetchedPage, SearchFilters, Source};
use crate::providers::SourceProvider;
use crate::providers::http::send_json;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct ExaProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl ExaProvider {
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
                .header("x-api-key", &self.api_key)
                .json(body),
            "Exa",
        )
        .await
    }
}

#[async_trait]
impl SourceProvider for ExaProvider {
    fn name(&self) -> &'static str {
        "exa"
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
        let mut body = json!({
            "query": query,
            "numResults": max_results.clamp(1, 100)
        });
        let object = body.as_object_mut().expect("JSON object");
        if !filters.include_domains.is_empty() {
            object.insert("includeDomains".to_string(), json!(filters.include_domains));
        }
        if !filters.exclude_domains.is_empty() {
            object.insert("excludeDomains".to_string(), json!(filters.exclude_domains));
        }
        if let Some(days) = filters.recency_days {
            let date = (Utc::now() - Duration::days(i64::from(days)))
                .format("%Y-%m-%dT00:00:00.000Z")
                .to_string();
            object.insert("startPublishedDate".to_string(), json!(date));
        }
        let response = self.post("search", &body).await?;
        Ok(normalize_results(&response))
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = self
            .post("contents", &json!({ "urls": [url], "text": true }))
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
                HybridSearchError::Provider("Exa contents returned no content".to_string())
            })?;
        Ok(FetchedPage {
            content: content.to_string(),
            title: result
                .and_then(|item| item.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_date: result
                .and_then(|item| item.get("publishedDate"))
                .and_then(Value::as_str)
                .map(str::to_string),
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
            let mut source = Source::new(item.get("url")?.as_str()?, "exa");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item.get("summary").and_then(Value::as_str) {
                source = source.with_description(description);
            }
            if let Some(date) = item.get("publishedDate").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}
